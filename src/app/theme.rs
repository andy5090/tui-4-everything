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
        AppTheme::Future => ThemePalette {
            background: Color::Rgb(7, 16, 20),
            surface: Color::Rgb(11, 25, 30),
            foreground: Color::Rgb(228, 238, 240),
            accent: Color::Rgb(93, 225, 242),
            muted: Color::Rgb(166, 180, 183),
            selected: Color::Rgb(255, 122, 61),
            border: Color::Rgb(52, 70, 74),
            tab_foreground: Color::Rgb(154, 220, 227),
        },
        AppTheme::Amber => ThemePalette {
            background: Color::Rgb(18, 11, 0),
            surface: Color::Rgb(33, 21, 0),
            foreground: Color::Rgb(255, 224, 154),
            accent: Color::Rgb(255, 176, 0),
            muted: Color::Rgb(181, 121, 0),
            selected: Color::Rgb(255, 194, 71),
            border: Color::Rgb(116, 80, 0),
            tab_foreground: Color::Rgb(255, 210, 122),
        },
        AppTheme::GreenScreen => ThemePalette {
            background: Color::Rgb(2, 11, 4),
            surface: Color::Rgb(6, 22, 8),
            foreground: Color::Rgb(201, 247, 207),
            accent: Color::Rgb(74, 252, 114),
            muted: Color::Rgb(108, 168, 117),
            selected: Color::Rgb(128, 255, 153),
            border: Color::Rgb(40, 94, 50),
            tab_foreground: Color::Rgb(168, 238, 177),
        },
        AppTheme::Terracotta => ThemePalette {
            background: Color::Rgb(20, 20, 19),
            surface: Color::Rgb(42, 41, 38),
            foreground: Color::Rgb(250, 249, 245),
            accent: Color::Rgb(217, 119, 87),
            muted: Color::Rgb(176, 174, 165),
            selected: Color::Rgb(217, 119, 87),
            border: Color::Rgb(109, 107, 103),
            tab_foreground: Color::Rgb(232, 230, 220),
        },
    }
}

thread_local! {
    static ACTIVE_THEME: Cell<AppTheme> = const { Cell::new(AppTheme::Future) };
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
    fn future_theme_matches_the_web_future_palette() {
        assert_eq!(
            palette_for(AppTheme::Future),
            ThemePalette {
                background: Color::Rgb(7, 16, 20),
                surface: Color::Rgb(11, 25, 30),
                foreground: Color::Rgb(228, 238, 240),
                accent: Color::Rgb(93, 225, 242),
                muted: Color::Rgb(166, 180, 183),
                selected: Color::Rgb(255, 122, 61),
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
    fn amber_and_retro_green_use_classic_monochrome_crt_palettes() {
        let amber = palette_for(AppTheme::Amber);
        assert_eq!(amber.background, Color::Rgb(18, 11, 0));
        assert_eq!(amber.surface, Color::Rgb(33, 21, 0));
        assert_eq!(amber.foreground, Color::Rgb(255, 224, 154));
        assert_eq!(amber.accent, Color::Rgb(255, 176, 0));
        assert_eq!(amber.border, Color::Rgb(116, 80, 0));

        let retro = palette_for(AppTheme::GreenScreen);
        assert_eq!(retro.background, Color::Rgb(2, 11, 4));
        assert_eq!(retro.surface, Color::Rgb(6, 22, 8));
        assert_eq!(retro.foreground, Color::Rgb(201, 247, 207));
        assert_eq!(retro.accent, Color::Rgb(74, 252, 114));
        assert_eq!(retro.border, Color::Rgb(40, 94, 50));
    }

    #[test]
    fn terracotta_theme_uses_a_distinct_warm_clay_palette() {
        let terracotta = palette_for(AppTheme::Terracotta);

        assert_eq!(terracotta.background, Color::Rgb(20, 20, 19));
        assert_eq!(terracotta.surface, Color::Rgb(42, 41, 38));
        assert_eq!(terracotta.foreground, Color::Rgb(250, 249, 245));
        assert_eq!(terracotta.accent, Color::Rgb(217, 119, 87));
        assert_eq!(terracotta.muted, Color::Rgb(176, 174, 165));
        assert_eq!(terracotta.selected, Color::Rgb(217, 119, 87));
        assert_eq!(terracotta.border, Color::Rgb(109, 107, 103));
    }
}
