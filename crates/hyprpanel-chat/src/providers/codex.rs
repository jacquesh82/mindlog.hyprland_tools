//! Codex app-server : session ChatGPT et vrais événements de texte incrémentaux.
//! Le provider dédié n'a ni env_key ni endpoint API facturé à l'usage.

use super::{
    CLI_TIMEOUT, OUTPUT_LIMIT, command, decode_reply, prepare_prompt, private_dir, reply_schema,
    streaming::TextStream,
};
use crate::chat_config::{ChatConfig, Provider};
use anyhow::{Context, bail, ensure};
use langchain_rust::{
    language_models::GenerateResult,
    schemas::{FunctionDefinition, Message},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout},
};

const SUBSCRIPTION_PROVIDER: &str = "lyra_chatgpt_subscription";

struct AppServer {
    child: Child,
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

pub(super) async fn generate(
    cfg: &ChatConfig,
    functions: &[FunctionDefinition],
    messages: &[Message],
    on_text: &mut impl FnMut(&str),
) -> anyhow::Result<GenerateResult> {
    let dir = private_dir()?;
    let (system, prompt, _images, paths) = prepare_prompt(messages, functions, dir.path())?;
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
        child,
        stdin,
        stdout,
        next_id: 0,
        received: 0,
    };
    let result = tokio::time::timeout(CLI_TIMEOUT, async {
        server.rpc("initialize", json!({"clientInfo":{"name":"lyra","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}})).await?;
        server.send(json!({"method":"initialized","params":{}})).await?;
        let account = server.rpc("account/read", json!({"refreshToken":false})).await?;
        ensure!(account["account"]["type"] == "chatgpt", "Codex requires a ChatGPT subscription login (`codex login`); API billing is disabled.");
        let model = if cfg.codex_model.is_empty() {
            let models = server.rpc("model/list", json!({})).await?;
            let choices = models["data"].as_array().context("catalogue Codex absent")?;
            let selected = choices.iter().find(|m| m["isDefault"] == true).or_else(|| choices.first()).context("aucun modèle Codex disponible")?;
            selected["model"].as_str().or_else(|| selected["id"].as_str()).context("modèle Codex absent")?.to_string()
        } else { cfg.codex_model.clone() };
        let thread = server.rpc("thread/start", json!({
            "model":model,"modelProvider":SUBSCRIPTION_PROVIDER,"cwd":dir.path(),
            "approvalPolicy":"never","sandbox":"read-only","ephemeral":true,
            "baseInstructions":system,"developerInstructions":"Follow LYRA’s response protocol. Return only the requested schema, with text first. All actions go through the declared LYRA tools.",
            "experimentalRawEvents":false
        })).await?;
        let thread_id = thread["thread"]["id"].as_str().context("identifiant de conversation Codex absent")?;
        let mut input = vec![json!({"type":"text","text":prompt})];
        input.extend(paths.into_iter().map(|path| json!({"type":"localImage","path":path})));
        // Les notifications de texte peuvent arriver avant la réponse turn/start :
        // envoyer sans rpc() et lire immédiatement l'ensemble du flux.
        server.next_id += 1;
        let turn_request = server.next_id;
        server.send(json!({"id":turn_request,"method":"turn/start","params":{
            "threadId":thread_id,"input":input,"outputSchema":reply_schema(functions)
        }})).await?;
        let mut progress = TextStream::new(on_text);
        let mut raw = String::new();
        let mut item_id = String::new();
        let mut reply = None;
        loop {
            let event = server.receive().await?;
            if event["id"] == turn_request && event.get("error").is_some() {
                bail!("Codex n'a pas pu démarrer la réponse : {}", event["error"]["message"].as_str().unwrap_or("requête refusée"));
            }
            if let Some(id) = event["params"]["threadId"].as_str() { if id != thread_id { continue; } }
            match event["method"].as_str() {
                Some("item/agentMessage/delta") => {
                    let id = event["params"]["itemId"].as_str().unwrap_or("");
                    if id != item_id { raw.clear(); item_id = id.into(); }
                    if let Some(delta) = event["params"]["delta"].as_str() { raw.push_str(delta); progress.update(&raw); }
                }
                Some("item/completed") if event["params"]["item"]["type"] == "agentMessage" => {
                    if let Some(text) = event["params"]["item"]["text"].as_str() {
                        if let Ok(value) = serde_json::from_str::<Value>(text) { reply = Some(value); }
                    }
                }
                Some("turn/completed") => {
                    let turn = &event["params"]["turn"];
                    ensure!(turn["status"] == "completed", "Codex a interrompu la réponse (authentification, quota ou réseau)");
                    break;
                }
                _ => { server.reject_unexpected_request(&event).await?; }
            }
        }
        decode_reply(reply.context("réponse structurée Codex absente")?, functions)
    }).await.context("Codex : délai de réponse dépassé")?;
    // App-server est persistant ; chaque appel LYRA possède son processus isolé.
    let _ = server.child.kill().await;
    result
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
}
