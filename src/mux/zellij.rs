use anyhow::{Result, bail};

use crate::mux::workspace::Workspace;

pub fn render_layout_kdl(workspace: &Workspace) -> Result<String> {
    if workspace.layout.panes.is_empty() {
        bail!("workspace has no panes");
    }

    let mut out = String::new();
    out.push_str("layout {\n");
    out.push_str("  pane name=\"root\" {\n");

    for pane in &workspace.layout.panes {
        out.push_str(&format!(
            "    pane name=\"{}\" split=\"{}\" direction=\"{:?}\" size=\"{}\" command=\"{}\"\n",
            pane.id,
            pane.split,
            pane.direction,
            pane.size,
            pane.cmd.replace('"', "\\\"")
        ));
    }

    out.push_str("  }\n");
    out.push_str("}\n");
    Ok(out)
}
