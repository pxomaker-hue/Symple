//! Filtres d'exclusion (motifs glob, insensibles à la casse).

use crate::engine::{TMP_SUFFIX, TRASH_DIR};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

pub struct Filter {
    names: GlobSet,
    paths: GlobSet,
}

impl Filter {
    pub fn new(patterns: &[String]) -> Self {
        let (mut names, mut paths) = (GlobSetBuilder::new(), GlobSetBuilder::new());
        for p in patterns.iter().map(|p| p.trim().trim_matches('/')).filter(|p| !p.is_empty()) {
            let Ok(g) = GlobBuilder::new(p).case_insensitive(true).literal_separator(true).build() else { continue };
            if p.contains('/') { paths.add(g) } else { names.add(g) };
        }
        Filter {
            names: names.build().unwrap_or_else(|_| GlobSet::empty()),
            paths: paths.build().unwrap_or_else(|_| GlobSet::empty()),
        }
    }

    /// `rel` : chemin relatif avec des `/`.
    pub fn ignored(&self, rel: &str) -> bool {
        let rel = rel.replace('\\', "/");
        let name = rel.rsplit('/').next().unwrap_or(&rel);
        name.ends_with(TMP_SUFFIX)
            || rel.split('/').any(|c| c == TRASH_DIR || self.names.is_match(c))
            || self.paths.is_match(&rel)
    }
}
