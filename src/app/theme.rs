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
        AppTheme::Cyan => ThemePalette {
            background: Color::Rgb(7, 16, 20),
            surface: Color::Rgb(11, 25, 30),
            foreground: Color::Rgb(228, 238, 240),
            accent: Color::Rgb(93, 225, 242),
            muted: Color::Rgb(166, 180, 183),
            selected: Color::Rgb(93, 225, 242),
            border: Color::Rgb(52, 70, 74),
            tab_foreground: Color::Rgb(154, 220, 227),
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
        AppTheme::Terracotta => ThemePalette {
            background: Color::Rgb(28, 17, 16),
            surface: Color::Rgb(45, 25, 23),
            foreground: Color::Rgb(244, 221, 212),
            accent: Color::Rgb(233, 130, 99),
            muted: Color::Rgb(170, 104, 85),
            selected: Color::Rgb(242, 160, 127),
            border: Color::Rgb(142, 79, 63),
            tab_foreground: Color::Rgb(244, 221, 212),
        },
    }
}

thread_local! {
    static ACTIVE_THEME: Cell<AppTheme> = const { Cell::new(AppTheme::Cyan) };
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
    fn cyan_theme_matches_the_web_cyan_palette() {
        assert_eq!(
            palette_for(AppTheme::Cyan),
            ThemePalette {
                background: Color::Rgb(7, 16, 20),
                surface: Color::Rgb(11, 25, 30),
                foreground: Color::Rgb(228, 238, 240),
                accent: Color::Rgb(93, 225, 242),
                muted: Color::Rgb(166, 180, 183),
                selected: Color::Rgb(93, 225, 242),
                border: Color::Rgb(52, 70, 74),
                tab_foreground: Color::Rgb(154, 220, 227),
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

    #[test]
    fn terracotta_theme_uses_a_distinct_warm_clay_palette() {
        let terracotta = palette_for(AppTheme::Terracotta);

        assert_eq!(terracotta.background, Color::Rgb(28, 17, 16));
        assert_eq!(terracotta.surface, Color::Rgb(45, 25, 23));
        assert_eq!(terracotta.foreground, Color::Rgb(244, 221, 212));
        assert_eq!(terracotta.accent, Color::Rgb(233, 130, 99));
        assert_eq!(terracotta.selected, Color::Rgb(242, 160, 127));
        assert_ne!(terracotta.border, terracotta.background);
    }
}
