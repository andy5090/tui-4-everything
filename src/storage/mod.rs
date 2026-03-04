use std::path::{Path, PathBuf};

pub fn gate_report_path(root: &Path, gate_id: &str) -> PathBuf {
    root.join("artifacts").join("gates").join(format!("{}-report.json", gate_id))
}
