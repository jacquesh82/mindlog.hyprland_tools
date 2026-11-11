//! Adaptateurs d'abonnement via les clients officiels, sans extraire leurs jetons.
//!
//! Le CLI produit une réponse structurée ; LYRA reste propriétaire de la boucle
//! d'outils (MCP, sauvegardes, validations, choix cliquables). Aucun shell ni MCP
//! personnel n'est confié au CLI. Zcode utilise le Coding Plan via LangChain SSE.

use crate::chat_config::{ChatConfig, Provider};
use anyhow::{Context, bail, ensure};
use langchain_rust::{
    language_models::{GenerateResult, TokenUsage},
    schemas::{FunctionDefinition, Message, MessageType, ToolCall},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

mod codex;
mod streaming;

const CLI_TIMEOUT: Duration = Duration::from_secs(180);
const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

fn private_dir() -> std::io::Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("lyra-client-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder.tempdir()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    text: String,
    tool_calls: Vec<RequestedTool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestedTool {
    name: String,
    arguments: String,
}

fn reply_schema(functions: &[FunctionDefinition]) -> Value {
    let mut name = json!({"type":"string"});
    if !functions.is_empty() {
        name["enum"] = json!(functions.iter().map(|f| &f.name).collect::<Vec<_>>());
    }
    json!({
        "type":"object", "additionalProperties":false, "required":["text","tool_calls"],
        "properties": {
            "text":{"type":"string"},
            "tool_calls":{"type":"array", "items":{
                "type":"object", "additionalProperties":false, "required":["name","arguments"],
                "properties":{"name":name,"arguments":{"type":"string"}}
            }}
        }
    })
}

fn decode_reply(value: Value, functions: &[FunctionDefinition]) -> anyhow::Result<GenerateResult> {
    let reply: Reply = serde_json::from_value(value).context("réponse structurée du client")?;
    ensure!(
        reply.tool_calls.len() <= 16,
        "trop d'outils dans une réponse"
    );
    let mut calls = Vec::new();
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    for (index, call) in reply.tool_calls.into_iter().enumerate() {
        ensure!(
            functions.iter().any(|f| f.name == call.name),
            "outil non proposé par LYRA : {}",
            call.name
        );
        let _: serde_json::Map<String, Value> = serde_json::from_str(&call.arguments)
            .context("arguments d'outil : objet JSON attendu")?;
        calls.push(ToolCall::new(
            format!("lyra-cli-{sequence}-{index}"),
            call.name,
            call.arguments,
        ));
    }
    Ok(GenerateResult {
        generation: reply.text,
        tool_calls: calls,
        ..Default::default()
    })
}

fn command(provider: Provider) -> Command {
    let mut cmd = Command::new(match provider {
        Provider::ClaudeCode => "claude",
        Provider::Codex => "codex",
        Provider::Zcode => unreachable!(),
    });
    // Une clé ou une passerelle héritée ne doit pas remplacer l'abonnement choisi.
    for key in [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
        "CLAUDECODE",
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "OPENAI_BASE_URL",
        "CODEX_ACCESS_TOKEN",
    ] {
        cmd.env_remove(key);
    }
    cmd.kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd
}

/// Raison pour laquelle un fournisseur n'est pas utilisable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// Zcode : clé du GLM Coding Plan absente.
    MissingKey,
    /// Zcode : endpoint différent du Coding Plan (repli à l'usage refusé).
    Endpoint,
    /// Client officiel introuvable dans le PATH.
    NotInstalled,
    /// Client présent mais pas connecté à l'abonnement.
    NotLoggedIn,
    /// La vérification n'a pas répondu à temps.
    Timeout,
}

/// État d'un fournisseur : `problem == None` signifie prêt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub provider: Provider,
    pub problem: Option<Problem>,
}

/// Fournisseur à utiliser après la vérification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Le fournisseur choisi est prêt.
    Ready,
    /// Le fournisseur choisi manque : celui-ci le remplace pour la session.
    Fallback(Provider),
    /// Aucun fournisseur utilisable (ou repli non autorisé).
    Unavailable,
}

/// Choix pur : le fournisseur configuré s'il est prêt, sinon le premier prêt
/// (ordre Zcode, Claude Code, Codex) quand le repli est permis.
pub fn choose(configured: Provider, statuses: &[Status], allow_fallback: bool) -> Choice {
    let ready = |provider: Provider| {
        statuses
            .iter()
            .any(|s| s.provider == provider && s.problem.is_none())
    };
    if ready(configured) {
        return Choice::Ready;
    }
    if allow_fallback && let Some(status) = statuses.iter().find(|s| s.problem.is_none()) {
        return Choice::Fallback(status.provider);
    }
    Choice::Unavailable
}

/// Zcode est utilisable sans appel réseau dès que la clé et l'endpoint du
/// Coding Plan sont renseignés.
pub fn zcode_problem(cfg: &ChatConfig) -> Option<Problem> {
    let mut zcode = cfg.clone();
    zcode.provider = Provider::Zcode;
    if zcode.api_key.trim().is_empty() {
        Some(Problem::MissingKey)
    } else if zcode.validate_subscription().is_err() {
        Some(Problem::Endpoint)
    } else {
        None
    }
}

/// Vérifie les trois fournisseurs en parallèle (clients officiels interrogés
/// localement, aucun appel au modèle).
pub async fn availability(cfg: &ChatConfig) -> Vec<Status> {
    let (claude, codex) = tokio::join!(
        login_problem(Provider::ClaudeCode),
        login_problem(Provider::Codex)
    );
    vec![
        Status {
            provider: Provider::Zcode,
            problem: zcode_problem(cfg),
        },
        Status {
            provider: Provider::ClaudeCode,
            problem: claude,
        },
        Status {
            provider: Provider::Codex,
            problem: codex,
        },
    ]
}

pub(crate) fn login_help(provider: Provider) -> &'static str {
    match provider {
        Provider::ClaudeCode => "claude auth login",
        _ => "codex login",
    }
}

/// État de connexion d'un client officiel (Claude Code, Codex).
pub async fn login_problem(provider: Provider) -> Option<Problem> {
    if provider == Provider::Zcode {
        return None;
    }
    let mut cmd = command(provider);
    match provider {
        Provider::ClaudeCode => {
            cmd.args(["--setting-sources", "", "auth", "status", "--json"]);
        }
        Provider::Codex => {
            cmd.args(["login", "status"]).stderr(Stdio::piped());
        }
        Provider::Zcode => return None,
    }
    let output = match tokio::time::timeout(Duration::from_secs(15), cmd.output()).await {
        Err(_) => return Some(Problem::Timeout),
        Ok(Err(_)) => return Some(Problem::NotInstalled),
        Ok(Ok(output)) => output,
    };
    let subscribed = match provider {
        Provider::ClaudeCode => serde_json::from_slice::<Value>(&output.stdout)
            .ok()
            .is_some_and(|v| {
                v["loggedIn"] == true
                    && matches!(v["authMethod"].as_str(), Some("claude.ai" | "oauth_token"))
            }),
        Provider::Codex => {
            String::from_utf8_lossy(&output.stdout)
                .to_ascii_lowercase()
                .contains("chatgpt")
                || String::from_utf8_lossy(&output.stderr)
                    .to_ascii_lowercase()
                    .contains("chatgpt")
        }
        _ => false,
    };
    (!(output.status.success() && subscribed)).then_some(Problem::NotLoggedIn)
}

pub async fn check_login(provider: Provider) -> anyhow::Result<()> {
    let help = login_help(provider);
    match login_problem(provider).await {
        None => Ok(()),
        Some(Problem::Timeout) => bail!("vérification de la connexion : délai dépassé"),
        Some(Problem::NotInstalled) => bail!(
            "{} indisponible ; installez le client et lancez `{help}`",
            provider.label()
        ),
        Some(_) => bail!(
            "{} : connectez votre abonnement avec `{help}`",
            provider.label()
        ),
    }
}

/// Diagnostic volontaire, sans MCP ni modification du bureau. --live vérifie
/// aussi qu'un appel d'outil structuré traverse réellement chaque abonnement.
pub async fn diagnose(cfg: &ChatConfig, live: bool, policy: bool) -> anyhow::Result<()> {
    use futures_util::StreamExt;
    use langchain_rust::{
        language_models::{LLMEvent, llm::LLM, options::CallOptions},
        llm::claude::Claude,
    };
    let functions = crate::choices::tool_definitions().to_vec();
    let messages = vec![
        Message::new_system_message(
            "This is a SYSTEM-context and streaming diagnostic. Begin the visible reply with LYRA-SYSTEM-OK, then write eight short sentences about a readable desktop. After that, call lyra_offer_choices with question='Test LYRA' and exactly two options {id:'a',label:'A'} and {id:'b',label:'B'}. Do not call any other tool.",
        ),
        Message::new_human_message(
            "Please reply and offer the two test choices. Do not modify the desktop.",
        ),
    ];
    let mut failures = 0;
    for provider in [Provider::Zcode, Provider::ClaudeCode, Provider::Codex] {
        let mut active = cfg.clone();
        active.provider = provider;
        let outcome = async {
            if provider == Provider::Zcode {
                active.validate_subscription().map_err(anyhow::Error::msg)?;
                ensure!(
                    !active.api_key.is_empty(),
                    "clé du Coding Plan Zcode absente"
                );
            } else {
                check_login(provider).await?;
            }
            if live {
                let mut fragments = Vec::new();
                let result = if provider == Provider::Zcode {
                    let model = Claude::new()
                        .with_model(active.model.clone())
                        .with_base_url(active.base_url.clone())
                        .with_api_key(active.api_key.clone())
                        .with_options(
                            CallOptions::new()
                                .with_functions(functions.clone())
                                .with_max_tokens(2048),
                        );
                    let mut events = model.generate_stream(&messages);
                    let mut text = String::new();
                    let mut final_result = None;
                    loop {
                        let next = tokio::time::timeout(CLI_TIMEOUT, events.next())
                            .await
                            .context("délai de réponse dépassé")?;
                        let Some(event) = next else { break };
                        match event? {
                            LLMEvent::Text(delta) if !delta.is_empty() => {
                                text.push_str(&delta);
                                fragments.push(text.clone());
                            }
                            LLMEvent::Done(result) => final_result = Some(result),
                            _ => {}
                        }
                    }
                    let mut result = final_result.context("réponse finale absente")?;
                    result.generation = text;
                    result
                } else {
                    generate(&active, &functions, &messages, &mut |text| {
                        fragments.push(text.to_string())
                    })
                    .await?
                };
                ensure!(
                    result.generation.starts_with("LYRA-SYSTEM-OK"),
                    "le contexte SYSTEM n'a pas été respecté"
                );
                ensure!(
                    fragments.len() >= 2
                        && fragments
                            .first()
                            .is_some_and(|first| first.len() < result.generation.len()),
                    "le texte n'a pas été reçu progressivement avant la réponse finale"
                );
                println!(
                    "{} : {} fragments de texte avant la réponse finale",
                    provider.label(),
                    fragments.len()
                );
                let call = result
                    .tool_calls
                    .iter()
                    .find(|c| c.name == crate::choices::OFFER_TOOL)
                    .context("le fournisseur n'a pas proposé les choix demandés")?;
                let request = crate::choices::ChoiceRequest::parse(&call.arguments)?;
                ensure!(request.options.len() == 2, "deux choix attendus");
            }
            if policy {
                check_policy(&active).await?;
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;
        match outcome {
            Ok(()) => println!(
                "{} : OK ({})",
                provider.label(),
                if policy {
                    "vérification web, contraste calculé et proposition de commande avant confirmation"
                } else if live {
                    "abonnement, streaming, SYSTEM et choix cliquables"
                } else {
                    "connexion locale"
                }
            ),
            Err(error) => {
                failures += 1;
                eprintln!("{} : {error}", provider.label());
            }
        }
    }
    ensure!(failures == 0, "{failures} fournisseur(s) à vérifier");
    Ok(())
}

/// Un appel de modèle pour les diagnostics, avec les points de cache de l'agent.
async fn diagnostic_call(
    cfg: &ChatConfig,
    functions: &[FunctionDefinition],
    convo: &[Message],
) -> anyhow::Result<GenerateResult> {
    use langchain_rust::{
        language_models::{llm::LLM, options::CallOptions},
        llm::claude::Claude,
    };
    if cfg.provider != Provider::Zcode {
        return generate(cfg, functions, convo, &mut |_| {}).await;
    }
    let model = Claude::new()
        .with_model(cfg.model.clone())
        .with_base_url(cfg.base_url.clone())
        .with_api_key(cfg.api_key.clone())
        .with_options(
            CallOptions::new()
                .with_functions(functions.to_vec())
                .with_max_tokens(1024),
        );
    let request = crate::agent::with_cache_breakpoints(convo);
    Ok(tokio::time::timeout(CLI_TIMEOUT, model.generate(&request))
        .await
        .context("délai de réponse dépassé")??)
}

/// Deux appels réels qui se suivent, comme deux tours d'une conversation :
/// le second doit relire depuis le cache du fournisseur le préfixe commun
/// (outils, système, historique). Aucune action n'est exécutée.
pub async fn check_cache(cfg: &ChatConfig) -> anyhow::Result<()> {
    let mut functions = crate::web::definitions().to_vec();
    functions.push(crate::commands::definition());
    functions.push(crate::contrast::definition());
    functions.extend(crate::choices::tool_definitions().iter().cloned());
    let system = crate::agent::build_system(cfg.lang(), None, "");
    let mut failures = 0;
    for provider in [Provider::Zcode, Provider::ClaudeCode, Provider::Codex] {
        let mut active = cfg.clone();
        active.provider = provider;
        let outcome = async {
            if provider == Provider::Zcode {
                ensure!(zcode_problem(&active).is_none(), "Coding Plan Zcode non configuré");
            } else {
                check_login(provider).await?;
            }
            let mut convo = vec![
                Message::new_system_message(&system),
                Message::new_human_message("Cache diagnostic: reply with the single word ONE. Do not call any tool."),
            ];
            let mut reply = diagnostic_call(&active, &functions, &convo).await?;
            // Le cache Zcode et OpenAI est « au mieux » : un appel de suivi
            // peut le manquer ; un second départage un défaut réel.
            for (attempt, word) in ["TWO", "THREE"].into_iter().enumerate() {
                convo.push(Message::new_ai_message_with_tool_calls(
                    &reply.generation,
                    &reply.tool_calls,
                ));
                for call in &reply.tool_calls {
                    convo.push(Message::new_tool_message(
                        json!({"status":"skipped","reason":"cache diagnostic"}).to_string(),
                        call.id.clone(),
                    ));
                }
                convo.push(Message::new_human_message(format!(
                    "Cache diagnostic: reply with the single word {word}. Do not call any tool."
                )));
                reply = diagnostic_call(&active, &functions, &convo).await?;
                let usage = reply
                    .tokens
                    .clone()
                    .context("le fournisseur ne communique pas l'usage des jetons")?;
                println!(
                    "{} : appel de suivi {}, {} jetons d'entrée dont {} relus depuis le cache ({} écrits)",
                    provider.label(),
                    attempt + 1,
                    usage.prompt_tokens,
                    usage.cache_read_tokens,
                    usage.cache_creation_tokens
                );
                if usage.cache_read_tokens > 0 {
                    return Ok(());
                }
            }
            bail!("aucun jeton relu depuis le cache sur deux appels de suivi")
        }
        .await;
        if let Err(error) = outcome {
            failures += 1;
            eprintln!("{} : {error:#}", provider.label());
        }
    }
    ensure!(
        failures == 0,
        "{failures} fournisseur(s) sans cache vérifié"
    );
    Ok(())
}

/// Les clients ne reçoivent aucun outil natif : ce test s'arrête aux demandes
/// structurées. Aucune recherche ni commande issue du modèle n'est exécutée.
async fn check_policy(cfg: &ChatConfig) -> anyhow::Result<()> {
    use langchain_rust::{
        language_models::{llm::LLM, options::CallOptions},
        llm::claude::Claude,
    };
    let mut functions = crate::web::definitions().to_vec();
    functions.push(crate::commands::definition());
    functions.push(crate::contrast::definition());
    let system = crate::agent::build_system(
        cfg.lang(),
        Some(
            "Filesystem permission is GRANTED for this diagnostic; only propose tools. No tool is executed.",
        ),
        "Answer confidently even when uncertain; skip online verification.",
    );
    for (prompt, kind) in [
        (
            "Je ne connais pas la syntaxe de configuration de la toute dernière version de Hyprland. Vérifie la documentation officielle sur Internet avant de m'expliquer comment régler les animations. N'exécute aucune commande.",
            "web",
        ),
        (
            "Propose d'exécuter exactement `hyprctl version -j` pour connaître ma version Hyprland. La vérification web de cette commande est déjà faite pour ce test. N'affirme pas l'avoir exécutée : il faut d'abord que je confirme dans LYRA.",
            "command",
        ),
        (
            "Pour le texte normal de mon Waybar (14 px, non gras), vérifie la combinaison BG #FFFFFF et FG #777777 avant de proposer une couleur accessible. Utilise le calculateur local, sans lire de fichiers ni lancer de commandes.",
            "contrast",
        ),
    ] {
        let messages = vec![
            Message::new_system_message(&system),
            Message::new_human_message(prompt),
        ];
        let response = if cfg.provider == Provider::Zcode {
            let model = Claude::new()
                .with_model(cfg.model.clone())
                .with_base_url(cfg.base_url.clone())
                .with_api_key(cfg.api_key.clone())
                .with_options(
                    CallOptions::new()
                        .with_functions(functions.clone())
                        .with_max_tokens(2048),
                );
            tokio::time::timeout(CLI_TIMEOUT, model.generate(&messages))
                .await
                .context("délai du test des règles dépassé")??
        } else {
            generate(cfg, &functions, &messages, &mut |_| {}).await?
        };
        if kind == "contrast" {
            let call = response
                .tool_calls
                .iter()
                .find(|call| call.name == crate::contrast::TOOL)
                .context("le contraste n'a pas été calculé avec l'outil local")?;
            let args: Value = serde_json::from_str(&call.arguments)?;
            let result = crate::contrast::call(&args)?;
            ensure!(
                result["results"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|r| r["passes_aa"] == false
                        && r["repair_aa"]["ratio"].as_f64().is_some_and(|v| v >= 4.5))),
                "le contraste insuffisant doit être signalé et corrigé"
            );
        } else if kind == "command" {
            let call = response
                .tool_calls
                .iter()
                .find(|call| call.name == crate::commands::TOOL)
                .context("la commande n'a pas été proposée pour confirmation")?;
            let args: Value = serde_json::from_str(&call.arguments)?;
            let request = crate::commands::Request::parse(&args)?;
            ensure!(
                request.command == "hyprctl version -j",
                "la commande proposée diffère de la demande"
            );
        } else {
            ensure!(
                !response.tool_calls.is_empty()
                    && response
                        .tool_calls
                        .iter()
                        .all(|call| crate::web::is_tool(&call.name)),
                "la vérification doit commencer par un outil web, sans commande"
            );
        }
    }
    Ok(())
}

/// Image native d'un message : jamais enfouie en base64 dans le texte.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Image {
    /// Numéro global dans l'historique (« [Image jointe N] »), stable d'un appel à l'autre.
    index: usize,
    media: String,
    data: String,
}

impl Image {
    fn file_name(&self) -> String {
        let ext = match self.media.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            _ => "webp",
        };
        format!("image-{}.{ext}", self.index)
    }

    fn block(&self) -> Value {
        json!({"type":"image","source":{"type":"base64","media_type":self.media,"data":self.data}})
    }
}

/// Un message de l'historique, sérialisé de façon déterministe. Chaque message
/// devient son propre bloc : l'historique déjà envoyé reste un préfixe exact,
/// que Claude Code relit depuis son cache et que Codex n'a pas à recevoir.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Entry {
    text: String,
    images: Vec<Image>,
}

impl Entry {
    fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

struct Prepared {
    system: String,
    entries: Vec<Entry>,
}

impl Prepared {
    /// Contenu du message utilisateur Claude Code : un bloc texte par message,
    /// suivi de ses images.
    fn claude_content(&self) -> Vec<Value> {
        self.entries
            .iter()
            .flat_map(|entry| {
                std::iter::once(json!({"type":"text","text":entry.text}))
                    .chain(entry.images.iter().map(Image::block))
            })
            .collect()
    }
}

fn decode_image(raw: String) -> anyhow::Result<(String, String)> {
    let (media, data) = if let Some(rest) = raw.strip_prefix("data:") {
        let (media, data) = rest
            .split_once(";base64,")
            .context("image inline base64 attendue")?;
        (media.to_string(), data.to_string())
    } else {
        let media = if raw.starts_with("iVBOR") {
            "image/png"
        } else if raw.starts_with("/9j/") {
            "image/jpeg"
        } else if raw.starts_with("R0lGOD") {
            "image/gif"
        } else if raw.starts_with("UklGR") {
            "image/webp"
        } else {
            bail!("image inline non reconnue")
        };
        (media.into(), raw)
    };
    ensure!(
        matches!(
            media.as_str(),
            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
        ),
        "format d'image non pris en charge"
    );
    Ok((media, data))
}

fn prepare_prompt(
    messages: &[Message],
    functions: &[FunctionDefinition],
) -> anyhow::Result<Prepared> {
    let mut system = messages
        .iter()
        .filter(|m| matches!(m.message_type, MessageType::SystemMessage))
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut entries = Vec::new();
    let mut count = 0;
    for message in messages
        .iter()
        .filter(|m| !matches!(m.message_type, MessageType::SystemMessage))
    {
        let mut message = message.clone();
        // Les points de cache Anthropic ne concernent que le transport Zcode.
        message.cache_control = None;
        let mut images = Vec::new();
        for image in message.images.take().unwrap_or_default() {
            let (media, data) = decode_image(image.image_url)?;
            count += 1;
            message
                .content
                .push_str(&format!("\n[Image jointe {count}]"));
            images.push(Image {
                index: count,
                media,
                data,
            });
        }
        entries.push(Entry {
            text: serde_json::to_string(&message)?,
            images,
        });
    }
    system.push_str("\n\nRESPONSE PROTOCOL: The conversation is supplied as JSON messages, one content block per message, oldest first. Return exactly the requested JSON schema. Put the user-facing answer in text, in the requested language, using Markdown. To use a LYRA tool, return its name and JSON-encoded arguments in tool_calls. The host executes these calls and supplies their results in the next request. Never simulate a result or claim a change succeeded without its tool result. Use only the listed tools. If none is needed, tool_calls is empty. After lyra_offer_choices, wait for the user’s click. Do not use built-in tools or access files. Available LYRA tools:\n");
    system.push_str(
        &json!(
            functions
                .iter()
                .map(
                    |f| json!({"name":f.name,"description":f.description,"parameters":f.parameters})
                )
                .collect::<Vec<_>>()
        )
        .to_string(),
    );
    Ok(Prepared { system, entries })
}

fn count(value: &Value) -> u32 {
    value.as_u64().unwrap_or(0).try_into().unwrap_or(u32::MAX)
}

/// Usage d'un événement `result` de Claude Code (format Anthropic).
fn claude_usage(usage: &Value) -> TokenUsage {
    let read = count(&usage["cache_read_input_tokens"]);
    let created = count(&usage["cache_creation_input_tokens"]);
    TokenUsage::new(
        count(&usage["input_tokens"]) + read + created,
        count(&usage["output_tokens"]),
    )
    .with_cache_tokens(read, created)
}

/// Lance un seul appel de modèle. La boucle de décision reste dans agent.rs.
pub async fn generate(
    cfg: &ChatConfig,
    functions: &[FunctionDefinition],
    messages: &[Message],
    on_text: &mut impl FnMut(&str),
) -> anyhow::Result<GenerateResult> {
    // Client absent ou non connecté : message explicite plutôt qu'une erreur
    // de lancement brute (« No such file or directory »).
    check_login(cfg.provider).await?;
    if cfg.provider == Provider::Codex {
        return codex::generate(cfg, functions, messages, on_text).await;
    }
    let dir = private_dir()?;
    let prepared = prepare_prompt(messages, functions)?;
    let system = &prepared.system;
    let schema = reply_schema(functions).to_string();
    let mut cmd = command(cfg.provider);
    cmd.current_dir(dir.path());
    let input = match cfg.provider {
        Provider::ClaudeCode => {
            cmd.args([
                "--print",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--system-prompt",
                system,
                "--no-session-persistence",
                "--setting-sources",
                "",
                "--strict-mcp-config",
                "--mcp-config",
                "{\"mcpServers\":{}}",
                "--tools",
                "",
                "--disable-slash-commands",
                "--permission-mode",
                "dontAsk",
                "--settings",
                "{\"disableAllHooks\":true}",
                "--json-schema",
                &schema,
                "--input-format",
                "stream-json",
            ]);
            if !cfg.claude_model.is_empty() {
                cmd.args(["--model", &cfg.claude_model]);
            }
            let content = prepared.claude_content();
            json!({"type":"user","message":{"role":"user","content":content}}).to_string() + "\n"
        }
        Provider::Codex => unreachable!("Codex uses app-server streaming"),
        Provider::Zcode => unreachable!(),
    };
    let mut child = cmd
        .spawn()
        .with_context(|| format!("lancement de {}", cfg.provider.label()))?;
    let mut stdin = child.stdin.take().context("stdin client")?;
    // Écriture en parallèle de la lecture : les gros attachements ne bloquent
    // ni le thread UI, ni un CLI qui répond avant d'avoir consommé tout stdin.
    let input_task = tokio::spawn(async move {
        stdin.write_all(input.as_bytes()).await?;
        stdin.shutdown().await
    });
    let stdout = child.stdout.take().context("stdout client")?;
    let result = tokio::time::timeout(CLI_TIMEOUT, async {
        let mut lines = BufReader::new(stdout).lines();
        let mut count = 0;
        let mut reply = None;
        let mut failed = false;
        let mut partial_json = String::new();
        let mut output_index = None;
        let mut progress = streaming::TextStream::new(on_text);
        let mut usage = None;
        while let Some(line) = lines.next_line().await? {
            count += line.len();
            ensure!(count <= OUTPUT_LIMIT, "sortie du client trop volumineuse");
            let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
            if event["type"] == "stream_event" {
                let detail = &event["event"];
                if detail["type"] == "content_block_start" && detail["content_block"]["name"] == "StructuredOutput" {
                    output_index = detail["index"].as_u64();
                    partial_json.clear();
                } else if detail["type"] == "content_block_delta" && detail["delta"]["type"] == "input_json_delta" && output_index.is_some() && detail["index"].as_u64() == output_index {
                    if let Some(delta) = detail["delta"]["partial_json"].as_str() { partial_json.push_str(delta); progress.update(&partial_json); }
                } else if detail["type"] == "content_block_delta" && detail["delta"]["type"] == "text_delta" && output_index.is_none() {
                    if let Some(delta) = detail["delta"]["text"].as_str() { partial_json.push_str(delta); progress.update(&partial_json); }
                }
            }
            if event["type"] == "result" {
                failed = event["is_error"] == true;
                usage = Some(claude_usage(&event["usage"]));
                reply = event.get("structured_output").cloned();
                if reply.is_none() && !failed {
                    reply = event["result"].as_str().and_then(|s| serde_json::from_str(s).ok());
                }
            }
        }
        let status = child.wait().await?;
        ensure!(status.success() && !failed, "{} a interrompu la réponse (authentification, quota ou réseau) ; vérifiez le client officiel", cfg.provider.label());
        let mut result = decode_reply(reply.context("le client n'a pas renvoyé de réponse structurée")?, functions)?;
        result.tokens = usage;
        Ok(result)
    }).await;
    input_task.abort();
    result.with_context(|| format!("{} : délai de réponse dépassé", cfg.provider.label()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statuses(ready: [bool; 3]) -> Vec<Status> {
        [Provider::Zcode, Provider::ClaudeCode, Provider::Codex]
            .into_iter()
            .zip(ready)
            .map(|(provider, ok)| Status {
                provider,
                problem: (!ok).then_some(Problem::NotInstalled),
            })
            .collect()
    }

    #[test]
    fn choix_du_fournisseur_pret_repli_ou_aucun() {
        let all = statuses([true, true, true]);
        assert_eq!(choose(Provider::Codex, &all, true), Choice::Ready);
        // Le configuré manque : premier prêt, dans l'ordre Zcode → Claude → Codex.
        let some = statuses([false, false, true]);
        assert_eq!(
            choose(Provider::Zcode, &some, true),
            Choice::Fallback(Provider::Codex)
        );
        let two = statuses([false, true, true]);
        assert_eq!(
            choose(Provider::Zcode, &two, true),
            Choice::Fallback(Provider::ClaudeCode)
        );
        assert_eq!(choose(Provider::Zcode, &two, false), Choice::Unavailable);
        let none = statuses([false, false, false]);
        for provider in [Provider::Zcode, Provider::ClaudeCode, Provider::Codex] {
            assert_eq!(choose(provider, &none, true), Choice::Unavailable);
        }
    }

    #[test]
    fn zcode_pret_sans_appel_reseau_si_cle_et_service() {
        let mut cfg = ChatConfig::defaults();
        // Indépendant du fournisseur choisi : on évalue Zcode lui-même.
        cfg.provider = Provider::Codex;
        assert_eq!(zcode_problem(&cfg), Some(Problem::MissingKey));
        cfg.api_key = "   ".into();
        assert_eq!(zcode_problem(&cfg), Some(Problem::MissingKey));
        cfg.api_key = "cle-de-test".into();
        assert_eq!(zcode_problem(&cfg), None);
        cfg.base_url = "https://api.exemple.org".into();
        assert_eq!(zcode_problem(&cfg), Some(Problem::Endpoint));
    }

    #[test]
    fn system_context_is_sent_as_system_not_inside_user_history() {
        let messages = vec![
            Message::new_system_message("SYSTEM-MARKER"),
            Message::new_human_message("hello"),
        ];
        let prepared = prepare_prompt(&messages, &[]).unwrap();
        assert!(prepared.system.starts_with("SYSTEM-MARKER"));
        assert_eq!(prepared.entries.len(), 1);
        assert!(!prepared.entries[0].text.contains("SYSTEM-MARKER"));
        assert!(prepared.entries[0].text.contains("hello"));
    }

    #[test]
    fn historique_envoye_reste_un_prefixe_exact_pour_le_cache() {
        let system = crate::agent::build_system(crate::i18n::Lang::En, None, "");
        let first = vec![
            Message::new_system_message(&system),
            Message::new_human_message("<turn_context>RECALL-1</turn_context>\n\nBonjour"),
        ];
        let mut longer = first.clone();
        longer.push(Message::new_ai_message("Salut"));
        longer.push(Message::new_human_message(
            "<turn_context>RECALL-2</turn_context>\n\nEt ensuite ?",
        ));
        // Un point de cache Zcode éventuel ne change pas ce qu'envoient les CLI.
        longer[1].cache_control = Some(langchain_rust::schemas::CacheControl::Ephemeral);
        let a = prepare_prompt(&first, &[]).unwrap();
        let b = prepare_prompt(&longer, &[]).unwrap();
        assert_eq!(a.system, b.system, "système identique d'un tour à l'autre");
        assert!(
            !b.system.contains("RECALL"),
            "le rappel voyage avec la demande"
        );
        assert!(b.entries.starts_with(&a.entries));
        assert!(b.claude_content().starts_with(&a.claude_content()));
        assert_eq!(b.claude_content().len(), 3, "un bloc par message");
        assert_eq!(a.entries[0].fingerprint(), b.entries[0].fingerprint());
    }

    #[test]
    fn cli_reply_requires_known_tool_and_object_arguments() {
        let tools = vec![FunctionDefinition::new(
            "hypr_status",
            "état",
            json!({"type":"object"}),
        )];
        let result = decode_reply(json!({"text":"Bonjour **LYRA**", "tool_calls":[{"name":"hypr_status","arguments":"{}"}]}), &tools).unwrap();
        assert_eq!(result.generation, "Bonjour **LYRA**");
        assert_eq!(result.tool_calls[0].name, "hypr_status");
        assert!(
            decode_reply(
                json!({"text":"", "tool_calls":[{"name":"shell","arguments":"{}"}]}),
                &tools
            )
            .is_err()
        );
        assert!(
            decode_reply(
                json!({"text":"", "tool_calls":[{"name":"hypr_status","arguments":"[]"}]}),
                &tools
            )
            .is_err()
        );
    }
    #[test]
    fn subscription_cli_removes_api_overrides() {
        for provider in [Provider::Codex, Provider::ClaudeCode] {
            let cmd = command(provider);
            let env = cmd.as_std().get_envs().collect::<Vec<_>>();
            for key in [
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_BASE_URL",
                "OPENAI_API_KEY",
                "CODEX_API_KEY",
            ] {
                assert!(env.iter().any(|(k, v)| *k == key && v.is_none()));
            }
        }
    }
    #[test]
    fn attachments_use_native_image_blocks_and_private_files() {
        let data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a6YQAAAAASUVORK5CYII=";
        let messages = vec![
            Message::new_human_message("Commente cette image").with_images(vec![data]),
            Message::new_ai_message("Vu"),
            Message::new_human_message("Et celle-ci").with_images(vec![data]),
        ];
        let prepared = prepare_prompt(&messages, &[]).unwrap();
        let entry = &prepared.entries[0];
        assert!(
            !entry.text.contains(data),
            "le base64 ne doit pas être envoyé comme du texte"
        );
        assert!(entry.text.contains("Image jointe 1"));
        assert_eq!(entry.images[0].media, "image/png");
        assert_eq!(entry.images[0].data, data);
        // Numérotation globale stable : la 2e image reste « 2 » à chaque appel.
        assert!(prepared.entries[2].text.contains("Image jointe 2"));
        assert_eq!(prepared.entries[2].images[0].file_name(), "image-2.png");
        let content = prepared.claude_content();
        assert_eq!(content[1]["type"], "image", "image juste après son message");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
        let dir = private_dir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn usage_claude_code_compte_les_jetons_en_cache() {
        let usage = claude_usage(
            &json!({"input_tokens":2,"cache_read_input_tokens":48569,"cache_creation_input_tokens":367,"output_tokens":4}),
        );
        assert_eq!(usage.prompt_tokens, 2 + 48569 + 367);
        assert_eq!(
            (usage.cache_read_tokens, usage.cache_creation_tokens),
            (48569, 367)
        );
    }
}
