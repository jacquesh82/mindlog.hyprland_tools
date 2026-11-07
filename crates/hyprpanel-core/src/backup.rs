//! Sauvegardes horodatées des fichiers de configuration Hyprland.
//!
//! Un backup copie les `.lua`/`.conf` du premier niveau de `~/.config/hypr`
//! dans `<backups_dir>/<horodatage>[-<labele>/`. La restauration effectue
//! d'abord une sauvegarde de sécurité « pre-restore », puis recopie les
//! fichiers par-dessus la config active.

use std::path::{Path, PathBuf};

use crate::hypr_files;
use crate::timefmt;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("répertoire de configuration introuvable : {0}")]
    MissingConfigDir(PathBuf),
    #[error("backup « {0} » introuvable dans {1}")]
    UnknownBackup(String, PathBuf),
    #[error("nom de backup invalide : {0}")]
    InvalidName(String),
    #[error("E/S : {0}")]
    Io(#[from] std::io::Error),
}

fn config_files(dir: &Path) -> Vec<PathBuf> {
    hypr_files::scan(dir).into_iter().map(|f| PathBuf::from(f.path)).collect()
}

fn copy_all(files: &[PathBuf], dest: &Path) -> Result<(), BackupError> {
    std::fs::create_dir_all(dest)?;
    for f in files {
        let name = f.file_name().expect("chemin sans nom de fichier");
        std::fs::copy(f, dest.join(name))?;
    }
    Ok(())
}

/// Crée un backup et renvoie son nom (`YYYYmmdd-HHMMSS` ou `…-<label>`).
pub fn create_backup(
    config_dir: &Path,
    backups_dir: &Path,
    label: Option<&str>,
) -> Result<String, BackupError> {
    if !config_dir.is_dir() {
        return Err(BackupError::MissingConfigDir(config_dir.to_path_buf()));
    }
    let label: String = label
        .map(|l| {
            l.chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .take(32)
                .collect()
        })
        .filter(|l: &String| !l.is_empty())
        .map(|l| format!("-{l}"))
        .unwrap_or_default();
    let name = format!("{}{}", timefmt::compact(std::time::SystemTime::now()), label);
    let dest = backups_dir.join(&name);
    copy_all(&config_files(config_dir), &dest)?;
    Ok(name)
}

#[derive(Debug, serde::Serialize)]
pub struct BackupInfo {
    pub name: String,
    pub files: usize,
    pub size_bytes: u64,
    pub created: String,
}

pub fn list_backups(backups_dir: &Path) -> Vec<BackupInfo> {
    let Ok(entries) = std::fs::read_dir(backups_dir) else {
        return Vec::new();
    };
    let mut out: Vec<BackupInfo> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !is_valid_name(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let mut files = 0usize;
        let mut size = 0u64;
        if let Ok(inner) = std::fs::read_dir(&path) {
            for f in inner.flatten() {
                if let Ok(m) = f.metadata() {
                    if m.is_file() {
                        files += 1;
                        size += m.len();
                    }
                }
            }
        }
        out.push(BackupInfo {
            name: entry.file_name().to_string_lossy().into_owned(),
            files,
            size_bytes: size,
            created: entry
                .metadata()
                .and_then(|m| m.modified())
                .map(timefmt::iso)
                .unwrap_or_default(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.reverse(); // du plus récent au plus ancien
    out
}

/// Noms acceptés : alphanumérique, `-`, `_` — jamais de séparateur de chemin.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Restaure un backup dans la config active. Une sauvegarde « pre-restore »
/// est créée au préalable. Renvoie le nombre de fichiers restaurés.
pub fn restore_backup(
    config_dir: &Path,
    backups_dir: &Path,
    name: &str,
) -> Result<usize, BackupError> {
    if !is_valid_name(name) {
        return Err(BackupError::InvalidName(name.to_string()));
    }
    let src = backups_dir.join(name);
    if !src.is_dir() {
        return Err(BackupError::UnknownBackup(name.to_string(), backups_dir.to_path_buf()));
    }
    if !config_dir.is_dir() {
        return Err(BackupError::MissingConfigDir(config_dir.to_path_buf()));
    }
    create_backup(config_dir, backups_dir, Some("pre-restore"))?;
    let files = std::fs::read_dir(&src)?
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.path())
        .collect::<Vec<_>>();
    let count = files.len();
    copy_all(&files, config_dir)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hyprpanel-bkp-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn cycle_complet_backup_restore() {
        let config = tempdir("cfg");
        let backups = tempdir("bkps");
        fs::write(config.join("hyprland.lua"), "version 1").unwrap();
        fs::write(config.join("hyprpaper.conf"), "wp = a").unwrap();

        let b1 = create_backup(&config, &backups, None).unwrap();
        assert!(backups.join(&b1).join("hyprland.lua").is_file());
        assert_eq!(list_backups(&backups).len(), 1);

        // on modifie, puis on restaure
        fs::write(config.join("hyprland.lua"), "version 2 modifiée").unwrap();
        let n = restore_backup(&config, &backups, &b1).unwrap();
        assert_eq!(n, 2);
        assert_eq!(fs::read_to_string(config.join("hyprland.lua")).unwrap(), "version 1");
        // la sauvegarde de sécurité est là
        assert!(list_backups(&backups).iter().any(|b| b.name.ends_with("-pre-restore")));

        fs::remove_dir_all(&config).unwrap();
        fs::remove_dir_all(&backups).unwrap();
    }

    #[test]
    fn noms_invalides_rejetes() {
        let config = tempdir("cfg2");
        let backups = tempdir("bkps2");
        fs::write(config.join("hyprland.lua"), "x").unwrap();
        assert!(matches!(
            restore_backup(&config, &backups, "../escape"),
            Err(BackupError::InvalidName(_))
        ));
        assert!(matches!(
            restore_backup(&config, &backups, "inconnu"),
            Err(BackupError::UnknownBackup(_, _))
        ));
        // label nettoyé : seuls alphanumérique, `-` et `_` survivent
        let b = create_backup(&config, &backups, Some("test éà / injection")).unwrap();
        assert!(b.ends_with("-testinjection"), "label : {b}");
        fs::remove_dir_all(&config).unwrap();
        fs::remove_dir_all(&backups).unwrap();
    }
}
