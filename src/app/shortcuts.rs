use crate::catalog::models::{KeyHint, Tool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturedShortcut {
    pub key: &'static str,
    pub action: &'static str,
}

pub const APP_VIEW_SHORTCUTS: &[CapturedShortcut] = &[
    CapturedShortcut {
        key: "Alt+Left",
        action: "previous app",
    },
    CapturedShortcut {
        key: "Alt+Right",
        action: "next app",
    },
    CapturedShortcut {
        key: "Alt+Backspace",
        action: "leave apps running",
    },
    CapturedShortcut {
        key: "Alt+Q",
        action: "close app",
    },
    CapturedShortcut {
        key: "Alt+O",
        action: "open link",
    },
    CapturedShortcut {
        key: "Alt+C",
        action: "copy link",
    },
    CapturedShortcut {
        key: "Alt+M",
        action: "toggle mouse controls",
    },
    CapturedShortcut {
        key: "Alt+K",
        action: "open key guide",
    },
];

pub const APP_VIEW_TOOLBAR_LABELS: &[&str] = &[
    "[Prev]",
    "[Next]",
    "[Background]",
    "[Close]",
    "[Open]",
    "[Copy]",
];

pub fn toolbar_action_at(column: u16) -> Option<usize> {
    let mut start = "T4E ".len() as u16;
    for (index, label) in APP_VIEW_TOOLBAR_LABELS.iter().enumerate() {
        let end = start.saturating_add(label.len() as u16);
        if (start..end).contains(&column) {
            return Some(index);
        }
        start = end.saturating_add(1);
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutConflict {
    pub key: &'static str,
    pub app_action: String,
    pub t4e_action: &'static str,
}

pub fn app_view_conflicts(tool: &Tool) -> Vec<ShortcutConflict> {
    let mut conflicts = Vec::new();
    for hint in &tool.key_hints {
        let KeyHint::Binding { keys, action } = hint else {
            continue;
        };
        for key in keys {
            let normalized = normalize_key(key);
            if let Some(captured) = APP_VIEW_SHORTCUTS
                .iter()
                .find(|captured| normalize_key(captured.key) == normalized)
                && !conflicts
                    .iter()
                    .any(|conflict: &ShortcutConflict| conflict.key == captured.key)
            {
                conflicts.push(ShortcutConflict {
                    key: captured.key,
                    app_action: action.clone(),
                    t4e_action: captured.action,
                });
            }
        }
    }
    conflicts
}

pub fn conflict_summary(tool: &Tool) -> String {
    let conflicts = app_view_conflicts(tool);
    if conflicts.is_empty() {
        if has_incomplete_key_guide(tool) {
            "Check incomplete: some app shortcuts are unknown".to_string()
        } else {
            "No documented conflicts with T4E".to_string()
        }
    } else {
        let keys = conflicts
            .iter()
            .map(|conflict| conflict.key)
            .collect::<Vec<_>>()
            .join(", ");
        let coverage = if has_incomplete_key_guide(tool) {
            " · additional keys unknown"
        } else {
            ""
        };
        format!("Conflict: {keys}{coverage} · use Shift+Alt to send the app key")
    }
}

pub fn has_incomplete_key_guide(tool: &Tool) -> bool {
    tool.key_hints.is_empty()
        || tool
            .key_hints
            .iter()
            .any(|hint| matches!(hint, KeyHint::Unknown { .. } | KeyHint::Legacy(_)))
}

fn normalize_key(key: &str) -> String {
    let compact = key
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    for prefix in ["m-", "meta+", "meta-", "option+", "option-", "alt-"] {
        if let Some(key) = compact.strip_prefix(prefix) {
            return format!("alt+{key}");
        }
    }
    compact
}

#[cfg(test)]
mod tests {
    use super::{normalize_key, toolbar_action_at};

    #[test]
    fn key_comparison_ignores_case_and_spacing() {
        assert_eq!(normalize_key(" Alt + Q "), normalize_key("alt+q"));
        assert_eq!(normalize_key("M-q"), normalize_key("Option+Q"));
        assert_eq!(normalize_key("Alt-Left"), normalize_key("Alt+Left"));
    }

    #[test]
    fn toolbar_hit_test_tracks_rendered_labels() {
        assert_eq!(toolbar_action_at(4), Some(0));
        assert_eq!(toolbar_action_at(11), Some(1));
        assert_eq!(toolbar_action_at(18), Some(2));
        assert_eq!(toolbar_action_at(31), Some(3));
        assert_eq!(toolbar_action_at(39), Some(4));
        assert_eq!(toolbar_action_at(46), Some(5));
        assert_eq!(toolbar_action_at(3), None);
    }
}
