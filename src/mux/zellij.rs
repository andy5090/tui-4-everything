use anyhow::Result;

use crate::mux::workspace::Workspace;

pub fn render_layout_kdl(workspace: &Workspace) -> Result<String> {
    let mut out = String::new();
    out.push_str("layout {\n");
    out.push_str("  pane split_direction=\"vertical\" {\n");

    for pane in &workspace.layout.panes {
        out.push_str(&format!(
            "    pane name=\"{}\" command=\"{}\"\n",
            pane.id, pane.cmd
        ));
    }

    out.push_str("  }\n");
    out.push_str("}\n");
    Ok(out)
}
