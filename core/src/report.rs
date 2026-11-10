use crate::engine::Meta;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    pub pair_id: String,
    pub rel: String,
    pub left: Meta,
    pub right: Meta,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictChoice {
    KeepLeft,
    KeepRight,
    KeepBoth,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Stats {
    pub files: usize,
    pub copied: usize,
    pub deleted: usize,
    pub moved: usize,
    pub conflicts: usize,
    pub errors: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Idle,
    Syncing,
    Paused,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairStatus {
    pub pair_id: String,
    pub state: State,
    /// Millisecondes Unix de la dernière synchro réussie.
    pub last_sync: Option<i64>,
    pub files: usize,
    pub conflicts: usize,
    pub watching: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Progress {
    pub pair_id: String,
    pub done_files: usize,
    pub total_files: usize,
    pub done_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreviewKind {
    ToRight,
    ToLeft,
    DeleteLeft,
    DeleteRight,
    MoveLeft,
    MoveRight,
    Conflict,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewItem {
    pub kind: PreviewKind,
    pub rel: String,
    pub size: u64,
}

/// Ce qu'une synchro ferait, sans rien modifier.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Preview {
    pub to_right: usize,
    pub to_left: usize,
    pub deletes: usize,
    pub moves: usize,
    pub conflicts: usize,
    pub bytes: u64,
    /// Liste détaillée (tronquée à `PREVIEW_LIMIT` éléments).
    pub items: Vec<PreviewItem>,
    pub truncated: bool,
}

pub const PREVIEW_LIMIT: usize = 500;

/// Interface entre le moteur et l'interface graphique.
pub trait Reporter: Send + Sync {
    fn log(&self, pair_id: &str, level: Level, msg: String);
    fn status(&self, status: &PairStatus);
    fn conflict(&self, conflict: &Conflict);
    fn progress(&self, _progress: &Progress) {}
}
