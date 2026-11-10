//! Spécificités Windows : chemins longs et détection des lecteurs réseau.

use std::path::{Path, PathBuf};

/// Joint un chemin relatif (séparé par `/`) à une racine.
pub fn join(root: &Path, rel: &str) -> PathBuf {
    if rel.is_empty() {
        return root.to_path_buf();
    }
    #[cfg(windows)]
    {
        root.join(rel.replace('/', "\\"))
    }
    #[cfg(not(windows))]
    {
        root.join(rel)
    }
}

/// Ajoute le préfixe `\\?\` pour dépasser la limite de 260 caractères de Windows.
pub fn long_path(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy().replace('/', "\\");
        if s.starts_with(r"\\?\") || s.is_empty() {
            return PathBuf::from(s);
        }
        let s = s.trim_end_matches('\\');
        if let Some(unc) = s.strip_prefix(r"\\") {
            return PathBuf::from(format!(r"\\?\UNC\{unc}"));
        }
        if s.len() >= 2 && s.as_bytes()[1] == b':' {
            return PathBuf::from(format!(r"\\?\{s}"));
        }
        PathBuf::from(s)
    }
    #[cfg(not(windows))]
    {
        p.to_path_buf()
    }
}

/// Partage réseau (`\\serveur\partage`) ou lecteur réseau mappé (`Z:`).
pub fn is_network(p: &Path) -> bool {
    let s = p.to_string_lossy();
    if s.starts_with(r"\\") || s.starts_with("//") {
        return !s.starts_with(r"\\?\") || s.starts_with(r"\\?\UNC\");
    }
    #[cfg(windows)]
    {
        let b = s.as_bytes();
        if b.len() >= 2 && b[1] == b':' {
            let root: Vec<u16> = format!("{}:\\", &s[..1]).encode_utf16().chain(Some(0)).collect();
            // DRIVE_REMOTE = 4
            return unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(root.as_ptr()) } == 4;
        }
    }
    false
}
