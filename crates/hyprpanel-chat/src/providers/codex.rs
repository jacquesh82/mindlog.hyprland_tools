//! Codex app-server : session ChatGPT et vrais événements de texte incrémentaux.
//! Le provider dédié n'a ni env_key ni endpoint API facturé à l'usage.
//!
//! Cache : Codex route le cache de prompt OpenAI par identifiant de thread
//! (`prompt_cache_key`). Un thread neuf à chaque appel ne relit presque rien
//! du cache ; LYRA garde donc le processus et son thread éphémère pendant la
//! conversation et n'envoie que les messages nouveaux.

use super::{
    CLI_TIMEOUT, Entry, OUTPUT_LIMIT, command, count, decode_reply, prepare_prompt, private_dir,
    reply_schema, streaming::TextStream,
};
use crate::chat_config::{ChatConfig, Provider};
use anyhow::{Context, bail, ensure};
use base64::Engine as _;
use langchain_rust::{
    language_models::{GenerateResult, TokenUsage},
    schemas::{FunctionDefinition, Message},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout},
};

const SUBSCRIPTION_PROVIDER: &str = "lyra_chatgpt_subscription";
/// Au-delà, le cache distant a expiré : repartir d'un processus neuf.
const IDLE_LIMIT: std::time::Duration = std::time::Duration::from_secs(30 * 60);
const DEVELOPER_INSTRUCTIONS: &str = "Follow LYRA’s response protocol. Return only the requested schema, with text first. All actions go through the declared LYRA tools. Each new user input appends conversation entries (JSON messages, oldest first) to the previous ones: user messages, LYRA’s record of your previous reply with the call ids it assigned, and tool results.";

struct AppServer {
    /// Conservé pour `kill_on_drop` : détruire la session arrête le processus.
    _child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
    received: usize,
}

impl AppServer {
    async fn send(&mut self, value: Value) -> anyhow::Result<()> {
        self.stdin
            .write_all((value.to_string() + "\n").as_bytes())
            .await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn receive(&mut self) -> anyhow::Result<Value> {
        loop {
            let line = self
                .stdout
                .next_line()
                .await?
                .context("Codex app-server a fermé le flux")?;
            self.received += line.len();
            ensure!(
                self.received <= OUTPUT_LIMIT,
                "sortie Codex trop volumineuse"
            );
            if let Ok(message) = serde_json::from_str::<Value>(&line) {
                return Ok(message);
            }
        }
    }

    async fn reject_unexpected_request(&mut self, event: &Value) -> anyhow::Result<()> {
        if event.get("id").is_some() && event.get("method").is_some() {
            self.send(json!({"id":event["id"],"error":{"code":-32601,"message":"LYRA executes tools itself; built-in actions are unavailable."}})).await?;
        }
        Ok(())
    }

    async fn rpc(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"id":id,"method":method,"params":params}))
            .await?;
        loop {
            let event = self.receive().await?;
            if event["id"] == id && event.get("method").is_none() {
                if let Some(error) = event.get("error") {
                    bail!(
                        "Codex {method} : {}",
                        error["message"].as_str().unwrap_or("requête refusée")
                    );
                }
                return event
                    .get("result")
                    .cloned()
                    .context("résultat RPC Codex absent");
            }
            self.reject_unexpected_request(&event).await?;
        }
    }
}

fn mcp_names(value: &Value) -> anyhow::Result<Vec<&str>> {
    let entries = value.as_array().context("liste MCP Codex invalide")?;
    entries
        .iter()
        .map(|entry| entry["name"].as_str().context("nom du serveur MCP absent"))
        .collect()
}

/// Thread Codex vivant pour la conversation en cours.
struct Session {
    server: AppServer,
    /// Répertoire privé (cwd, images) : vit aussi longtemps que le processus.
    dir: tempfile::TempDir,
    thread_id: String,
    system: String,
    model: String,
    /// Empreintes des messages déjà connus du thread, dans l'ordre.
    sent: Vec<u64>,
    used: std::time::Instant,
}

/// Une seule conversation par processus LYRA. La session est retirée pendant
/// un appel : un appel annulé (futur abandonné) ou en échec la détruit, et
/// `kill_on_drop` arrête le processus.
static SESSION: std::sync::Mutex<Option<Session>> = std::sync::Mutex::new(None);

fn slot() -> std::sync::MutexGuard<'static, Option<Session>> {
    SESSION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Nombre de messages déjà connus du thread si la conversation le prolonge
/// strictement (même système, même modèle, historique préfixe, au moins un
/// message nouveau) ; `None` impose un thread neuf.
fn reusable_prefix(
    sent: &[u64],
    session_system: &str,
    session_model: &str,
    system: &str,
    model: &str,
    entries: &[u64],
) -> Option<usize> {
    (session_system == system
        && session_model == model
        && entries.len() > sent.len()
        && entries.starts_with(sent))
    .then_some(sent.len())
}

impl Session {
    async fn start(cfg: &ChatConfig, system: &str) -> anyhow::Result<Self> {
        let dir = private_dir()?;
        // Résoudre les configurations héritées sans en exposer les clés ou URL.
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            command(Provider::Codex)
                .current_dir(dir.path())
                .args(["mcp", "list", "--json"])
                .output(),
        )
        .await
        .context("Codex : vérification MCP trop longue")??;
        ensure!(
            output.status.success(),
            "Codex : impossible d'isoler les outils ; vérifiez `codex mcp list --json`"
        );
        let inherited: Value =
            serde_json::from_slice(&output.stdout).context("configuration MCP Codex invalide")?;
        let mut cmd = command(Provider::Codex);
        cmd.current_dir(dir.path()).args([
            "app-server",
            "--listen",
            "stdio://",
            "-c",
            &format!("model_provider=\"{SUBSCRIPTION_PROVIDER}\""),
            "-c",
            &format!("model_providers.{SUBSCRIPTION_PROVIDER}.name=\"OpenAI\""),
            "-c",
            &format!("model_providers.{SUBSCRIPTION_PROVIDER}.wire_api=\"responses\""),
            "-c",
            &format!("model_providers.{SUBSCRIPTION_PROVIDER}.requires_openai_auth=true"),
            "-c",
            "forced_login_method=\"chatgpt\"",
            "-c",
            "features.shell_tool=false",
            "-c",
            "features.multi_agent=false",
            "-c",
            "features.image_generation=false",
            "-c",
            "features.view_image=false",
            "-c",
            "features.apps=false",
            "-c",
            "features.plugins=false",
            "-c",
            "project_doc_max_bytes=0",
            "-c",
            "web_search=\"disabled\"",
        ]);
        for name in mcp_names(&inherited)? {
            let key = serde_json::to_string(name)?;
            cmd.args(["-c", &format!("mcp_servers.{key}.enabled=false")]);
        }
        let mut child = cmd.spawn().context("lancement de Codex app-server")?;
        let stdin = child.stdin.take().context("stdin Codex")?;
        let stdout = BufReader::new(child.stdout.take().context("stdout Codex")?).lines();
        let mut server = AppServer {
            _child: child,
            stdin,
            stdout,
            next_id: 0,
            received: 0,
        };
        server.rpc("initialize", json!({"clientInfo":{"name":"lyra","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}})).await?;
        server
            .send(json!({"method":"initialized","params":{}}))
            .await?;
        let account = server
            .rpc("account/read", json!({"refreshToken":false}))
            .await?;
        ensure!(
            account["account"]["type"] == "chatgpt",
            "Codex requires a ChatGPT subscription login (`codex login`); API billing is disabled."
        );
        let model = if cfg.codex_model.is_empty() {
            let models = server.rpc("model/list", json!({})).await?;
            let choices = models["data"]
                .as_array()
                .context("catalogue Codex absent")?;
            let selected = choices
                .iter()
                .find(|m| m["isDefault"] == true)
                .or_else(|| choices.first())
                .context("aucun modèle Codex disponible")?;
            selected["model"]
                .as_str()
                .or_else(|| selected["id"].as_str())
                .context("modèle Codex absent")?
                .to_string()
        } else {
            cfg.codex_model.clone()
        };
        let thread = server
            .rpc(
                "thread/start",
                json!({
                    "model":model,"modelProvider":SUBSCRIPTION_PROVIDER,"cwd":dir.path(),
                    "approvalPolicy":"never","sandbox":"read-only","ephemeral":true,
                    "baseInstructions":system,"developerInstructions":DEVELOPER_INSTRUCTIONS,
                    "experimentalRawEvents":false
                }),
            )
            .await?;
        let thread_id = thread["thread"]["id"]
            .as_str()
            .context("identifiant de conversation Codex absent")?
            .to_string();
        Ok(Self {
            server,
            dir,
            thread_id,
            system: system.to_string(),
            model: cfg.codex_model.clone(),
            sent: Vec::new(),
            used: std::time::Instant::now(),
        })
    }

    /// Un tour Codex avec les seuls messages nouveaux.
    async fn turn(
        &mut self,
        entries: &[Entry],
        functions: &[FunctionDefinition],
        on_text: &mut impl FnMut(&str),
    ) -> anyhow::Result<GenerateResult> {
        let mut input = Vec::new();
        for entry in entries {
            input.push(json!({"type":"text","text":entry.text}));
            for image in &entry.images {
                let path = self.dir.path().join(image.file_name());
                std::fs::write(
                    &path,
                    base64::engine::general_purpose::STANDARD
                        .decode(&image.data)
                        .context("image base64")?,
                )?;
                input.push(json!({"type":"localImage","path":path}));
            }
        }
        let thread_id = self.thread_id.clone();
        let server = &mut self.server;
        // Les notifications de texte peuvent arriver avant la réponse turn/start :
        // envoyer sans rpc() et lire immédiatement l'ensemble du flux.
        server.next_id += 1;
        let turn_request = server.next_id;
        server
            .send(json!({"id":turn_request,"method":"turn/start","params":{
                "threadId":thread_id,"input":input,"outputSchema":reply_schema(functions)
            }}))
            .await?;
        let mut progress = TextStream::new(on_text);
        let mut raw = String::new();
        let mut item_id = String::new();
        let mut reply = None;
        let mut usage = None;
        loop {
            let event = server.receive().await?;
            if event["id"] == turn_request && event.get("error").is_some() {
                bail!(
                    "Codex n'a pas pu démarrer la réponse : {}",
                    event["error"]["message"]
                        .as_str()
                        .unwrap_or("requête refusée")
                );
            }
            if let Some(id) = event["params"]["threadId"].as_str() {
                if id != thread_id {
                    continue;
                }
            }
            match event["method"].as_str() {
                Some("item/agentMessage/delta") => {
                    let id = event["params"]["itemId"].as_str().unwrap_or("");
                    if id != item_id {
                        raw.clear();
                        item_id = id.into();
                    }
                    if let Some(delta) = event["params"]["delta"].as_str() {
                        raw.push_str(delta);
                        progress.update(&raw);
                    }
                }
                Some("item/completed") if event["params"]["item"]["type"] == "agentMessage" => {
                    if let Some(text) = event["params"]["item"]["text"].as_str() {
                        if let Ok(value) = serde_json::from_str::<Value>(text) {
                            reply = Some(value);
                        }
                    }
                }
                Some("thread/tokenUsage/updated") => {
                    usage = Some(codex_usage(&event["params"]["tokenUsage"]["last"]));
                }
                Some("turn/completed") => {
                    let turn = &event["params"]["turn"];
                    ensure!(
                        turn["status"] == "completed",
                        "Codex a interrompu la réponse (authentification, quota ou réseau)"
                    );
                    break;
                }
                _ => {
                    server.reject_unexpected_request(&event).await?;
                }
            }
        }
        let mut result = decode_reply(
            reply.context("réponse structurée Codex absente")?,
            functions,
        )?;
        result.tokens = usage;
        Ok(result)
    }
}

/// Usage du dernier appel (`tokenUsage.last`) : `inputTokens` inclut la part en cache.
fn codex_usage(last: &Value) -> TokenUsage {
    TokenUsage::new(count(&last["inputTokens"]), count(&last["outputTokens"])).with_cache_tokens(
        count(&last["cachedInputTokens"]),
        count(&last["cacheWriteInputTokens"]),
    )
}

pub(super) async fn generate(
    cfg: &ChatConfig,
    functions: &[FunctionDefinition],
    messages: &[Message],
    on_text: &mut impl FnMut(&str),
) -> anyhow::Result<GenerateResult> {
    let prepared = prepare_prompt(messages, functions)?;
    let fingerprints: Vec<u64> = prepared.entries.iter().map(Entry::fingerprint).collect();
    let previous = slot().take();
    tokio::time::timeout(CLI_TIMEOUT, async {
        let (mut session, known) = match previous.and_then(|session| {
            let known = reusable_prefix(
                &session.sent,
                &session.system,
                &session.model,
                &prepared.system,
                &cfg.codex_model,
                &fingerprints,
            )
            .filter(|_| session.used.elapsed() < IDLE_LIMIT)?;
            Some((session, known))
        }) {
            Some(reused) => reused,
            // Toute session précédente non réutilisable est détruite ici.
            None => (Session::start(cfg, &prepared.system).await?, 0),
        };
        let result = session
            .turn(&prepared.entries[known..], functions, on_text)
            .await?;
        session.sent = fingerprints;
        session.used = std::time::Instant::now();
        *slot() = Some(session);
        Ok(result)
    })
    .await
    .context("Codex : délai de réponse dépassé")?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inherited_mcp_only_exports_names_not_credentials() {
        let settings = json!([{"name":"example.with.dots","transport":{"url":"http://localhost?key=private"}}]);
        assert_eq!(mcp_names(&settings).unwrap(), vec!["example.with.dots"]);
        assert!(mcp_names(&json!([{"transport":{}}])).is_err());
    }

    #[test]
    fn le_thread_est_reutilise_seulement_si_la_conversation_le_prolonge() {
        let sent = [1, 2, 3];
        // Prolongement strict : seuls les messages 4 et 5 sont envoyés.
        assert_eq!(
            reusable_prefix(&sent, "S", "", "S", "", &[1, 2, 3, 4, 5]),
            Some(3)
        );
        // Système, modèle ou historique différents : thread neuf.
        assert_eq!(
            reusable_prefix(&sent, "S", "", "S2", "", &[1, 2, 3, 4]),
            None
        );
        assert_eq!(
            reusable_prefix(&sent, "S", "", "S", "gpt", &[1, 2, 3, 4]),
            None
        );
        assert_eq!(
            reusable_prefix(&sent, "S", "", "S", "", &[1, 9, 3, 4]),
            None
        );
        // Tour annulé (historique tronqué) ou rien de nouveau : thread neuf.
        assert_eq!(reusable_prefix(&sent, "S", "", "S", "", &[1, 2]), None);
        assert_eq!(reusable_prefix(&sent, "S", "", "S", "", &[1, 2, 3]), None);
    }

    #[test]
    fn usage_codex_compte_les_jetons_en_cache() {
        let usage = codex_usage(
            &json!({"inputTokens":24170,"cachedInputTokens":24064,"cacheWriteInputTokens":0,"outputTokens":5}),
        );
        assert_eq!(
            (
                usage.prompt_tokens,
                usage.cache_read_tokens,
                usage.completion_tokens
            ),
            (24170, 24064, 5)
        );
    }
}
