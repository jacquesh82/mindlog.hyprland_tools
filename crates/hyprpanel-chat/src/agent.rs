//! Boucle d'agent : protocole Anthropic Messages en streaming (SSE),
//! exécution des outils via le client MCP, mise à jour de l'UI en direct.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Context};
use futures_util::StreamExt;
use serde_json::{json, Map, Value};
use slint::Weak;


use crate::chat_config::ChatConfig;
use crate::mcp::{self, McpSession};
use crate::ui;
use crate::ChatWindow;

/// Événements envoyés par l'UI vers la boucle agent.
pub enum UserEvent {
    Text(String),
    Reset,
}

const MAX_ROUNDS: usize = 12;
const TIMEOUT_LLM: Duration = Duration::from_secs(180);
const TIMEOUT_TOOL: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Machine à états SSE (pure, testée)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Map<String, Value>,
}

#[derive(Debug, Default)]
pub struct TurnOutput {
    pub text: String,
    pub thinking: String,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Default)]
struct SseState {
    output: TurnOutput,
    /// index de bloc → accumulateur (json brut pour les tool_use)
    tools: HashMap<usize, (String, String, String)>, // (id, nom, json accumulé)
}

/// Applique un événement SSE (JSON décodé) à l'état du tour.
fn apply_event(state: &mut SseState, event: &Value) -> Result<(), String> {
    let kind = event["type"].as_str().unwrap_or("");
    match kind {
        "content_block_start" => {
            let index = event["index"].as_u64().unwrap_or(0) as usize;
            if event["content_block"]["type"] == "tool_use" {
                state.tools.insert(
                    index,
                    (
                        event["content_block"]["id"].as_str().unwrap_or_default().to_string(),
                        event["content_block"]["name"].as_str().unwrap_or_default().to_string(),
                        String::new(),
                    ),
                );
            }
            Ok(())
        }
        "content_block_delta" => {
            let index = event["index"].as_u64().unwrap_or(0) as usize;
            match event["delta"]["type"].as_str().unwrap_or("") {
                "text_delta" => {
                    state.output.text.push_str(event["delta"]["text"].as_str().unwrap_or(""));
                    Ok(())
                }
                "thinking_delta" => {
                    state.output.thinking.push_str(event["delta"]["thinking"].as_str().unwrap_or(""));
                    Ok(())
                }
                "input_json_delta" => {
                    if let Some(entry) = state.tools.get_mut(&index) {
                        entry.2.push_str(event["delta"]["partial_json"].as_str().unwrap_or(""));
                    }
                    Ok(())
                }
                _ => Ok(()),
            }
        }
        "content_block_stop" => {
            let index = event["index"].as_u64().unwrap_or(0) as usize;
            if let Some((id, name, json_str)) = state.tools.remove(&index) {
                let input: Map<String, Value> = if json_str.trim().is_empty() {
                    Map::new()
                } else {
                    serde_json::from_str(&json_str).unwrap_or_else(|e| {
                        // JSON partiel/malformé : on transmet ce qu'on a dans un champ dédié
                        let mut m = Map::new();
                        m.insert("__json_brut".into(), Value::String(json_str.clone()));
                        m.insert("__erreur_parse".into(), Value::String(e.to_string()));
                        m
                    })
                };
                state.output.tool_calls.push(ToolCall { id, name, input });
            }
            Ok(())
        }
        "message_delta" => {
            if let Some(err) = event["delta"]["error"].as_object() {
                return Err(format!("erreur du modèle : {err:?}"));
            }
            Ok(())
        }
        "error" => Err(format!(
            "erreur du modèle : {}",
            event["error"]["message"].as_str().unwrap_or("inconnue")
        )),
        // message_start, message_stop, ping : rien à accumuler
        _ => Ok(()),
    }
}

/// Extrait les lignes complètes du tampon (`data: …`), pure et testé.
fn next_data_line(buf: &mut String) -> Option<String> {
    loop {
        let nl = buf.find('\n')?;
        let mut line: String = buf.drain(..=nl).collect();
        line.truncate(line.trim_end_matches(['\n', '\r']).len());
        if let Some(data) = line.strip_prefix("data: ") {
            return Some(data.to_string());
        }
        // lignes « event: », « : ping », vides… on continue
    }
}

// ---------------------------------------------------------------------------
// Boucle principale
// ---------------------------------------------------------------------------

pub async fn run(cfg: ChatConfig, mut rx: tokio::sync::mpsc::UnboundedReceiver<UserEvent>, weak: Weak<ChatWindow>) {
    if cfg.api_key.trim().is_empty() {
        ui::append(&weak, "error", "Clé API absente. Renseignez api_key dans ~/.config/hyprpanel/chat.toml puis relancez (ou `hyprpanel-chat init` si Zcode est configuré).");
        ui::set_status(&weak, "config incomplète");
    }

    let mcp = match cfg.mcp_url() {
        Ok(url) => match McpSession::connect(&url).await {
            Ok(session) => {
                ui::set_status(&weak, format!("MCP {} : {} outils", session.server_name().unwrap_or_default(), session.tools().len()));
                Some(session)
            }
            Err(e) => {
                ui::append(&weak, "error", format!("Serveur MCP injoignable : {e:#}"));
                ui::set_status(&weak, "MCP indisponible");
                None
            }
        },
        Err(e) => {
            ui::append(&weak, "error", format!("Config du daemon introuvable : {e} (lancez `hyprpanel init`)"));
            ui::set_status(&weak, "daemon non configuré");
            None
        }
    };

    let http = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(e) => {
            ui::append(&weak, "error", format!("Client HTTP : {e}"));
            return;
        }
    };

    let tools: Vec<Value> = mcp.as_ref().map(|m| m.tools().iter().map(mcp::tool_to_anthropic).collect()).unwrap_or_default();
    let system = build_system(mcp.as_ref().map(|m| m.instructions()).flatten().as_deref());

    let mut convo: Vec<Value> = Vec::new();

    while let Some(event) = rx.recv().await {
        match event {
            UserEvent::Reset => {
                convo.clear();
                ui::clear(&weak);
                ui::append(&weak, "status", "nouvelle conversation");
            }
            UserEvent::Text(text) => {
                ui::append(&weak, "user", &text);
                ui::set_busy(&weak, true);
                if let Err(e) = run_turn(&cfg, &http, mcp.as_ref(), &tools, &system, &mut convo, &text, &weak).await {
                    ui::append(&weak, "error", format!("{e:#}"));
                }
                ui::set_busy(&weak, false);
                ui::set_status(
                    &weak,
                    match &mcp {
                        Some(m) => format!("MCP {} : {} outils", m.server_name().unwrap_or_default(), m.tools().len()),
                        None => "MCP indisponible".to_string(),
                    },
                );
            }
        }
    }
}

fn build_system(server_instructions: Option<&str>) -> String {
    let mut system = String::from(
        "Tu es l'agent du « panneau de configuration Hyprland » dans une fenêtre de chat native. \
Réponds en français, de façon concise et actionnable. Tu configures l'interface de l'utilisateur \
en direct : utilise les outils disponibles plutôt que de longues explications. Préviens avec \
hypr_notify avant un changement visible important. Si une demande est ambiguë ou risquée \
(perte de données, écrans, saisie), demande confirmation.",
    );
    if let Some(instructions) = server_instructions {
        system.push_str("\n\n—— Instructions du serveur MCP ——\n");
        system.push_str(instructions);
    }
    system
}

async fn run_turn(
    cfg: &ChatConfig,
    http: &reqwest::Client,
    session: Option<&McpSession>,
    tools: &[Value],
    system: &str,
    convo: &mut Vec<Value>,
    user_text: &str,
    weak: &Weak<ChatWindow>,
) -> anyhow::Result<()> {
    convo.push(json!({"role": "user", "content": user_text}));

    for _round in 0..MAX_ROUNDS {
        ui::set_status(weak, "réflexion…");
        let response = request_stream(cfg, http, convo, tools, system).await?;
        let (output, bubble_created) = parse_stream(response, weak).await?;

        let mut content = Vec::new();
        if !output.text.is_empty() {
            content.push(json!({"type": "text", "text": output.text}));
        } else if bubble_created {
            // bulle créée puis restée vide (tour outils sans texte) : on la retire
            ui::remove_last(weak);
        }
        for tc in &output.tool_calls {
            content.push(json!({"type": "tool_use", "id": tc.id, "name": tc.name, "input": tc.input}));
        }
        if content.is_empty() {
            bail!("réponse du modèle sans contenu utilisable");
        }
        convo.push(json!({"role": "assistant", "content": content}));

        let Some(session) = session else {
            if !output.tool_calls.is_empty() {
                ui::append(weak, "error", "le modèle veut appeler un outil mais le serveur MCP est indisponible");
            }
            break;
        };
        if output.tool_calls.is_empty() {
            break;
        }

        let mut results = Vec::new();
        for tc in output.tool_calls {
            ui::set_status(weak, format!("outil {}…", tc.name));
            ui::append(weak, "tool", format!("→ {}({})", tc.name, compact_args(&tc.input)));
            let call = session.call(&tc.name, tc.input.clone());
            let out = match tokio::time::timeout(TIMEOUT_TOOL, call).await {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => format!("erreur outil : {e:#}"),
                Err(_) => format!("outil {} : timeout après 30 s", tc.name),
            };
            ui::append(weak, "tool", truncate_str(&out, 400));
            results.push(json!({"type": "tool_result", "tool_use_id": tc.id, "content": out}));
        }
        convo.push(json!({"role": "user", "content": results}));
    }
    Ok(())
}

async fn request_stream(
    cfg: &ChatConfig,
    http: &reqwest::Client,
    convo: &[Value],
    tools: &[Value],
    system: &str,
) -> anyhow::Result<reqwest::Response> {
    let body = json!({
        "model": cfg.model,
        "max_tokens": cfg.max_tokens,
        "system": system,
        "stream": true,
        "tools": tools,
        "messages": convo,
    });
    let url = format!("{}/v1/messages", cfg.base_url.trim_end_matches('/'));
    let fut = async {
        http.post(&url)
            .header("x-api-key", &cfg.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
    };
    let response = tokio::time::timeout(TIMEOUT_LLM, fut)
        .await
        .map_err(|_| anyhow::anyhow!("API : timeout après 180 s"))?
        .context("requête vers l'API LLM")?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!("API {status} : {}", truncate_str(&body, 300));
    }
    Ok(response)
}

/// Consomme le flux SSE : accumule le tour et streame le texte assistant
/// dans l'UI. Renvoie la sortie et `true` si une bulle assistant a été créée
/// (pour que l'appelant puisse la retirer si elle reste vide).
async fn parse_stream(response: reqwest::Response, weak: &Weak<ChatWindow>) -> anyhow::Result<(TurnOutput, bool)> {
    let mut stream = response.bytes_stream();
    let mut buf = String::new();
    let mut state = SseState::default();
    let mut bubble_created = false;

    loop {
        let chunk = tokio::select! {
            chunk = stream.next() => chunk,
            _ = tokio::time::sleep(TIMEOUT_LLM) => bail!("flux SSE : silence anormal"),
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk.context("lecture du flux SSE")?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(data) = next_data_line(&mut buf) {
            if data == "[DONE]" {
                continue;
            }
            let event: Value = match serde_json::from_str(&data) {
                Ok(v) => v,
                Err(_) => continue, // ligne non-JSON (keep-alive) : ignorée
            };
            apply_event(&mut state, &event).map_err(anyhow::Error::msg)?;
            if !state.output.text.is_empty() {
                if !bubble_created {
                    bubble_created = true;
                    ui::append(weak, "assistant", "");
                }
                ui::update_last(weak, "assistant", &state.output.text);
            }
        }
    }

    Ok((state.output, bubble_created))
}

fn compact_args(input: &Map<String, Value>) -> String {
    let joined: Vec<String> = input
        .iter()
        .map(|(k, v)| {
            let v = v.to_string();
            format!("{k}={}", truncate_str(&v, 60))
        })
        .collect();
    truncate_str(&joined.join(", "), 200)
}

pub fn truncate_str(s: &str, limit: usize) -> String {
    if s.len() <= limit {
        return s.to_string();
    }
    let mut cut = limit;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &s[..cut])
}

// ---------------------------------------------------------------------------
// Tests (SSE pur)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evenements_sse_nominaux() {
        let mut state = SseState::default();
        let events = [
            r#"{"type":"message_start","message":{}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Bonjour "}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"jacques"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
            r#"{"type":"message_stop"}"#,
        ];
        for e in events {
            apply_event(&mut state, &serde_json::from_str(e).unwrap()).unwrap();
        }
        assert_eq!(state.output.text, "Bonjour jacques");
        assert!(state.output.tool_calls.is_empty());
    }

    #[test]
    fn evenements_sse_appel_outil() {
        let mut state = SseState::default();
        let events = [
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"hypr_get_option"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"option\":"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"general:gaps_in\"}"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
        ];
        for e in events {
            apply_event(&mut state, &serde_json::from_str(e).unwrap()).unwrap();
        }
        assert_eq!(state.output.tool_calls.len(), 1);
        let tc = &state.output.tool_calls[0];
        assert_eq!(tc.id, "toolu_1");
        assert_eq!(tc.name, "hypr_get_option");
        assert_eq!(tc.input.get("option").and_then(Value::as_str), Some("general:gaps_in"));
    }

    #[test]
    fn json_partiel_outil_recupere() {
        let mut state = SseState::default();
        for e in [
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t2","name":"x"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"a\":1,"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
        ] {
            apply_event(&mut state, &serde_json::from_str(e).unwrap()).unwrap();
        }
        let tc = &state.output.tool_calls[0];
        assert!(tc.input.contains_key("__json_brut"));
    }

    #[test]
    fn erreur_modele_propagee() {
        let mut state = SseState::default();
        let e = r#"{"type":"error","error":{"type":"overloaded_error","message":"surchargé"}}"#;
        assert!(apply_event(&mut state, &serde_json::from_str(e).unwrap()).is_err());
    }

    #[test]
    fn lignes_data_extraites() {
        let mut buf = String::from("event: message\ndata: {\"a\":1}\n\n: ping\ndata: [DONE]\ndata: ");
        assert_eq!(next_data_line(&mut buf), Some("{\"a\":1}".to_string()));
        assert_eq!(next_data_line(&mut buf), Some("[DONE]".to_string()));
        assert_eq!(next_data_line(&mut buf), None); // dernier fragment incomplet
        buf.push_str("{\"b\":2}\n");
        assert_eq!(next_data_line(&mut buf), Some("{\"b\":2}".to_string()));
    }

    #[test]
    fn troncature_utf8() {
        assert_eq!(truncate_str("abc", 5), "abc");
        let s = "é".repeat(10); // 20 octets
        let t = truncate_str(&s, 11); // coupe dans un multi-octet
        assert!(t.ends_with('…') && t.len() < 14);
    }
}
