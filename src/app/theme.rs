use std::cell::Cell;

use ratatui::style::Color;

use crate::storage::AppTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePalette {
    pub background: Color,
    pub surface: Color,
    pub foreground: Color,
    pub accent: Color,
    pub muted: Color,
    pub selected: Color,
    pub border: Color,
    pub tab_foreground: Color,
}

pub const fn palette_for(theme: AppTheme) -> ThemePalette {
    match theme {
        AppTheme::Default => ThemePalette {
            background: Color::Reset,
            surface: Color::Reset,
            foreground: Color::Reset,
            accent: Color::Cyan,
            muted: Color::DarkGray,
            selected: Color::Yellow,
            border: Color::Reset,
            tab_foreground: Color::Gray,
        },
        AppTheme::Amber => ThemePalette {
            background: Color::Rgb(23, 22, 13),
            surface: Color::Rgb(32, 29, 15),
            foreground: Color::Rgb(243, 234, 208),
            accent: Color::Rgb(255, 200, 90),
            muted: Color::Rgb(168, 124, 47),
            selected: Color::Rgb(255, 200, 90),
            border: Color::Rgb(117, 106, 73),
            tab_foreground: Color::Rgb(243, 234, 208),
        },
        AppTheme::GreenScreen => ThemePalette {
            background: Color::Rgb(4, 18, 8),
            surface: Color::Rgb(7, 28, 12),
            foreground: Color::Rgb(202, 244, 207),
            accent: Color::Rgb(101, 255, 129),
            muted: Color::Rgb(63, 132, 76),
            selected: Color::Rgb(163, 255, 169),
            border: Color::Rgb(55, 112, 65),
            tab_foreground: Color::Rgb(202, 244, 207),
        },
    }
}

thread_local! {
    static ACTIVE_THEME: Cell<AppTheme> = const { Cell::new(AppTheme::Default) };
}

pub(crate) fn activate(theme: AppTheme) {
    ACTIVE_THEME.set(theme);
}

pub(crate) fn active_palette() -> ThemePalette {
    palette_for(ACTIVE_THEME.get())
}

#[cfg(test)]
mod tests {
    use super::{ThemePalette, palette_for};
    use crate::storage::AppTheme;
    use ratatui::style::Color;

    #[test]
    fn default_theme_preserves_the_existing_terminal_palette() {
        assert_eq!(
            palette_for(AppTheme::Default),
            ThemePalette {
                background: Color::Reset,
                surface: Color::Reset,
                foreground: Color::Reset,
                accent: Color::Cyan,
                muted: Color::DarkGray,
                selected: Color::Yellow,
                border: Color::Reset,
                tab_foreground: Color::Gray,
            }
        );
    }

    #[test]
    fn phosphor_themes_define_distinct_full_palettes() {
        let amber = palette_for(AppTheme::Amber);
        let green = palette_for(AppTheme::GreenScreen);
        assert_ne!(amber.background, green.background);
        assert_ne!(amber.accent, green.accent);
        assert_ne!(amber.surface, amber.background);
        assert_ne!(green.surface, green.background);
    }
}
