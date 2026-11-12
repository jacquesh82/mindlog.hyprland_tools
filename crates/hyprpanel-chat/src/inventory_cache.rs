//! Cache de l'inventaire : empreinte du référentiel installé.
//!
//! L'inventaire (bases de paquets, requêtes confirmées, lanceurs, configurations)
//! est coûteux et peut demander des confirmations : il n'est relancé que si le
//! référentiel a changé depuis la dernière observation, ou sur demande explicite
//! (« Actualiser l'environnement »). L'empreinte ne lit AUCUN contenu : seulement
//! les métadonnées (date de modification, taille, cible de lien) des bases que
//! chaque gestionnaire réécrit à l'installation, la mise à jour ou la suppression.
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

/// Incrémenter quand la forme de l'inventaire change : invalide tous les caches.
const FORMAT: u32 = 1;
/// Borne des entrées énumérées dans un répertoire de référentiel (pipx, xbps).
const MAX_LISTED: usize = 512;

/// Bases surveillées, relatives à la racine système.
const SYSTEM_PATHS: &[&str] = &[
    // pacman : répertoire réécrit (pkg-version/) à chaque transaction.
    "var/lib/pacman/local",
    // dpkg
    "var/lib/dpkg/status",
    // rpm (emplacements historique et sysimage)
    "var/lib/rpm",
    "usr/lib/sysimage/rpm",
    // apk
    "lib/apk/db/installed",
    // portage
    "var/db/pkg",
    // flatpak : `.changed` est touché par flatpak à chaque modification.
    "var/lib/flatpak/.changed",
    "var/lib/flatpak/app",
    // snap : un fichier .snap par révision installée.
    "var/lib/snapd/snaps",
    // nix (profils système et par utilisateur)
    "nix/var/nix/profiles",
    "nix/var/nix/profiles/default",
    // homebrew : `opt/` est relinké à chaque installation/mise à jour.
    "opt/homebrew/opt",
    "opt/homebrew/Caskroom",
    "usr/local/opt",
    "usr/local/Caskroom",
    "home/linuxbrew/.linuxbrew/opt",
    "home/linuxbrew/.linuxbrew/Caskroom",
];

/// Bases utilisateur, relatives au HOME.
const HOME_PATHS: &[&str] = &[
    ".local/share/flatpak/.changed",
    ".local/share/flatpak/app",
    ".nix-profile",
    ".local/state/nix/profiles",
    ".local/share/pipx/venvs",
];

fn state(path: &Path) -> String {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return "absent".into();
    };
    let mut out = format!("{}.{}:{}", meta.mtime(), meta.mtime_nsec(), meta.size());
    if meta.file_type().is_symlink() {
        // Profils Nix : une génération = une nouvelle cible.
        if let Ok(target) = std::fs::canonicalize(path) {
            out.push_str(&format!("->{}", target.display()));
        }
        if let Ok(meta) = std::fs::metadata(path) {
            out.push_str(&format!("@{}.{}", meta.mtime(), meta.mtime_nsec()));
        }
    }
    out
}

/// Entrées d'un répertoire, triées et bornées ; `filter` retient les noms utiles.
fn listed(directory: &Path, filter: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| filter(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .take(MAX_LISTED)
        .collect();
    entries.sort();
    entries
}

/// Empreinte du référentiel installé — déterministe pour un état donné.
///
/// `managers` et `config_dir` en font partie : un nouveau gestionnaire détecté
/// ou un autre dossier de configuration autorisé invalide aussi le cache.
pub fn fingerprint(
    system_root: &Path,
    home: Option<&Path>,
    managers: &[String],
    config_dir: &Path,
) -> String {
    let mut lines = vec![
        format!("format\t{FORMAT}\t{}", env!("CARGO_PKG_VERSION")),
        format!("managers\t{}", managers.join(",")),
        format!("config_dir\t{}", config_dir.display()),
    ];
    let mut watch = |label: &str, path: PathBuf| {
        lines.push(format!("{label}\t{}\t{}", path.display(), state(&path)));
    };
    for relative in SYSTEM_PATHS {
        watch("system", system_root.join(relative));
    }
    // xbps : pkgdb-<version>.plist, nom variable selon la version.
    for path in listed(&system_root.join("var/db/xbps"), |n| {
        n.starts_with("pkgdb-") && n.ends_with(".plist")
    }) {
        watch("xbps", path);
    }
    for directory in crate::inventory::launcher_dirs(system_root) {
        watch("launchers", directory);
    }
    if let Some(home) = home {
        for relative in HOME_PATHS {
            watch("home", home.join(relative));
        }
        // pipx : une mise à jour réécrit les métadonnées de chaque venv.
        for venv in listed(&home.join(".local/share/pipx/venvs"), |_| true) {
            watch("pipx", venv.join("pipx_metadata.json"));
        }
    }
    let cargo = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".cargo")));
    if let Some(cargo) = cargo {
        watch("cargo", cargo.join(".crates.toml"));
        watch("cargo", cargo.join(".crates2.json"));
    }
    let mut hasher = Sha256::new();
    for line in &lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Empreinte de la machine courante.
pub fn current(managers: &[String], config_dir: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    fingerprint(Path::new("/"), home.as_deref(), managers, config_dir)
}

/// Le profil mémorisé reste valable tant que l'empreinte n'a pas bougé.
pub fn reusable(cached: Option<&str>, current: &str) -> bool {
    cached.is_some_and(|cached| cached == current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let home = dir.path().join("home");
        std::fs::create_dir_all(root.join("var/lib/pacman/local/zsh-5.9-1")).unwrap();
        std::fs::create_dir_all(root.join("usr/share/applications")).unwrap();
        std::fs::create_dir_all(home.join(".local/share/pipx/venvs/black")).unwrap();
        std::fs::write(
            home.join(".local/share/pipx/venvs/black/pipx_metadata.json"),
            "{}",
        )
        .unwrap();
        (dir, root, home)
    }

    fn print(root: &Path, home: &Path) -> String {
        fingerprint(root, Some(home), &["pacman".into()], &home.join(".config"))
    }

    #[test]
    fn unchanged_referential_keeps_the_cache() {
        let (_dir, root, home) = fixture();
        let first = print(&root, &home);
        assert_eq!(first, print(&root, &home));
        assert!(reusable(Some(&first), &print(&root, &home)));
        assert!(
            !reusable(None, &first),
            "profil sans empreinte : inventaire"
        );
    }

    #[test]
    fn package_transaction_invalidates_the_cache() {
        let (_dir, root, home) = fixture();
        let before = print(&root, &home);
        // Mise à jour pacman : l'ancien répertoire disparaît, le nouveau arrive.
        std::fs::remove_dir(root.join("var/lib/pacman/local/zsh-5.9-1")).unwrap();
        std::fs::create_dir(root.join("var/lib/pacman/local/zsh-5.9-2")).unwrap();
        // Des systèmes de fichiers ont une résolution d'1 s : forcer la date.
        let local = std::fs::File::open(root.join("var/lib/pacman/local")).unwrap();
        local
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        assert_ne!(before, print(&root, &home));
    }

    #[test]
    fn new_launcher_pipx_or_manager_invalidates_the_cache() {
        let (_dir, root, home) = fixture();
        let before = print(&root, &home);
        std::fs::write(
            home.join(".local/share/pipx/venvs/black/pipx_metadata.json"),
            "{\"version\":2}",
        )
        .unwrap();
        let after_pipx = print(&root, &home);
        assert_ne!(before, after_pipx);
        std::fs::create_dir_all(root.join("var/lib/flatpak")).unwrap();
        std::fs::write(root.join("var/lib/flatpak/.changed"), "").unwrap();
        let after_flatpak = print(&root, &home);
        assert_ne!(after_pipx, after_flatpak);
        let other_managers = fingerprint(
            &root,
            Some(&home),
            &["pacman".into(), "flatpak".into()],
            &home.join(".config"),
        );
        assert_ne!(after_flatpak, other_managers);
    }

    #[test]
    fn approved_config_directory_is_part_of_the_fingerprint() {
        let (_dir, root, home) = fixture();
        let other = fingerprint(&root, Some(&home), &["pacman".into()], &home);
        assert_ne!(print(&root, &home), other);
    }
}
