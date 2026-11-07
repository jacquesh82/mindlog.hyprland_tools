//! Inventaire des fichiers de configuration Hyprland (~/.config/hypr).
//!
//! La configuration active est `hyprland.lua` (API `hl.*`). `monitors.lua` et
//! `inputs.lua` sont **générés par hyprdmc** : lecture seule pour nous — les
//! modifier à la main serait écrasé au prochain passage du démon.

use std::path::Path;

/// Fichiers appartenant à un outil externe (hyprdmc) : ne jamais éditer.
pub const MANAGED_EXTERNALLY: &[(&str, &str)] = &[("monitors.lua", "hyprdmc"), ("inputs.lua", "hyprdmc")];

#[derive(Debug, serde::Serialize)]
pub struct ConfigFileInfo {
    pub name: String,
    pub path: String,
    /// `main-lua` | `generated-lua` | `lua` | `conf` | `theme`
    pub kind: &'static str,
    /// Nom de l'outil qui régénère ce fichier, s'il y en a un.
    pub managed_by: Option<&'static str>,
    pub size_bytes: u64,
    pub modified: String,
}

fn classify(name: &str) -> (&'static str, Option<&'static str>) {
    if let Some((_, owner)) = MANAGED_EXTERNALLY.iter().find(|(n, _)| *n == name) {
        return ("generated-lua", Some(*owner));
    }
    match name {
        "hyprland.lua" => ("main-lua", None),
        "dracula.conf" | "mocha.conf" => ("theme", None),
        n if n.ends_with(".lua") => ("lua", None),
        n if n.ends_with(".conf") => ("conf", None),
        _ => ("other", None),
    }
}

/// Liste les fichiers de config Hyprland du premier niveau du répertoire.
pub fn scan(dir: &Path) -> Vec<ConfigFileInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<ConfigFileInfo> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let (kind, managed_by) = classify(&name);
        if kind == "other" {
            continue;
        }
        let meta = entry.metadata().ok();
        out.push(ConfigFileInfo {
            name,
            path: path.to_string_lossy().into_owned(),
            kind,
            managed_by,
            size_bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            modified: meta
                .and_then(|m| m.modified().ok())
                .map(crate::timefmt::iso)
                .unwrap_or_default(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[derive(Debug, serde::Serialize)]
pub struct SettingMatch {
    pub file: String,
    pub line: usize,
    pub text: String,
}

/// Cherche une aiguille (insensible à la casse) dans les `.lua`/`.conf` du
/// répertoire et renvoie les lignes correspondantes, `file:line` inclus.
pub fn find_in_configs(dir: &Path, needle: &str, max: usize) -> Vec<SettingMatch> {
    let needle = needle.to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut files: Vec<_> = scan(dir).into_iter().map(|f| f.path).collect();
    files.sort();
    for file in files {
        let Ok(content) = std::fs::read_to_string(&file) else { continue };
        for (i, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                out.push(SettingMatch {
                    file: file.clone(),
                    line: i + 1,
                    text: line.trim().to_string(),
                });
                if out.len() >= max {
                    return out;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hyprpanel-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn scan_classe_les_fichiers() {
        let d = tempdir("scan");
        fs::write(d.join("hyprland.lua"), "hl.config{}").unwrap();
        fs::write(d.join("monitors.lua"), "-- généré").unwrap();
        fs::write(d.join("hyprpaper.conf"), "").unwrap();
        fs::write(d.join("dracula.conf"), "").unwrap();
        fs::write(d.join("binaire.png"), "").unwrap();
        fs::create_dir(d.join("sous-dossier")).unwrap();

        let files = scan(&d);
        let names: Vec<(&str, &'static str, Option<&str>)> = files
            .iter()
            .map(|f| (f.name.as_str(), f.kind, f.managed_by))
            .collect();
        assert_eq!(
            names,
            vec![
                ("dracula.conf", "theme", None),
                ("hyprland.lua", "main-lua", None),
                ("hyprpaper.conf", "conf", None),
                ("monitors.lua", "generated-lua", Some("hyprdmc")),
            ]
        );
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn find_repere_file_ligne() {
        let d = tempdir("find");
        fs::write(d.join("hyprland.lua"), "hl.config({\n  general = { gaps_in = 5 },\n})\n").unwrap();
        let m = find_in_configs(&d, "gaps_in", 10);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].line, 2);
        assert!(m[0].text.contains("gaps_in"));
        // insensible à la casse
        assert_eq!(find_in_configs(&d, "GAPS_IN", 10).len(), 1);
        // aiguille vide → rien
        assert!(find_in_configs(&d, "", 10).is_empty());
        fs::remove_dir_all(&d).unwrap();
    }
}
