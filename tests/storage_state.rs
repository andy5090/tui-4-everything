use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use t4e::catalog::models::InstallMethod;
use t4e::installer::engine::InstallTask;
use t4e::installer::execution::InstallJob;
use t4e::storage::{
    LaunchOptionPreference, PersistentState, RecentItem, UserSettings, load_state, save_state,
};

fn temp_file() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("t4e-state-{}-{nonce}", std::process::id()))
        .join("state.json")
}

#[test]
fn persistent_state_round_trips_queue_and_logs() {
    let path = temp_file();
    let task = InstallTask {
        tool_id: "ripgrep".to_string(),
        method: InstallMethod::Apt,
        command: "apt-get install -y ripgrep".to_string(),
        check_command: None,
        additional_check_commands: Vec::new(),
        install_timeout_sec: None,
        requires_privileges: false,
        requires_confirmation: false,
        queued_at: Utc::now(),
    };
    let expected = PersistentState {
        queue: vec![InstallJob::new(task, "apt")],
        logs: vec!["queued ripgrep".to_string()],
        favorites: vec!["ripgrep".to_string()],
        recents: vec![RecentItem {
            id: "files-pack".to_string(),
            kind: "workspace".to_string(),
            timestamp: Utc::now(),
        }],
        settings: UserSettings {
            install_timeout_sec: 900,
            ..UserSettings::default()
        },
        launch_preferences: BTreeMap::from([(
            "cmatrix".to_string(),
            BTreeMap::from([(
                "color".to_string(),
                LaunchOptionPreference {
                    enabled: true,
                    value: Some("cyan".to_string()),
                },
            )]),
        )]),
    };

    save_state(&path, &expected).expect("state saves");
    let actual = load_state(&path).expect("state loads");

    assert_eq!(actual, expected);
    let _ = fs::remove_dir_all(path.parent().expect("state parent"));
}

#[test]
fn legacy_state_uses_defaults_for_new_user_fields() {
    let path = temp_file();
    fs::create_dir_all(path.parent().expect("state parent")).expect("directory creates");
    fs::write(&path, r#"{"queue":[],"logs":["legacy"]}"#).expect("legacy state writes");

    let actual = load_state(&path).expect("legacy state loads");

    assert!(actual.favorites.is_empty());
    assert!(actual.recents.is_empty());
    assert_eq!(actual.settings, UserSettings::default());
    assert!(actual.launch_preferences.is_empty());
    let _ = fs::remove_dir_all(path.parent().expect("state parent"));
}

#[test]
fn legacy_settings_enable_mouse_controls_when_the_field_is_missing() {
    let path = temp_file();
    fs::create_dir_all(path.parent().expect("state parent")).expect("directory creates");
    fs::write(
        &path,
        r#"{
            "settings": {
                "default_mux": "tmux",
                "install_timeout_sec": 600,
                "max_install_attempts": 2,
                "confirm_all_installs": false
            }
        }"#,
    )
    .expect("legacy state writes");

    let actual = load_state(&path).expect("legacy state loads");

    assert!(actual.settings.mouse_enabled);
    let _ = fs::remove_dir_all(path.parent().expect("state parent"));
}
