use std::io;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{Local, NaiveDateTime, Timelike, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// Color index 8 selects the animated rainbow palette.
pub const RAINBOW_COLOR: u8 = 8;
/// Color index 9 selects a single color whose hue cycles continuously.
pub const HUE_CYCLE_COLOR: u8 = 9;
/// How long a solid-to-solid hue transition runs.
const COLOR_ANIM_DURATION: Duration = Duration::from_millis(350);
/// Rainbow hue spread across the full clock width, in degrees.
const RAINBOW_SPREAD_DEGREES: f32 = 360.0;
/// Hue drift per second for the rainbow and hue-cycle palettes, in degrees.
const HUE_DRIFT_DEGREES: f32 = 45.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigClockOptions {
    pub seconds: bool,
    pub twelve_hour: bool,
    pub utc: bool,
    pub show_date: bool,
    /// ANSI color index 0-7, matching tty-clock's `-C` ordering, 8 for rainbow,
    /// or 9 for a hue-cycling color.
    pub color: u8,
    /// Stretch glyphs to fill the pane instead of preserving their proportions.
    pub stretch: bool,
}

pub fn run(options: BigClockOptions) -> Result<()> {
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    event_loop(&mut terminal, options)
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut options: BigClockOptions,
) -> Result<()> {
    let loop_start = Instant::now();
    let mut color_animation = None;
    let mut last_text = String::new();
    let mut dirty = true;
    loop {
        while event::poll(Duration::from_millis(0))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    KeyCode::Right | KeyCode::Char('c') => {
                        shift_color(&mut options, &mut color_animation, loop_start, 1);
                        dirty = true;
                    }
                    KeyCode::Left | KeyCode::Char('C') => {
                        shift_color(&mut options, &mut color_animation, loop_start, -1);
                        dirty = true;
                    }
                    _ => {}
                },
                Event::Resize(_, _) => {
                    terminal.autoresize()?;
                    dirty = true;
                }
                _ => {}
            }
        }
        let now = if options.utc {
            Utc::now().naive_utc()
        } else {
            Local::now().naive_local()
        };
        let phase = loop_start.elapsed().as_secs_f32() * HUE_DRIFT_DEGREES;
        let display = resolve_display(&options, &mut color_animation, phase, &mut dirty);
        let text = time_text(now, options);
        if dirty || text != last_text {
            terminal.draw(|frame| draw_clock(frame, &text, now, options, &display))?;
            last_text = text;
            dirty = false;
        }
        let tick = if color_animation.is_some() {
            Duration::from_millis(40)
        } else if options.color == RAINBOW_COLOR || options.color == HUE_CYCLE_COLOR {
            Duration::from_millis(80)
        } else {
            Duration::from_millis(100)
        };
        thread::sleep(tick);
    }
}

/// Applies one color step, starting a hue transition between solid colors.
/// Entering the rainbow palette switches instantly because it animates itself.
fn shift_color(
    options: &mut BigClockOptions,
    animation: &mut Option<ColorAnimation>,
    loop_start: Instant,
    direction: i8,
) {
    let now = Instant::now();
    let phase = loop_start.elapsed().as_secs_f32() * HUE_DRIFT_DEGREES;
    let from = current_rgb(options.color, animation, phase, now);
    options.color = cycle_color(options.color, direction);
    *animation = if options.color == RAINBOW_COLOR || options.color == HUE_CYCLE_COLOR {
        None
    } else {
        Some(ColorAnimation {
            from,
            to_index: options.color,
            started: now,
        })
    };
}

fn current_rgb(
    steady_index: u8,
    animation: &Option<ColorAnimation>,
    phase: f32,
    now: Instant,
) -> (u8, u8, u8) {
    if let Some(animation) = animation {
        return lerp_hue_rgb(
            animation.from,
            rgb_for_index(animation.to_index),
            animation.progress(now),
        );
    }
    if steady_index == RAINBOW_COLOR {
        return rainbow_rgb(0.5, phase);
    }
    if steady_index == HUE_CYCLE_COLOR {
        return rgb_from_hsv(phase, 1.0, 1.0);
    }
    rgb_for_index(steady_index)
}

/// Resolves what to draw this frame and keeps redrawing while the display is
/// animated; solid transitions snap back to the terminal palette color once
/// the hue animation completes.
fn resolve_display(
    options: &BigClockOptions,
    animation: &mut Option<ColorAnimation>,
    phase: f32,
    dirty: &mut bool,
) -> ClockDisplay {
    if options.color == RAINBOW_COLOR {
        *animation = None;
        *dirty = true;
        return ClockDisplay::Rainbow { phase };
    }
    if options.color == HUE_CYCLE_COLOR {
        *animation = None;
        *dirty = true;
        let (r, g, b) = rgb_from_hsv(phase, 1.0, 1.0);
        return ClockDisplay::Solid(Color::Rgb(r, g, b));
    }
    if let Some(active) = animation {
        let progress = active.progress(Instant::now());
        *dirty = true;
        if progress >= 1.0 {
            *animation = None;
        } else {
            let (r, g, b) = lerp_hue_rgb(active.from, rgb_for_index(active.to_index), progress);
            return ClockDisplay::Solid(Color::Rgb(r, g, b));
        }
    }
    ClockDisplay::Solid(ansi_color(options.color))
}

struct ColorAnimation {
    from: (u8, u8, u8),
    to_index: u8,
    started: Instant,
}

impl ColorAnimation {
    fn progress(&self, now: Instant) -> f32 {
        let t = now.saturating_duration_since(self.started).as_secs_f32()
            / COLOR_ANIM_DURATION.as_secs_f32();
        let t = t.clamp(0.0, 1.0);
        // smoothstep easing
        t * t * (3.0 - 2.0 * t)
    }
}

#[derive(Debug)]
enum ClockDisplay {
    Solid(Color),
    Rainbow { phase: f32 },
}

impl ClockDisplay {
    fn color_at(&self, x_fraction: f32) -> Color {
        match self {
            Self::Solid(color) => *color,
            Self::Rainbow { phase } => {
                let (r, g, b) = rainbow_rgb(x_fraction, *phase);
                Color::Rgb(r, g, b)
            }
        }
    }
}

fn rainbow_rgb(x_fraction: f32, phase: f32) -> (u8, u8, u8) {
    rgb_from_hsv(x_fraction * RAINBOW_SPREAD_DEGREES + phase, 1.0, 1.0)
}

fn draw_clock(
    frame: &mut ratatui::Frame,
    time: &str,
    now: NaiveDateTime,
    options: BigClockOptions,
    display: &ClockDisplay,
) {
    let area = frame.area();
    // Black digits get a white canvas so color 0 never renders an empty screen.
    if options.color == 0 {
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(Color::White));
    }
    let date = options
        .show_date
        .then(|| now.format("%Y-%m-%d").to_string());
    let mut time_scale = fit_scale(time, area.width, area.height, options.stretch);
    let mut date_scale = date.as_ref().map_or(time_scale, |date| {
        date_scale_for(date, time_scale, area.width)
    });
    while date.is_some()
        && GLYPH_ROWS as u16 * (time_scale.pixel_rows + date_scale.pixel_rows) + 1 > area.height
        && time_scale.pixel_rows > 1
    {
        if options.stretch {
            time_scale.pixel_rows -= 1;
        } else {
            let scale = time_scale.pixel_rows - 1;
            time_scale = GlyphScale {
                pixel_cols: 2 * scale,
                pixel_rows: scale,
            };
        }
        date_scale = date.as_ref().map_or(time_scale, |date| {
            date_scale_for(date, time_scale, area.width)
        });
    }

    let mut lines = render_glyphs(time, time_scale, display);
    if let Some(date) = &date {
        lines.push(Line::default());
        lines.extend(render_glyphs(date, date_scale, display));
    }
    let content_width = line_width(&lines);
    let target = centered_rect(
        content_width.min(area.width),
        (lines.len() as u16).min(area.height),
        area,
    );
    frame.render_widget(
        Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center),
        target,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}

fn time_text(now: NaiveDateTime, options: BigClockOptions) -> String {
    let hour24 = now.hour();
    let (hour, meridiem) = if options.twelve_hour {
        let hour12 = hour24 % 12;
        (if hour12 == 0 { 12 } else { hour12 }, Some(hour24 >= 12))
    } else {
        (hour24, None)
    };
    let minute = now.minute();
    let mut text = format!("{hour:02}:{minute:02}");
    if options.seconds {
        let second = now.second();
        text.push_str(&format!(":{second:02}"));
    }
    if let Some(pm) = meridiem {
        text.push_str(if pm { " PM" } else { " AM" });
    }
    text
}

fn cycle_color(current: u8, direction: i8) -> u8 {
    (current as i8 + direction).rem_euclid(10) as u8
}

/// Vivid RGB values used for hue transitions and the rainbow palette.
fn rgb_for_index(index: u8) -> (u8, u8, u8) {
    match index {
        0 => (0, 0, 0),
        1 => (255, 0, 0),
        2 => (0, 255, 0),
        3 => (255, 255, 0),
        4 => (0, 0, 255),
        5 => (255, 0, 255),
        6 => (0, 255, 255),
        _ => (255, 255, 255),
    }
}

/// Interpolates between two colors along the shortest hue arc in HSV space,
/// which reads as a smooth hue rotation between terminal palette colors.
fn lerp_hue_rgb(from: (u8, u8, u8), to: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let (h1, s1, v1) = hsv_from_rgb(from.0, from.1, from.2);
    let (h2, s2, v2) = hsv_from_rgb(to.0, to.1, to.2);
    // Achromatic endpoints adopt the other color's hue to avoid gray flashes.
    let (h1, h2) = if s1 == 0.0 && s2 != 0.0 {
        (h2, h2)
    } else if s2 == 0.0 && s1 != 0.0 {
        (h1, h1)
    } else {
        (h1, h2)
    };
    let delta = h2 - h1;
    let delta = if delta > 180.0 {
        delta - 360.0
    } else if delta < -180.0 {
        delta + 360.0
    } else {
        delta
    };
    let h = (h1 + delta * t).rem_euclid(360.0);
    let s = s1 + (s2 - s1) * t;
    let v = v1 + (v2 - v1) * t;
    rgb_from_hsv(h, s, v)
}

fn hsv_from_rgb(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / delta)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    (hue.rem_euclid(360.0), saturation, max)
}

fn rgb_from_hsv(hue: f32, saturation: f32, value: f32) -> (u8, u8, u8) {
    let hue = hue.rem_euclid(360.0);
    let chroma = value * saturation;
    let x = chroma * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (hue as u32) / 60 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = value - chroma;
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

fn ansi_color(index: u8) -> Color {
    match index {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        _ => Color::White,
    }
}

const GLYPH_ROWS: usize = 5;

fn glyph(ch: char) -> [&'static str; GLYPH_ROWS] {
    match ch {
        '0' => ["###", "# #", "# #", "# #", "###"],
        '1' => [" # ", "## ", " # ", " # ", "###"],
        '2' => ["###", "  #", "###", "#  ", "###"],
        '3' => ["###", "  #", "###", "  #", "###"],
        '4' => ["# #", "# #", "###", "  #", "  #"],
        '5' => ["###", "#  ", "###", "  #", "###"],
        '6' => ["###", "#  ", "###", "# #", "###"],
        '7' => ["###", "  #", "  #", "  #", "  #"],
        '8' => ["###", "# #", "###", "# #", "###"],
        '9' => ["###", "# #", "###", "  #", "###"],
        ':' => [" ", "#", " ", "#", " "],
        '-' => ["   ", "   ", "###", "   ", "   "],
        // AM/PM letters use wide thin letterforms so they read as text next
        // to the chunky digits.
        'A' => ["  #  ", " # # ", "#   #", "#####", "#   #"],
        'M' => ["#   #", "## ##", "# # #", "#   #", "#   #"],
        'P' => ["#### ", "#   #", "#### ", "#    ", "#    "],
        _ => ["   ", "   ", "   ", "   ", "   "],
    }
}

/// Pixel size of the rendered text at scale 1 (each pixel is 2x1 cells).
fn pixel_size(text: &str) -> (u16, u16) {
    let width = text
        .chars()
        .enumerate()
        .map(|(index, ch)| {
            let gap = u16::from(index > 0);
            gap + glyph(ch)[0].len() as u16
        })
        .sum::<u16>();
    (width, GLYPH_ROWS as u16)
}

/// Horizontal and vertical cell size of one font pixel. The axes scale
/// independently so the clock fills the pane on both dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GlyphScale {
    pixel_cols: u16,
    pixel_rows: u16,
}

/// Largest scale that fits the text inside the given cell bounds. With
/// `stretch` the axes scale independently to fill the pane; otherwise a
/// uniform scale keeps glyph proportions (one pixel = 2x1 cells).
fn fit_scale(text: &str, max_cols: u16, max_rows: u16, stretch: bool) -> GlyphScale {
    let (width, height) = pixel_size(text);
    if width == 0 || height == 0 || max_cols == 0 || max_rows == 0 {
        return GlyphScale {
            pixel_cols: 1,
            pixel_rows: 1,
        };
    }
    if stretch {
        return GlyphScale {
            pixel_cols: (max_cols / width).max(1),
            pixel_rows: (max_rows / height).max(1),
        };
    }
    let scale = (max_cols / (2 * width)).min(max_rows / height).max(1);
    GlyphScale {
        pixel_cols: 2 * scale,
        pixel_rows: scale,
    }
}

/// Secondary text (date) renders at half the time scale, capped to fit width.
fn date_scale_for(text: &str, time_scale: GlyphScale, max_cols: u16) -> GlyphScale {
    let (width, _) = pixel_size(text);
    let width_cap = max_cols.checked_div(width).unwrap_or(1).max(1);
    GlyphScale {
        pixel_cols: (time_scale.pixel_cols / 2).clamp(1, width_cap),
        pixel_rows: (time_scale.pixel_rows / 2).max(1),
    }
}

fn render_glyphs(text: &str, scale: GlyphScale, display: &ClockDisplay) -> Vec<Line<'static>> {
    let cell_width = scale.pixel_cols.max(1) as usize;
    let total_pixels = pixel_size(text).0.max(1) as f32;
    let mut lines = Vec::with_capacity(GLYPH_ROWS * scale.pixel_rows.max(1) as usize);
    for row in 0..GLYPH_ROWS {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut segment = String::new();
        let mut segment_color: Option<Color> = None;
        let mut x = 0_u16;
        let mut push_cell = |on: bool, spans: &mut Vec<Span<'static>>| {
            let cell_color = on.then(|| display.color_at((x as f32 + 0.5) / total_pixels));
            if cell_color != segment_color {
                if !segment.is_empty() {
                    let style = match segment_color {
                        Some(color) => Style::default().fg(color),
                        None => Style::default(),
                    };
                    spans.push(Span::styled(std::mem::take(&mut segment), style));
                }
                segment_color = cell_color;
            }
            let cell = if on { '█' } else { ' ' };
            segment.push_str(&cell.to_string().repeat(cell_width));
            x += 1;
        };
        for (index, ch) in text.chars().enumerate() {
            if index > 0 {
                push_cell(false, &mut spans);
            }
            for pixel in glyph(ch)[row].chars() {
                push_cell(pixel == '#', &mut spans);
            }
        }
        if !segment.is_empty() {
            let style = match segment_color {
                Some(color) => Style::default().fg(color),
                None => Style::default(),
            };
            spans.push(Span::styled(segment, style));
        }
        for _ in 0..scale.pixel_rows.max(1) {
            lines.push(Line::from(spans.clone()));
        }
    }
    lines
}

fn line_width(lines: &[Line<'static>]) -> u16 {
    lines
        .iter()
        .map(|line| line.width() as u16)
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPTIONS: BigClockOptions = BigClockOptions {
        seconds: false,
        twelve_hour: false,
        utc: false,
        show_date: false,
        color: 2,
        stretch: false,
    };

    #[test]
    fn time_text_uses_24_hour_clock_by_default() {
        let now = NaiveDateTime::parse_from_str("2026-07-27 15:04:09", "%Y-%m-%d %H:%M:%S")
            .expect("time parses");
        assert_eq!(time_text(now, OPTIONS), "15:04");
    }

    #[test]
    fn time_text_supports_seconds_and_12_hour_meridiem() {
        let now = NaiveDateTime::parse_from_str("2026-07-27 00:04:09", "%Y-%m-%d %H:%M:%S")
            .expect("time parses");
        let options = BigClockOptions {
            seconds: true,
            twelve_hour: true,
            ..OPTIONS
        };
        assert_eq!(time_text(now, options), "12:04:09 AM");
        let evening = NaiveDateTime::parse_from_str("2026-07-27 23:59:58", "%Y-%m-%d %H:%M:%S")
            .expect("time parses");
        assert_eq!(time_text(evening, options), "11:59:58 PM");
    }

    #[test]
    fn pixel_size_accounts_for_glyph_gaps() {
        assert_eq!(pixel_size("1"), (3, 5));
        assert_eq!(pixel_size("12:34"), (3 + 1 + 3 + 1 + 1 + 1 + 3 + 1 + 3, 5));
    }

    #[test]
    fn fit_scale_stretch_scales_axes_independently() {
        // "12:34" is 17px wide and 5px tall.
        assert_eq!(
            fit_scale("12:34", 17 * 4, 5 * 4, true),
            GlyphScale {
                pixel_cols: 4,
                pixel_rows: 4,
            }
        );
        assert_eq!(
            fit_scale("12:34", 17 * 4 - 1, 100, true),
            GlyphScale {
                pixel_cols: 3,
                pixel_rows: 20,
            }
        );
        assert_eq!(
            fit_scale("12:34", 100, 5 * 2 - 1, true),
            GlyphScale {
                pixel_cols: 5,
                pixel_rows: 1,
            }
        );
    }

    #[test]
    fn fit_scale_default_preserves_glyph_proportions() {
        // Uniform scale: one pixel = 2x1 cells, limited by the tighter axis.
        assert_eq!(
            fit_scale("12:34", 17 * 2 * 3, 5 * 3, false),
            GlyphScale {
                pixel_cols: 6,
                pixel_rows: 3,
            }
        );
        // Wide pane: height limits, width is letterboxed.
        assert_eq!(
            fit_scale("12:34", 200, 5 * 3, false),
            GlyphScale {
                pixel_cols: 6,
                pixel_rows: 3,
            }
        );
        // Tall pane: width limits, height is letterboxed.
        assert_eq!(
            fit_scale("12:34", 17 * 2 * 2, 100, false),
            GlyphScale {
                pixel_cols: 4,
                pixel_rows: 2,
            }
        );
        // Degenerate and tiny panes fall back to the minimum 2x1 pixel.
        assert_eq!(
            fit_scale("12:34", 4, 2, false),
            GlyphScale {
                pixel_cols: 2,
                pixel_rows: 1,
            }
        );
        assert_eq!(
            fit_scale("12:34", 0, 0, false),
            GlyphScale {
                pixel_cols: 1,
                pixel_rows: 1,
            }
        );
    }

    #[test]
    fn date_scale_halves_time_scale_and_caps_width() {
        let time_scale = GlyphScale {
            pixel_cols: 10,
            pixel_rows: 6,
        };
        // "2026-07-27" is 39px wide.
        assert_eq!(
            date_scale_for("2026-07-27", time_scale, 400),
            GlyphScale {
                pixel_cols: 5,
                pixel_rows: 3,
            }
        );
        assert_eq!(
            date_scale_for("2026-07-27", time_scale, 39 * 3),
            GlyphScale {
                pixel_cols: 3,
                pixel_rows: 3,
            }
        );
        assert_eq!(
            date_scale_for("2026-07-27", time_scale, 10),
            GlyphScale {
                pixel_cols: 1,
                pixel_rows: 3,
            }
        );
    }

    #[test]
    fn am_pm_letters_use_thin_wide_letterforms() {
        for ch in ['A', 'M', 'P'] {
            assert_eq!(glyph(ch)[0].len(), 5, "{ch} should be 5px wide");
        }
        // Thin letterforms keep interior whitespace instead of solid blobs.
        assert_eq!(glyph('M')[1], "## ##");
        assert!(glyph('M')[2].contains(' '));
        assert_eq!(glyph('A')[0], "  #  ");
    }

    #[test]
    fn render_glyphs_scales_pixels_to_cells() {
        let scale = GlyphScale {
            pixel_cols: 6,
            pixel_rows: 3,
        };
        let display = ClockDisplay::Solid(Color::Green);
        let lines = render_glyphs("1", scale, &display);
        assert_eq!(lines.len(), 15);
        assert_eq!(line_width(&lines), 18);
        let bottom: String = lines[14]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(bottom, "█".repeat(18));
    }

    #[test]
    fn cycle_color_wraps_through_all_palettes_in_both_directions() {
        assert_eq!(cycle_color(2, 1), 3);
        assert_eq!(cycle_color(7, 1), RAINBOW_COLOR);
        assert_eq!(cycle_color(RAINBOW_COLOR, 1), HUE_CYCLE_COLOR);
        assert_eq!(cycle_color(HUE_CYCLE_COLOR, 1), 0);
        assert_eq!(cycle_color(0, -1), HUE_CYCLE_COLOR);
        assert_eq!(cycle_color(HUE_CYCLE_COLOR, -1), RAINBOW_COLOR);
        assert_eq!(cycle_color(3, -1), 2);
    }

    #[test]
    fn black_digits_render_on_a_white_canvas() {
        let backend = ratatui::backend::TestBackend::new(20, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let options = BigClockOptions {
            color: 0,
            ..OPTIONS
        };
        let display = ClockDisplay::Solid(ansi_color(0));
        let now = NaiveDateTime::parse_from_str("2026-07-27 15:04:09", "%Y-%m-%d %H:%M:%S")
            .expect("time parses");
        terminal
            .draw(|frame| draw_clock(frame, "15:04", now, options, &display))
            .expect("draws");
        let cell = terminal
            .backend()
            .buffer()
            .cell((0, 0))
            .expect("corner cell");
        assert_eq!(cell.bg, Color::White);
    }

    #[test]
    fn hue_cycle_display_is_an_animated_solid_color() {
        let options = BigClockOptions {
            color: HUE_CYCLE_COLOR,
            ..OPTIONS
        };
        let mut animation = None;
        let mut dirty = false;
        let display = resolve_display(&options, &mut animation, 120.0, &mut dirty);
        assert!(dirty, "hue cycle keeps redrawing");
        let ClockDisplay::Solid(Color::Rgb(r, g, b)) = display else {
            panic!("expected a solid truecolor, got {display:?}");
        };
        let (hue, _, _) = hsv_from_rgb(r, g, b);
        assert!(
            (115.0..=125.0).contains(&hue),
            "phase 120° should render hue near 120°, got {hue}"
        );
    }

    #[test]
    fn hsv_roundtrip_preserves_primary_colors() {
        for rgb in [
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            (255, 255, 0),
            (255, 0, 255),
            (0, 255, 255),
        ] {
            let (h, s, v) = hsv_from_rgb(rgb.0, rgb.1, rgb.2);
            let back = rgb_from_hsv(h, s, v);
            assert!(
                back.0.abs_diff(rgb.0) <= 1
                    && back.1.abs_diff(rgb.1) <= 1
                    && back.2.abs_diff(rgb.2) <= 1,
                "{rgb:?} -> {back:?}"
            );
        }
    }

    #[test]
    fn hue_lerp_matches_endpoints() {
        let from = rgb_for_index(1);
        let to = rgb_for_index(6);
        assert_eq!(lerp_hue_rgb(from, to, 0.0), from);
        assert_eq!(lerp_hue_rgb(from, to, 1.0), to);
    }

    #[test]
    fn hue_lerp_takes_the_shortest_arc() {
        // Magenta (300°) to red (0°) rotates through 330°, never through green.
        let mid = lerp_hue_rgb(rgb_for_index(5), rgb_for_index(1), 0.5);
        let (hue, _, _) = hsv_from_rgb(mid.0, mid.1, mid.2);
        assert!(
            (320.0..=340.0).contains(&hue),
            "midpoint hue should be near 330°, got {hue}"
        );
    }

    #[test]
    fn hue_lerp_to_black_fades_without_hue_jump() {
        // Fading red to black keeps the red hue instead of sweeping the wheel.
        let mid = lerp_hue_rgb(rgb_for_index(1), rgb_for_index(0), 0.5);
        let (hue, _, value) = hsv_from_rgb(mid.0, mid.1, mid.2);
        assert!(hue <= 1.0 || hue >= 359.0, "hue drifted to {hue}");
        assert!((0.4..=0.6).contains(&value));
    }

    #[test]
    fn rainbow_color_varies_across_the_clock() {
        let display = ClockDisplay::Rainbow { phase: 0.0 };
        let left = display.color_at(0.0);
        let middle = display.color_at(0.33);
        let right = display.color_at(0.66);
        assert!(left != middle && middle != right && left != right);
        assert_eq!(display.color_at(1.0), left);
    }

    #[test]
    fn render_glyphs_draws_colon_separator() {
        let scale = GlyphScale {
            pixel_cols: 2,
            pixel_rows: 1,
        };
        let display = ClockDisplay::Solid(Color::Green);
        let lines = render_glyphs("1:2", scale, &display);
        assert_eq!(line_width(&lines), 18);
        let row_zero: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(row_zero, "  ██        ██████");
        let row_one: String = lines[1]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(row_one, "████    ██      ██");
    }

    #[test]
    fn render_glyphs_rainbow_paints_segments_with_distinct_colors() {
        let scale = GlyphScale {
            pixel_cols: 2,
            pixel_rows: 1,
        };
        let display = ClockDisplay::Rainbow { phase: 0.0 };
        let lines = render_glyphs("88888", scale, &display);
        let colors: Vec<_> = lines[0]
            .spans
            .iter()
            .filter_map(|span| span.style.fg)
            .collect();
        assert!(colors.len() > 2, "expected a gradient, got {colors:?}");
        let first = colors.first();
        assert!(colors.iter().any(|color| Some(color) != first));
    }
}
