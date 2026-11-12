//! Configuration du démon : `~/.config/hyprpanel/config.toml` (chmod 600).
//!
//! Le jeton fait office d'authentification minimaliste : le serveur n'écoute
//! que sur 127.0.0.1 et refuse toute requête MCP sans `Authorization: Bearer
//! <jeton>` (ou `?key=<jeton>` en query, pour les clients sans en-têtes).

use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 8765;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub port: u16,
    /// Jeton hexadécimal 256 bits, comparé en temps constant.
    pub token: String,
    pub hypr_config_dir: PathBuf,
    pub backups_dir: PathBuf,
    pub screenshots_dir: PathBuf,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/root".into()))
}

fn xdg_config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
}

fn xdg_state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state"))
}

fn xdg_cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"))
}

/// Chemin du fichier de configuration du démon.
pub fn config_path() -> PathBuf {
    xdg_config_dir().join("hyprpanel/config.toml")
}

impl Settings {
    pub fn config_path(&self) -> PathBuf {
        config_path()
    }

    /// Valeurs par défaut : config Hyprland dans `~/.config/hypr`, backups
    /// dans le state XDG, captures dans le cache XDG.
    pub fn defaults() -> Self {
        Self {
            port: DEFAULT_PORT,
            token: String::new(),
            hypr_config_dir: home().join(".config/hypr"),
            backups_dir: xdg_state_dir().join("hyprpanel/backups"),
            screenshots_dir: xdg_cache_dir().join("hyprpanel/screenshots"),
        }
    }

    /// Génère un jeton depuis /dev/urandom (32 octets → 64 caractères hex).
    pub fn generate_token() -> String {
        use std::io::Read;
        let mut buf = [0u8; 32];
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            if f.read_exact(&mut buf).is_ok() {
                return buf.iter().map(|b| format!("{b:02x}")).collect();
            }
        }
        // repli dégradé (urandom indisponible) — jeton marqueur refusé au load
        // par la longueur, mais distinct d'un vrai jeton.
        format!("insecure-{}", std::process::id())
    }

    /// Charge la configuration ; erreur claire si absente (lancer `init`).
    pub fn load() -> Result<Self, String> {
        let path = config_path();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("impossible de lire {} : {e}", path.display()))?;
        let s: Settings =
            toml::from_str(&content).map_err(|e| format!("{} mal formé : {e}", path.display()))?;
        if s.token.len() < 32 {
            return Err(format!(
                "jeton trop court dans {} — relancez `hyprpanel init`",
                path.display()
            ));
        }
        Ok(s)
    }

    /// Crée la configuration si absente (ou si `force`), avec un jeton frais.
    /// Renvoie les réglages effectifs et `true` s'ils viennent d'être écrits.
    pub fn init(force: bool) -> Result<(Self, bool), String> {
        let path = config_path();
        if path.exists() && !force {
            return Ok((Self::load()?, false));
        }
        let mut s = Self::defaults();
        s.token = Self::generate_token();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, s.as_toml()).map_err(|e| e.to_string())?;
        restrict_permissions(&path);
        Ok((s, true))
    }

    pub fn as_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_serialisation() {
        let mut s = Settings::defaults();
        s.token = "a".repeat(64);
        let toml_str = s.as_toml();
        let reloaded: Settings = toml::from_str(&toml_str).unwrap();
        assert_eq!(reloaded.port, s.port);
        assert_eq!(reloaded.token, s.token);
        assert_eq!(reloaded.hypr_config_dir, s.hypr_config_dir);
    }

    #[test]
    fn jetons_generees_uniques_et_hex() {
        let a = Settings::generate_token();
        let b = Settings::generate_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
