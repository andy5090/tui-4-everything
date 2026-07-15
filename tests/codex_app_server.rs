use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use t4e::codex::app_server::CodexAppServer;

fn fake_server() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("t4e-fake-codex-{nonce}.sh"));
    fs::write(
        &path,
        r#"#!/bin/sh
IFS= read -r initialize
printf '%s\n' '{"id":0,"result":{"userAgent":"fake","platformFamily":"unix","platformOs":"linux"}}'
IFS= read -r initialized
IFS= read -r account
printf '%s\n' '{"id":1,"result":{"account":{"type":"chatgpt","email":"test@example.com"}}}'
"#,
    )
    .expect("fake server writes");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).expect("permissions");
    }
    path
}

#[test]
fn client_performs_initialize_handshake_before_requests() {
    let path = fake_server();
    let mut client = CodexAppServer::spawn_command("sh", &[path.to_str().expect("path")])
        .expect("fake server starts");

    let initialized = client.initialize().expect("initializes");
    assert_eq!(initialized["platformOs"], "linux");
    let account = client.account_read().expect("account reads");
    assert_eq!(account["account"]["type"], "chatgpt");
    assert!(client.initialize().is_err());
    drop(client);
    let _ = fs::remove_file(path);
}

#[test]
fn installed_codex_app_server_accepts_current_protocol_when_available() {
    if Command::new("codex").arg("--version").output().is_err() {
        return;
    }
    let mut client = CodexAppServer::spawn().expect("Codex app-server starts");
    let initialized = client.initialize().expect("current protocol initializes");
    assert!(initialized.get("userAgent").is_some());
    let account = client.account_read().expect("current account status reads");
    assert!(account.get("account").is_some());
}
