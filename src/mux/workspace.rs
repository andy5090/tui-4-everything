use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceRegistry {
    pub workspaces: Vec<Workspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub title: String,
    pub mux: MuxBackend,
    pub session_name: Option<String>,
    pub layout: Layout,
    #[serde(default)]
    pub recommended_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Layout {
    pub panes: Vec<Pane>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    pub split: String,
    pub direction: SplitDirection,
    pub size: String,
    pub cmd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MuxBackend {
    #[serde(rename = "tmux")]
    Tmux,
    #[serde(rename = "zellij")]
    Zellij,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SplitDirection {
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "right")]
    Right,
    #[serde(rename = "up")]
    Up,
    #[serde(rename = "down")]
    Down,
}
