//! Configuration du client chat : `~/.config/hyprpanel/chat.toml` (chmod 600).
//!
//! `api_key` peut être initialisée depuis la config ZCode (provider Z.ai,
//! protocole Anthropic-compatible) — jamais affichée en clair dans les sorties.

use std::path::PathBuf;

/// Endpoint officiel du GLM Coding Plan (pas de repli vers une API à l'usage).
pub const ZCODE_SUBSCRIPTION_URL: &str = "https://api.z.ai/api/anthropic";
pub const MAX_SYSTEM_CONTEXT_CHARS: usize = 16_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Zcode,
    ClaudeCode,
    Codex,
}

impl Provider {
    pub fn index(self) -> i32 {
        match self {
            Self::Zcode => 0,
            Self::ClaudeCode => 1,
            Self::Codex => 2,
        }
    }
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Zcode),
            1 => Some(Self::ClaudeCode),
            2 => Some(Self::Codex),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Zcode => "Zcode",
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatConfig {
    #[serde(default)]
    pub provider: Provider,
    /// Vide = modèle choisi par le client officiel pour cet abonnement.
    #[serde(default)]
    pub claude_model: String,
    #[serde(default)]
    pub codex_model: String,
    /// Vide = rôle LYRA par défaut ; sinon remplace ce rôle pour les trois modes.
    #[serde(default)]
    pub system_context: String,
    #[serde(default = "default_scope_limit")]
    pub limit_wayland_scope: bool,
    #[serde(default = "default_memory_enabled")]
    pub memory_enabled: bool,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    /// Langue de l'interface et du prompt système : "fr" (défaut) ou "en".
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_animate_avatar")]
    pub animate_avatar: bool,
    /// Optionnel ; sinon composé depuis la config du daemon (port + jeton).
    pub mcp_url: Option<String>,
}

impl ChatConfig {
    pub fn validate_subscription(&self) -> Result<(), String> {
        if self.provider == Provider::Zcode
            && self.base_url.trim_end_matches('/') != ZCODE_SUBSCRIPTION_URL
        {
            return Err("Zcode requires the GLM Coding Plan endpoint https://api.z.ai/api/anthropic; pay-as-you-go API fallback is disabled.".into());
        }
        Ok(())
    }

    pub fn active_model(&self) -> &str {
        match self.provider {
            Provider::Zcode => &self.model,
            Provider::ClaudeCode => &self.claude_model,
            Provider::Codex => &self.codex_model,
        }
    }

    pub fn set_active_model(&mut self, model: String) {
        match self.provider {
            Provider::Zcode => self.model = model,
            Provider::ClaudeCode => self.claude_model = model,
            Provider::Codex => self.codex_model = model,
        }
    }

    pub fn model_label(&self) -> String {
        self.active_model().to_string()
    }

    pub fn lang(&self) -> crate::i18n::Lang {
        crate::i18n::Lang::parse(&self.language)
    }
}

fn default_language() -> String {
    "fr".into()
}

fn default_animate_avatar() -> bool {
    true
}

fn default_scope_limit() -> bool {
    true
}

fn default_memory_enabled() -> bool {
    true
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

/// Registre des tours de conversation (restauration de config) :
/// `$XDG_STATE_HOME/hyprpanel/lyra-history.json`.
pub fn state_history_path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state"))
        .join("hyprpanel/lyra-history.json")
}

impl ChatConfig {
    pub fn defaults() -> Self {
        Self {
            provider: Provider::Zcode,
            claude_model: String::new(),
            codex_model: String::new(),
            system_context: String::new(),
            limit_wayland_scope: true,
            memory_enabled: true,
            base_url: ZCODE_SUBSCRIPTION_URL.into(),
            api_key: String::new(),
            model: "GLM-5.3".into(),
            max_tokens: 8192,
            language: "fr".into(),
            animate_avatar: true,
            mcp_url: None,
        }
    }

    /// Écrit la config (chmod 600) — appelé par le panneau de réglages.
    pub fn save(&self) -> Result<(), String> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, self.as_toml()).map_err(|e| e.to_string())?;
        restrict_permissions(&path);
        Ok(())
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
        Ok(format!(
            "http://127.0.0.1:{}/mcp?key={}",
            settings.port, settings.token
        ))
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
    if key.is_empty() { None } else { Some(key) }
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
    fn system_context_roundtrips_and_old_configs_keep_default_role() {
        let mut cfg = ChatConfig::defaults();
        let old = cfg
            .as_toml()
            .lines()
            .filter(|line| !line.starts_with("system_context"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            toml::from_str::<ChatConfig>(&old)
                .unwrap()
                .system_context
                .is_empty()
        );
        cfg.system_context = "You are my desktop assistant.\nPréférences : #7BDACD, compact layouts.\nKeep `commands` intact.".into();
        assert_eq!(
            toml::from_str::<ChatConfig>(&cfg.as_toml())
                .unwrap()
                .system_context,
            cfg.system_context
        );
    }

    #[test]
    fn zcode_never_falls_back_to_a_metered_api_endpoint() {
        let mut cfg = ChatConfig::defaults();
        assert!(cfg.validate_subscription().is_ok());
        cfg.base_url = "https://api.z.ai/api/paas/v4".into();
        assert!(cfg.validate_subscription().is_err());
        cfg.base_url = "https://api.anthropic.com".into();
        assert!(cfg.validate_subscription().is_err());
        cfg.provider = Provider::Codex;
        assert!(
            cfg.validate_subscription().is_ok(),
            "Codex uses its own ChatGPT session, not this endpoint"
        );
    }

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
        assert!(reloaded.animate_avatar);
        assert!(reloaded.memory_enabled);
        assert_eq!(
            reloaded.mcp_url.as_deref(),
            Some("http://127.0.0.1:1/mcp?key=x")
        );
    }

    #[test]
    fn memory_migrates_old_settings_and_can_be_disabled() {
        let mut cfg = ChatConfig::defaults();
        let old = cfg
            .as_toml()
            .lines()
            .filter(|line| !line.starts_with("memory_enabled"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(toml::from_str::<ChatConfig>(&old).unwrap().memory_enabled);
        cfg.memory_enabled = false;
        assert!(
            !toml::from_str::<ChatConfig>(&cfg.as_toml())
                .unwrap()
                .memory_enabled
        );
    }

    #[test]
    fn animation_config_ancienne_et_preference_persistante() {
        let mut cfg = ChatConfig::defaults();
        let old = cfg
            .as_toml()
            .lines()
            .filter(|line| !line.starts_with("animate_avatar"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(toml::from_str::<ChatConfig>(&old).unwrap().animate_avatar);
        cfg.animate_avatar = false;
        assert!(
            !toml::from_str::<ChatConfig>(&cfg.as_toml())
                .unwrap()
                .animate_avatar
        );
    }

    #[test]
    fn provider_defaults_migration_and_models_are_independent() {
        let mut cfg = ChatConfig::defaults();
        let old = cfg
            .as_toml()
            .lines()
            .filter(|line| {
                !["provider", "claude_model", "codex_model"]
                    .iter()
                    .any(|key| line.starts_with(key))
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            toml::from_str::<ChatConfig>(&old).unwrap().provider,
            Provider::Zcode
        );
        let glm = cfg.model.clone();
        cfg.provider = Provider::ClaudeCode;
        cfg.set_active_model("sonnet".into());
        cfg.provider = Provider::Codex;
        assert_eq!(cfg.active_model(), "");
        cfg.set_active_model("test-codex-model".into());
        let saved: ChatConfig = toml::from_str(&cfg.as_toml()).unwrap();
        assert_eq!(saved.provider, Provider::Codex);
        assert_eq!(saved.model, glm);
        assert_eq!(saved.claude_model, "sonnet");
        assert_eq!(saved.codex_model, "test-codex-model");
        for provider in [Provider::Zcode, Provider::ClaudeCode, Provider::Codex] {
            assert_eq!(Provider::from_index(provider.index()), Some(provider));
        }
        assert_eq!(Provider::from_index(-1), None);
        assert_eq!(Provider::from_index(3), None);
    }
}
#[test]
fn wayland_scope_defaults_on_for_old_configs_and_can_be_disabled() {
    let mut cfg = ChatConfig::defaults();
    assert!(cfg.limit_wayland_scope);
    let old = cfg
        .as_toml()
        .lines()
        .filter(|line| !line.starts_with("limit_wayland_scope"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        toml::from_str::<ChatConfig>(&old)
            .unwrap()
            .limit_wayland_scope
    );
    cfg.limit_wayland_scope = false;
    assert!(
        !toml::from_str::<ChatConfig>(&cfg.as_toml())
            .unwrap()
            .limit_wayland_scope
    );
}
