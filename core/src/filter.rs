//! Filtres d'exclusion (motifs glob, insensibles à la casse).

use crate::engine::{TMP_SUFFIX, TRASH_DIR, VERSIONS_DIR};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

/// Fichiers et dossiers internes des clients cloud : jamais synchronisés.
/// (`.debris` = corbeille locale de MEGA, bases de synchro Nextcloud/ownCloud, caches Dropbox/Google Drive…)
pub const CLOUD_INTERNALS: &[&str] = &[
    ".debris",
    ".megaignore",
    ".sync_*.db*",
    "._sync_*.db*",
    ".owncloudsync.log*",
    ".nextcloudsync.log*",
    ".sync-exclude.lst",
    ".dropbox",
    ".dropbox.attr",
    ".dropbox.cache",
    ".tmp.drivedownload",
    ".tmp.driveupload",
    ".~lock.*#",
    // Resilio Sync / BitTorrent Sync
    ".sync",
    ".SyncArchive",
    ".SyncID",
    ".SyncIgnore",
    ".SyncTemp",
    "*.!sync",
    // Syncthing
    ".stfolder",
    ".stversions",
    ".stignore",
    ".syncthing.*.tmp",
];

pub struct Filter {
    names: GlobSet,
    paths: GlobSet,
    internals: GlobSet,
}

impl Filter {
    pub fn new(patterns: &[String]) -> Self {
        let (mut names, mut paths) = (GlobSetBuilder::new(), GlobSetBuilder::new());
        for p in patterns.iter().map(|p| p.trim().trim_matches('/')).filter(|p| !p.is_empty()) {
            let Ok(g) = GlobBuilder::new(p).case_insensitive(true).literal_separator(true).build() else { continue };
            if p.contains('/') { paths.add(g) } else { names.add(g) };
        }
        let mut internals = GlobSetBuilder::new();
        for p in CLOUD_INTERNALS {
            if let Ok(g) = GlobBuilder::new(p).case_insensitive(true).build() {
                internals.add(g);
            }
        }
        Filter {
            internals: internals.build().unwrap_or_else(|_| GlobSet::empty()),
            names: names.build().unwrap_or_else(|_| GlobSet::empty()),
            paths: paths.build().unwrap_or_else(|_| GlobSet::empty()),
        }
    }

    /// `rel` : chemin relatif avec des `/`.
    pub fn ignored(&self, rel: &str) -> bool {
        let rel = rel.replace('\\', "/");
        let name = rel.rsplit('/').next().unwrap_or(&rel);
        name.ends_with(TMP_SUFFIX)
            || rel.split('/').any(|c| c == TRASH_DIR || c == VERSIONS_DIR || self.internals.is_match(c) || self.names.is_match(c))
            || self.paths.is_match(&rel)
    }
}
