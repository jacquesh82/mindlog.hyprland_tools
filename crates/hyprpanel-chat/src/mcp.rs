//! Client MCP du daemon hyprpanel (rmcp, streamable HTTP).
//!
//! L'URL porte le jeton en query (`?key=…`) : la query passe telle quelle
//! à reqwest, ce qui évite tout support d'en-têtes personnalisés.

use anyhow::Context;
use rmcp::model::{CallToolRequestParams, ClientConfig, Tool};
use rmcp::service::RunningService;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Map, Value, json};

pub struct McpSession {
    client: RunningService<RoleClient, ClientConfig>,
    tools: Vec<Tool>,
}

impl McpSession {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(url.to_string()),
        );
        let client = ClientConfig::default()
            .serve(transport)
            .await
            .context("connexion au serveur MCP hyprpanel (le daemon tourne ? systemctl --user status hyprpanel)")?;
        let tools = client.list_all_tools().await.context("list_tools MCP")?;
        Ok(Self { client, tools })
    }

    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// Instructions du serveur (workflow Hyprland) reçues au handshake.
    pub fn instructions(&self) -> Option<String> {
        self.client
            .peer_info()
            .as_ref()
            .and_then(|p| p.instructions.clone())
    }

    pub fn server_name(&self) -> Option<String> {
        self.client
            .peer_info()
            .as_ref()
            .and_then(|p| p.server_info.as_ref().map(|s| s.name.clone()))
    }

    pub async fn call(&self, name: &str, args: Map<String, Value>) -> anyhow::Result<String> {
        let result = self
            .client
            .call_tool(CallToolRequestParams::new(name.to_string()).with_arguments(args))
            .await
            .with_context(|| format!("appel de l'outil {name}"))?;
        let text: Vec<String> = result
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect();
        let mut out = text.join("\n");
        if result.is_error == Some(true) {
            out = format!("l'outil a signalé une erreur :\n{out}");
        }
        Ok(out)
    }
}

/// Convertit une définition d'outil MCP vers le format « tools » de l'API
/// Anthropic Messages ({name, description, input_schema}).
pub fn tool_to_anthropic(tool: &Tool) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description.as_deref().unwrap_or(""),
        "input_schema": Value::Object(tool.input_schema.as_ref().clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tool est #[non_exhaustive] : on le construit par désérialisation.
    fn fake_tool(name: &str, desc: &str, schema_json: &str) -> Tool {
        let schema: Map<String, Value> = serde_json::from_str(schema_json).unwrap();
        serde_json::from_value(json!({
            "name": name,
            "description": desc,
            "inputSchema": schema,
        }))
        .unwrap()
    }

    #[test]
    fn conversion_outil_vers_anthropic() {
        let t = fake_tool(
            "hypr_get_option",
            "Lit une option.",
            r#"{"type":"object","properties":{"option":{"type":"string"}},"required":["option"]}"#,
        );
        let v = tool_to_anthropic(&t);
        assert_eq!(v["name"], "hypr_get_option");
        assert_eq!(v["description"], "Lit une option.");
        assert_eq!(v["input_schema"]["type"], "object");
        assert_eq!(v["input_schema"]["required"][0], "option");
    }
}
