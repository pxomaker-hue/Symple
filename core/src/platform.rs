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

/// Dates de modification imprécises : réseau (SMB arrondit), FAT/exFAT (2 s), ou système inconnu.
/// Seuls les disques locaux NTFS/ReFS permettent une comparaison exacte.
pub fn coarse_mtime(p: &Path) -> bool {
    if is_network(p) {
        return true;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};
        let path: Vec<u16> = p.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
        let mut root = [0u16; 512];
        let mut fs_name = [0u16; 64];
        // SAFETY : tampons de taille annoncée, chaînes terminées par 0.
        let ok = unsafe {
            GetVolumePathNameW(path.as_ptr(), root.as_mut_ptr(), root.len() as u32) != 0
                && GetVolumeInformationW(
                    root.as_ptr(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    fs_name.as_mut_ptr(),
                    fs_name.len() as u32,
                ) != 0
        };
        if !ok {
            return true;
        }
        let n = fs_name.iter().position(|&c| c == 0).unwrap_or(fs_name.len());
        let name = String::from_utf16_lossy(&fs_name[..n]).to_ascii_uppercase();
        !(name == "NTFS" || name == "REFS")
    }
    #[cfg(not(windows))]
    {
        false
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
