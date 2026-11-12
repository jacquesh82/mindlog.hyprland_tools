//! Décisions mémorisées entre les sessions : accès aux fichiers et requêtes
//! d'inventaire (flatpak, pipx, cargo…). Fichier séparé de `chat.toml`
//! (`~/.config/hyprpanel/consents.toml`, chmod 600) : l'agent et la fenêtre
//! y écrivent chacun leurs clés sans écraser les réglages de l'autre.
//!
//! Absent ou illisible : rien n'est mémorisé, LYRA redemande.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Consents {
    /// Accès aux fichiers : `None` = demander au lancement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<bool>,
    /// Requêtes d'inventaire par gestionnaire : vrai = autorisée.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inventory: BTreeMap<String, bool>,
}

pub fn path() -> PathBuf {
    crate::chat_config::config_path()
        .parent()
        .map(|dir| dir.join("consents.toml"))
        .unwrap_or_else(|| PathBuf::from("consents.toml"))
}

pub fn load() -> Consents {
    load_from(&path())
}

pub fn load_from(path: &Path) -> Consents {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_to(path: &Path, consents: &Consents) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(consents).map_err(std::io::Error::other)?;
    std::fs::write(path, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Relit, modifie, réécrit : ne touche qu'à la clé concernée.
fn update(path: &Path, change: impl FnOnce(&mut Consents)) {
    let mut consents = load_from(path);
    change(&mut consents);
    if let Err(error) = save_to(path, &consents) {
        eprintln!(
            "consentements non enregistrés ({}) : {error}",
            path.display()
        );
    }
}

pub fn record_filesystem(approved: bool) {
    record_filesystem_at(&path(), approved);
}

pub fn record_filesystem_at(path: &Path, approved: bool) {
    update(path, |c| c.filesystem = Some(approved));
}

pub fn record_inventory(manager: &str, approved: bool) {
    record_inventory_at(&path(), manager, approved);
}

pub fn record_inventory_at(path: &Path, manager: &str, approved: bool) {
    update(path, |c| {
        c.inventory.insert(manager.to_string(), approved);
    });
}

/// Accès fichiers au lancement : l'option de la ligne de commande prime (sans
/// être enregistrée), puis la décision mémorisée ; `None` = demander.
pub fn startup_filesystem(option: Option<bool>, consents: &Consents) -> Option<bool> {
    option.or(consents.filesystem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_memorisees_et_relues() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hyprpanel/consents.toml");
        assert_eq!(load_from(&path), Consents::default());
        record_filesystem_at(&path, true);
        record_inventory_at(&path, "flatpak", true);
        record_inventory_at(&path, "pipx", false);
        let consents = load_from(&path);
        assert_eq!(consents.filesystem, Some(true));
        assert_eq!(consents.inventory.get("flatpak"), Some(&true));
        assert_eq!(consents.inventory.get("pipx"), Some(&false));
        // Une mise à jour ne touche qu'à sa clé.
        record_filesystem_at(&path, false);
        let consents = load_from(&path);
        assert_eq!(consents.filesystem, Some(false));
        assert_eq!(consents.inventory.len(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn fichier_illisible_redemande() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("consents.toml");
        std::fs::write(&path, "filesystem = \"peut-être\"").unwrap();
        assert_eq!(load_from(&path), Consents::default());
    }

    #[test]
    fn option_de_lancement_prioritaire() {
        let remembered = Consents {
            filesystem: Some(true),
            ..Default::default()
        };
        assert_eq!(startup_filesystem(None, &remembered), Some(true));
        assert_eq!(startup_filesystem(Some(false), &remembered), Some(false));
        assert_eq!(startup_filesystem(None, &Consents::default()), None);
    }
}
