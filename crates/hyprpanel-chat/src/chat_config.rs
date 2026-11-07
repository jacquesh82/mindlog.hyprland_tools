//! Configuration du client chat : `~/.config/hyprpanel/chat.toml` (chmod 600).
//!
//! `api_key` peut être initialisée depuis la config ZCode (provider Z.ai,
//! protocole Anthropic-compatible) — jamais affichée en clair dans les sorties.

use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    /// Optionnel ; sinon composé depuis la config du daemon (port + jeton).
    pub mcp_url: Option<String>,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/root".into()))
}

fn xdg_config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
}

pub fn config_path() -> PathBuf {
    xdg_config_dir().join("hyprpanel/chat.toml")
}

impl ChatConfig {
    pub fn defaults() -> Self {
        Self {
            base_url: "https://api.z.ai/api/anthropic".into(),
            api_key: String::new(),
            model: "GLM-5.3".into(),
            max_tokens: 8192,
            mcp_url: None,
        }
    }

    pub fn load() -> Result<Self, String> {
        let path = config_path();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("impossible de lire {} : {e}", path.display()))?;
        toml::from_str(&content).map_err(|e| format!("{} mal formé : {e}", path.display()))
    }

    /// Crée la config si absente. La clé est reprise de la config Zcode
    /// (provider Z.ai) quand elle y est disponible.
    pub fn init() -> Result<(Self, bool), String> {
        let path = config_path();
        if path.exists() {
            return Ok((Self::load()?, false));
        }
        let mut cfg = Self::defaults();
        if let Some(key) = zcode_api_key() {
            cfg.api_key = key;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, cfg.as_toml()).map_err(|e| e.to_string())?;
        restrict_permissions(&path);
        Ok((cfg, true))
    }

    pub fn load_or_init() -> Result<(Self, bool), String> {
        match Self::load() {
            Ok(c) => Ok((c, false)),
            Err(_) => Self::init(),
        }
    }

    pub fn as_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// URL MCP effective : `mcp_url` si définie, sinon composée depuis les
    /// réglages du daemon (port + jeton) via hyprpanel-core.
    pub fn mcp_url(&self) -> Result<String, String> {
        if let Some(url) = &self.mcp_url {
            return Ok(url.clone());
        }
        let settings = hyprpanel_core::Settings::load()?;
        Ok(format!("http://127.0.0.1:{}/mcp?key={}", settings.port, settings.token))
    }
}

/// Extrait la clé du provider Z.ai d'une config Zcode (JSON) — pur, testable.
pub fn extract_zai_key(zcode_json: &str) -> Option<String> {
    let root: serde_json::Value = serde_json::from_str(zcode_json).ok()?;
    let key = root
        .pointer("/provider/builtin:zai-coding-plan/options/apiKey")?
        .as_str()?
        .trim()
        .to_string();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

fn zcode_api_key() -> Option<String> {
    // La config Zcode vit dans ~/.zcode (et non ~/.config/zcode).
    let path = home().join(".zcode/cli/config.json");
    let content = std::fs::read_to_string(path).ok()?;
    extract_zai_key(&content)
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
    fn extraction_cle_zcode() {
        let cfg = r#"{"provider":{"builtin:zai-coding-plan":{"options":{"apiKey":"sk-test-123","baseURL":"https://api.z.ai/api/anthropic"}}}}"#;
        assert_eq!(extract_zai_key(cfg).as_deref(), Some("sk-test-123"));
        assert_eq!(extract_zai_key("{}"), None);
        assert_eq!(extract_zai_key("pas du tout du json"), None);
        // clé vide → None
        let vide = r#"{"provider":{"builtin:zai-coding-plan":{"options":{"apiKey":"  "}}}}"#;
        assert_eq!(extract_zai_key(vide), None);
    }

    #[test]
    fn cycle_toml() {
        let mut c = ChatConfig::defaults();
        c.api_key = "abc".into();
        c.mcp_url = Some("http://127.0.0.1:1/mcp?key=x".into());
        let reloaded: ChatConfig = toml::from_str(&c.as_toml()).unwrap();
        assert_eq!(reloaded.model, c.model);
        assert_eq!(reloaded.max_tokens, c.max_tokens);
        assert_eq!(reloaded.mcp_url.as_deref(), Some("http://127.0.0.1:1/mcp?key=x"));
    }
}
