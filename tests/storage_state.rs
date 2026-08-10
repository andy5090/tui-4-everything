use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use t4e::catalog::models::InstallMethod;
use t4e::installer::engine::InstallTask;
use t4e::installer::execution::InstallJob;
use t4e::storage::{
    AiApprovalMode, AppTheme, LaunchOptionPreference, PersistentState, ProviderAuthMode,
    RecentItem, UserSettings, default_api_provider_profiles, load_state, save_state,
};

fn temp_file() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir()
        .join(format!(
            "t4e-state-{}-{nonce}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
        .join("state.json")
}

#[test]
fn legacy_api_profiles_default_to_api_key_mode() {
    let profile: t4e::storage::ApiProviderProfile = serde_json::from_str(
        r#"{
            "label":"Legacy",
            "base_url":"https://example.com/v1",
            "model":"legacy-model",
            "api_key_env":"LEGACY_API_KEY"
        }"#,
    )
    .expect("legacy profile loads");

    assert_eq!(profile.auth_mode, ProviderAuthMode::ApiKey);
}

#[test]
fn legacy_ai_approval_modes_migrate_to_permission_modes() {
    let safe_only: AiApprovalMode = serde_json::from_str(r#""safe_only""#).expect("legacy auto");
    let all_bounded: AiApprovalMode =
        serde_json::from_str(r#""all_bounded""#).expect("legacy bypass");

    assert_eq!(safe_only, AiApprovalMode::Auto);
    assert_eq!(all_bounded, AiApprovalMode::Bypass);
    assert_eq!(
        UserSettings::default().ai_approval_mode,
        AiApprovalMode::Auto
    );
}

#[test]
fn themes_have_a_stable_default_and_serialized_names() {
    assert_eq!(UserSettings::default().theme, AppTheme::Future);
    assert_eq!(
        serde_json::to_string(&AppTheme::Future).unwrap(),
        r#""future""#
    );
    assert_eq!(
        serde_json::from_str::<AppTheme>(r#""default""#).unwrap(),
        AppTheme::Future
    );
    assert_eq!(
        serde_json::from_str::<AppTheme>(r#""cyan""#).unwrap(),
        AppTheme::Future
    );
    assert_eq!(
        serde_json::to_string(&AppTheme::Amber).unwrap(),
        r#""amber""#
    );
    assert_eq!(
        serde_json::from_str::<AppTheme>(r#""green_screen""#).unwrap(),
        AppTheme::GreenScreen
    );
    assert_eq!(AppTheme::GreenScreen.label(), "Retro Green");
    assert_eq!(
        serde_json::to_string(&AppTheme::Terracotta).unwrap(),
        r#""terracotta""#
    );
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
        expected_version: None,
        version_probe: None,
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
            preferred_ai_provider: "gemini".to_string(),
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
    assert!(actual.settings.preferred_ai_provider.is_empty());
    assert_eq!(
        actual.settings.api_providers,
        default_api_provider_profiles()
    );
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
    assert!(actual.settings.preferred_ai_provider.is_empty());
    assert_eq!(
        actual.settings.api_providers,
        default_api_provider_profiles()
    );
    let _ = fs::remove_dir_all(path.parent().expect("state parent"));
}
