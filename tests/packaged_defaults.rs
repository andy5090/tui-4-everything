use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn binary_validates_embedded_registry_outside_source_tree() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let working_dir = std::env::temp_dir().join(format!("t4e-packaged-defaults-{nonce}"));
    fs::create_dir_all(&working_dir).expect("create isolated directory");

    let output = Command::new(env!("CARGO_BIN_EXE_t4e"))
        .arg("validate")
        .current_dir(&working_dir)
        .output()
        .expect("run packaged binary");
    let _ = fs::remove_dir_all(&working_dir);

    assert!(
        output.status.success(),
        "validation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("validation ok"));
}
