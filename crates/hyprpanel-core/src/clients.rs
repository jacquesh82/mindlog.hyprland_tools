//! Détection de l'enregistrement du serveur MCP hyprpanel chez les clients
//! externes (Claude Code, ZCode), et commandes d'enregistrement associées.
//! Lecture seule : on n'écrit jamais dans leurs configurations.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ClientRegistrations {
    /// `hyprpanel` présent dans `mcpServers` global ou d'un projet de ~/.claude.json
    pub claude_code: bool,
    /// `hyprpanel` présent dans `mcp.servers` de ~/.zcode/cli/config.json
    pub zcode: bool,
}

impl ClientRegistrations {
    pub fn all_registered(&self) -> bool {
        self.claude_code && self.zcode
    }
}

/// Détecte `hyprpanel` dans le JSON de `~/.claude.json` (pur, testé) :
/// clé globale `mcpServers.hyprpanel` ou dans un projet `projects.*.mcpServers`.
pub fn claude_code_registered(claude_json: &str) -> bool {
    let Ok(root) = serde_json::from_str::<Value>(claude_json) else {
        return false;
    };
    if root["mcpServers"]["hyprpanel"].is_object() {
        return true;
    }
    let Some(projects) = root["projects"].as_object() else {
        return false;
    };
    projects
        .values()
        .any(|p| p["mcpServers"]["hyprpanel"].is_object())
}

/// Détecte `hyprpanel` dans le JSON de `~/.zcode/cli/config.json` (pur, testé).
pub fn zcode_registered(zcode_json: &str) -> bool {
    serde_json::from_str::<Value>(zcode_json)
        .map(|v| v["mcp"]["servers"]["hyprpanel"].is_object())
        .unwrap_or(false)
}

fn read(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn home() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/root".into()))
}

/// Détection réelle sur cette machine (fichiers absents = non enregistré).
pub fn detect_clients() -> ClientRegistrations {
    ClientRegistrations {
        claude_code: read(&home().join(".claude.json"))
            .map(|s| claude_code_registered(&s))
            .unwrap_or(false),
        zcode: read(&home().join(".zcode/cli/config.json"))
            .map(|s| zcode_registered(&s))
            .unwrap_or(false),
    }
}

/// Commandes d'enregistrement prêtes à coller.
pub fn registration_snippets(token: &str, port: u16) -> RegistrationSnippets {
    RegistrationSnippets {
        claude_code: format!(
            "claude mcp add --scope user --transport http hyprpanel http://127.0.0.1:{port}/mcp --header \"Authorization: Bearer {token}\""
        ),
        zcode: format!(
            "\"mcp\": {{ \"servers\": {{ \"hyprpanel\": {{ \"url\": \"http://127.0.0.1:{port}/mcp?key={token}\" }} }} }}  (dans ~/.zcode/cli/config.json)"
        ),
    }
}

#[derive(Debug, Clone)]
pub struct RegistrationSnippets {
    pub claude_code: String,
    pub zcode: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_claude_code() {
        let global = r#"{"mcpServers":{"hyprpanel":{"type":"http","url":"http://127.0.0.1:8765/mcp"}}}"#;
        assert!(claude_code_registered(global));
        let projet = r#"{"projects":{"/home/x/proj":{"mcpServers":{"hyprpanel":{"url":"x"}}}}}"#;
        assert!(claude_code_registered(projet));
        // absent ou malformé
        assert!(!claude_code_registered("{}"));
        assert!(!claude_code_registered(r#"{"mcpServers":{"autre":{}}}"#));
        assert!(!claude_code_registered("pas du tout du json"));
    }

    #[test]
    fn detection_zcode() {
        let cfg = r#"{"mcp":{"servers":{"hyprpanel":{"url":"http://127.0.0.1:8765/mcp?key=x"}}}}"#;
        assert!(zcode_registered(cfg));
        assert!(!zcode_registered(r#"{"mcp":{"servers":{}}}"#));
        assert!(!zcode_registered("{}"));
        assert!(!zcode_registered("garbage"));
    }

    #[test]
    fn snippets_complets() {
        let s = registration_snippets("abc123", 8765);
        assert!(s.claude_code.contains("Bearer abc123"));
        assert!(s.zcode.contains("key=abc123"));
    }
}
