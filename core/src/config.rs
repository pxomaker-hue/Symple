use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    /// Gauche → droite uniquement (miroir).
    Mirror,
    /// Les deux sens, le plus récent gagne.
    Bidirectional,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Demander à l'utilisateur (pop-up).
    Ask,
    /// Le plus récent gagne, l'autre version est conservée en copie.
    NewestWithBackup,
    /// Le plus récent gagne, sans copie.
    Newest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PairConfig {
    pub id: String,
    pub name: String,
    pub left: PathBuf,
    pub right: PathBuf,
    pub mode: SyncMode,
    pub propagate_deletes: bool,
    pub conflict: ConflictPolicy,
    pub excludes: Vec<String>,
    pub enabled: bool,
    /// Intervalle de vérification complète (secondes). Indispensable pour les NAS/SMB.
    pub poll_secs: u64,
    /// Nombre d'anciennes versions conservées pour chaque fichier écrasé (0 = aucune).
    pub versions: u32,
    /// Télécharger les fichiers « en ligne uniquement » (placeholders OneDrive/Nextcloud…) pour les copier.
    /// Désactivé : ils sont ignorés, sans jamais être supprimés ni écrasés.
    pub download_online_only: bool,
}

impl Default for PairConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            left: PathBuf::new(),
            right: PathBuf::new(),
            mode: SyncMode::Bidirectional,
            propagate_deletes: false,
            conflict: ConflictPolicy::NewestWithBackup,
            excludes: default_excludes(),
            enabled: true,
            poll_secs: 30,
            versions: 0,
            download_online_only: false,
        }
    }
}

pub fn default_excludes() -> Vec<String> {
    ["*.tmp", "~$*", "Thumbs.db", "desktop.ini", ".DS_Store", "*.part", "*.crdownload"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}
