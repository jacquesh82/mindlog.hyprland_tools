//! Boucle d'agent LYRA sur **LangChain Rust** (module claude) :
//! streaming `LLMEvent` → bulles, tool use natif Anthropic (`FunctionDefinition`
//! + `Message::new_tool_message`), exécution des outils via le client MCP.
//!
//! La boucle d'exécution reste volontairement fine et locale (streaming vers
//! l'UI, pont MCP, timeouts, registre de tours restaurables) : les
//! `AgentExecutor` génériques n'exposent ni l'un ni l'autre.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use base64::Engine as _;
use futures_util::StreamExt;
use langchain_rust::language_models::options::CallOptions;
use langchain_rust::language_models::{llm::LLM, GenerateResult, LLMEvent};
use langchain_rust::llm::claude::Claude;
use langchain_rust::schemas::{FunctionDefinition, ImageContent, Message};
use serde_json::{json, Map, Value};
use slint::Weak;
use tokio::sync::mpsc::UnboundedReceiver;

use hyprpanel_core::timefmt;

use crate::chat_config::ChatConfig;
use crate::i18n::{tr, Lang, Tr};
use crate::mcp::McpSession;
use crate::ui;
use crate::ChatWindow;

/// Événements envoyés par l'UI vers la boucle agent.
pub enum UserEvent {
    Input { text: String, files: Vec<PathBuf> },
    Reset,
    ConfigUpdated(Box<ChatConfig>),
    Restore(String),
}

const MAX_ROUNDS: usize = 12;
const TIMEOUT_LLM: Duration = Duration::from_secs(180);
const TIMEOUT_TOOL: Duration = Duration::from_secs(30);
/// Tailles max des pièces jointes.
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 64 * 1024;

// ---------------------------------------------------------------------------
// Pièces jointes (classification pure, testée)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    Text,
    Unsupported,
}

pub fn classify_attachment(path: &Path) -> AttachmentKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" => AttachmentKind::Image,
        "txt" | "md" | "lua" | "conf" | "toml" | "json" | "yaml" | "yml" | "ini" | "rs" | "py"
        | "sh" | "bash" | "css" | "html" | "xml" | "csv" | "log" | "service" | "slint" => {
            AttachmentKind::Text
        }
        _ => AttachmentKind::Unsupported,
    }
}

/// Libellé lisible d'une pièce (chip, bulle).
pub fn attachment_meta(path: &Path) -> String {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let kind = match classify_attachment(path) {
        AttachmentKind::Image => "image",
        AttachmentKind::Text => "texte",
        AttachmentKind::Unsupported => "?",
    };
    format!("{} · {}", human_size(size), kind)
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} Mo", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.0} Ko", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} o")
    }
}

/// Construit le message utilisateur (texte + pièces jointes). Les images
/// partent en base64 brut — LangChain renifle le media type ; les fichiers
/// texte sont inlinés. `read` injecté pour les tests.
fn build_user_message(
    text: &str,
    files: &[PathBuf],
    read: &dyn Fn(&Path) -> std::io::Result<Vec<u8>>,
) -> (Message, Vec<String>) {
    let mut content = text.trim().to_string();
    let mut images: Vec<ImageContent> = Vec::new();
    let mut notes = Vec::new();

    for file in files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.to_string_lossy().into_owned());
        match classify_attachment(file) {
            AttachmentKind::Image => match read(file) {
                Ok(bytes) if bytes.len() as u64 <= MAX_IMAGE_BYTES => {
                    images.push(ImageContent::from(
                        base64::engine::general_purpose::STANDARD.encode(&bytes),
                    ));
                }
                Ok(_) => notes.push(format!("📎 {name} : image trop volumineuse (> 5 Mo), non transmise")),
                Err(e) => notes.push(format!("📎 {name} : illisible ({e})")),
            },
            AttachmentKind::Text => match read(file) {
                Ok(bytes) if bytes.len() as u64 <= MAX_TEXT_BYTES => {
                    let inner = String::from_utf8_lossy(&bytes);
                    content.push_str(&format!("\n\n— fichier {name} —\n{inner}\n— fin {name} —"));
                }
                Ok(_) => notes.push(format!("📎 {name} : fichier texte trop volumineux (> 64 Ko), non transmis")),
                Err(e) => notes.push(format!("📎 {name} : illisible ({e})")),
            },
            AttachmentKind::Unsupported => {
                notes.push(format!("📎 {name} : type non pris en charge (images et fichiers texte uniquement)"));
            }
        }
    }

    if content.is_empty() {
        // Anthropic exige au moins un bloc de contenu lisible.
        content = if images.is_empty() {
            "(pièce jointe illisible)".to_string()
        } else {
            "(image)".to_string()
        };
    }
    (Message::new_human_message(content).with_images(images), notes)
}

// ---------------------------------------------------------------------------
// Registre des tours (historique restaurable)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TurnRecord {
    pub turn: u32,
    pub timestamp: String,
    /// Extrait du prompt (≈ 60 caractères).
    pub preview: String,
    /// Nom du backup « avant ce tour », si le daemon était joignable.
    pub backup: Option<String>,
}

struct Ledger {
    records: Vec<TurnRecord>,
    path: PathBuf,
}

impl Ledger {
    fn load(path: PathBuf) -> Self {
        let records = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { records, path }
    }

    fn next_turn(&self) -> u32 {
        self.records.iter().map(|r| r.turn).max().unwrap_or(0) + 1
    }

    fn push(&mut self, record: TurnRecord) {
        self.records.push(record);
        // Un échec d'écriture ne doit pas casser la conversation.
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.records) {
            let _ = std::fs::write(&self.path, json);
        }
    }
}

/// Extrait le nom du backup de la réponse de l'outil (« backup créé : X »).
fn parse_backup_name(response: &str) -> Option<String> {
    response
        .lines()
        .find_map(|l| l.split_once("backup créé :"))
        .map(|(_, name)| name.trim().to_string())
        .filter(|n| !n.is_empty() && !n.contains(' '))
}

// ---------------------------------------------------------------------------
// Boucle principale
// ---------------------------------------------------------------------------

pub async fn run(cfg: ChatConfig, mut rx: UnboundedReceiver<UserEvent>, weak: Weak<ChatWindow>) {
    let mut cfg = cfg;
    let mut translations = tr(cfg.lang());

    if cfg.api_key.trim().is_empty() {
        ui::append(&weak, "error", "Clé API absente : remplissez « clé API » dans ⚙ réglages.\n/api key missing: fill it in the ⚙ settings panel.");
    }

    // Connexion MCP + définitions d'outils pour le LLM.
    let mcp = match cfg.mcp_url() {
        Ok(url) => match McpSession::connect(&url).await {
            Ok(session) => {
                let n = session.tools().len();
                ui::set_status(&weak, format!("MCP : {n} outils"));
                Some(session)
            }
            Err(e) => {
                ui::append(&weak, "error", format!("{} : {e:#}", translations.mcp_unavailable));
                ui::set_status(&weak, "MCP ✗");
                None
            }
        },
        Err(e) => {
            ui::append(&weak, "error", format!("Config du daemon introuvable : {e} (lancez `hyprpanel init`)"));
            ui::set_status(&weak, "daemon ✗");
            None
        }
    };
    let functions: Vec<FunctionDefinition> = mcp
        .as_ref()
        .map(|m| {
            m.tools()
                .iter()
                .map(|t| {
                    FunctionDefinition::new(
                        &t.name,
                        t.description.as_deref().unwrap_or(""),
                        Value::Object(t.input_schema.as_ref().clone()),
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    // Clients externes : bulle d'information si Claude Code / ZCode manquent.
    notify_missing_clients(&weak, &cfg);

    let mut llm = build_llm(&cfg, &functions);
    let mut system = build_system(cfg.lang(), mcp.as_ref().and_then(|m| m.instructions()).as_deref());
    let mut convo: Vec<Message> = vec![Message::new_system_message(system.clone())];

    let mut ledger = Ledger::load(crate::chat_config::state_history_path());
    ui::history_set(&weak, &ledger.records);

    while let Some(event) = rx.recv().await {
        match event {
            UserEvent::Reset => {
                convo = vec![Message::new_system_message(system.clone())];
                ui::clear(&weak);
                ui::append(&weak, "status", &translations.new_conversation);
            }
            UserEvent::ConfigUpdated(new_cfg) => {
                cfg = *new_cfg;
                translations = tr(cfg.lang());
                ui::set_tr(&weak, translations.to_slint());
                system = build_system(cfg.lang(), mcp.as_ref().and_then(|m| m.instructions()).as_deref());
                convo[0] = Message::new_system_message(system.clone());
                llm = build_llm(&cfg, &functions);
                ui::set_status(&weak, &translations.settings_saved);
                ui::append(&weak, "status", &translations.settings_saved);
            }
            UserEvent::Input { text, files } => {
                let mut bubble = text.clone();
                for f in &files {
                    bubble.push_str(&format!("\n📎 {} ({})", f.display(), attachment_meta(f)));
                }
                ui::append(&weak, "user", bubble.trim());

                // Registre + backup AVANT le tour : restaurable depuis ⟲.
                let turn = ledger.next_turn();
                let preview: String = text.chars().take(60).collect();
                let backup = match mcp.as_ref() {
                    Some(m) => {
                        let mut args = Map::new();
                        args.insert("label".into(), json!(format!("lyra-turn-{turn:03}")));
                        match m.call("hypr_backup", args).await {
                            Ok(response) => parse_backup_name(&response),
                            Err(_) => None,
                        }
                    }
                    None => None,
                };
                ledger.push(TurnRecord {
                    turn,
                    timestamp: timefmt::iso(std::time::SystemTime::now()),
                    preview,
                    backup: backup.clone(),
                });
                ui::history_set(&weak, &ledger.records);

                ui::set_busy(&weak, true);
                if let Err(e) =
                    run_turn(&llm, mcp.as_ref(), &mut convo, &text, &files, &translations, &weak).await
                {
                    ui::append(&weak, "error", format!("{e:#}"));
                }
                ui::set_busy(&weak, false);
            }
            UserEvent::Restore(name) => {
                ui::set_busy(&weak, true);
                ui::append(&weak, "status", &translations.restore_started);
                let Some(mcp) = mcp.as_ref() else {
                    ui::append(&weak, "error", translations.mcp_unavailable.clone());
                    ui::set_busy(&weak, false);
                    continue;
                };
                let mut args = Map::new();
                args.insert("name".into(), json!(name));
                let outcome = match mcp.call("hypr_restore_backup", args).await {
                    Ok(report) => mcp.call("hypr_reload", Map::new()).await.map(|_| report),
                    Err(e) => Err(e),
                };
                match outcome {
                    Ok(_) => {
                        ui::append(&weak, "status", &translations.restore_done);
                        ui::set_status(&weak, &translations.restore_done);
                    }
                    Err(e) => {
                        ui::append(
                            &weak,
                            "error",
                            format!("{} : {e:#}", translations.restore_failed),
                        );
                    }
                }
                ui::set_busy(&weak, false);
            }
        }
    }
}

/// Prévient l'utilisateur si Claude Code ou ZCode ne sont pas enregistrés
/// auprès du panneau, avec les commandes prêtes à coller.
fn notify_missing_clients(weak: &Weak<ChatWindow>, cfg: &ChatConfig) {
    let regs = hyprpanel_core::detect_clients();
    if regs.all_registered() {
        return;
    }
    let Ok(settings) = hyprpanel_core::Settings::load() else { return };
    let snippets = hyprpanel_core::registration_snippets(&settings.token, settings.port);
    let t = tr(cfg.lang());
    let mut msg = format!("{} :", t.clients_notice);
    if !regs.claude_code {
        msg.push_str(&format!("\n\n• Claude Code — {}\n  {}", t.settings_clients_missing, snippets.claude_code));
    }
    if !regs.zcode {
        msg.push_str(&format!("\n\n• ZCode — {}\n  {}", t.settings_clients_missing, snippets.zcode));
    }
    ui::append(weak, "error", msg);
}

fn build_llm(cfg: &ChatConfig, functions: &[FunctionDefinition]) -> Claude {
    Claude::new()
        .with_model(cfg.model.clone())
        .with_api_key(cfg.api_key.clone())
        .with_base_url(cfg.base_url.clone())
        .with_options(
            CallOptions::new()
                .with_functions(functions.to_vec())
                .with_max_tokens(cfg.max_tokens),
        )
}

fn build_system(lang: Lang, server_instructions: Option<&str>) -> String {
    let mut system = match lang {
        Lang::Fr => String::from(
            "Tu es LYRA, l'assistante de configuration du bureau Hyprland, dans une fenêtre de chat native. \
Réponds en français, de façon concise et actionnable. Tu configures l'interface de l'utilisateur \
en direct : utilise les outils disponibles plutôt que de longues explications. Préviens avec \
hypr_notify avant un changement visible important. Si une demande est ambiguë ou risquée \
(perte de données, écrans, saisie), demande confirmation.",
        ),
        Lang::En => String::from(
            "You are LYRA, the Hyprland desktop configuration assistant, living in a native chat window. \
Answer concisely and actionably, in English. You configure the user's interface live: prefer using \
the available tools over long explanations. Warn with hypr_notify before any major visible change. \
If a request is ambiguous or risky (data loss, monitors, input), ask for confirmation. \
(The server instructions below are written in French.)",
        ),
    };
    if let Some(instructions) = server_instructions {
        system.push_str("\n\n—— Server instructions ——\n");
        system.push_str(instructions);
    }
    system
}

async fn run_turn(
    llm: &Claude,
    session: Option<&McpSession>,
    convo: &mut Vec<Message>,
    user_text: &str,
    files: &[PathBuf],
    translations: &Tr,
    weak: &Weak<ChatWindow>,
) -> anyhow::Result<()> {
    let (message, notes) = build_user_message(user_text, files, &|p| std::fs::read(p));
    for note in notes {
        ui::append(weak, "error", note);
    }
    convo.push(message);

    for _round in 0..MAX_ROUNDS {
        ui::set_status(weak, &translations.status_thinking);
        let (text, result) = stream_turn(llm, convo, weak).await?;

        if text.is_empty() && result.tool_calls.is_empty() {
            bail!("réponse du modèle sans contenu utilisable");
        }
        convo.push(Message::new_ai_message_with_tool_calls(&text, &result.tool_calls));

        let Some(session) = session else {
            if !result.tool_calls.is_empty() {
                ui::append(weak, "error", "le modèle veut appeler un outil mais le serveur MCP est indisponible");
            }
            break;
        };
        if result.tool_calls.is_empty() {
            break;
        }

        for tc in result.tool_calls {
            ui::set_status(weak, format!("{} {}…", translations.label_tool, tc.name));
            ui::append(weak, "tool", format!("→ {}({})", tc.name, compact_args(&tc.arguments)));
            let args = parse_tool_arguments(&tc.arguments);
            let call = session.call(&tc.name, args);
            let out = match tokio::time::timeout(TIMEOUT_TOOL, call).await {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => format!("erreur outil : {e:#}"),
                Err(_) => format!("outil {} : timeout après 30 s", tc.name),
            };
            ui::append(weak, "tool", truncate_str(&out, 400));
            convo.push(Message::new_tool_message(&out, tc.id.clone()));
        }
    }
    Ok(())
}

/// Consomme le flux d'événements LangChain : streame le texte dans la bulle
/// LYRA, renvoie le texte complet et le résultat final (tool calls inclus).
async fn stream_turn(
    llm: &Claude,
    convo: &[Message],
    weak: &Weak<ChatWindow>,
) -> anyhow::Result<(String, GenerateResult)> {
    let mut stream = llm.generate_stream(convo);
    let mut text = String::new();
    let mut bubble_created = false;
    let mut result: Option<GenerateResult> = None;

    loop {
        let next = tokio::time::timeout(TIMEOUT_LLM, stream.next())
            .await
            .map_err(|_| anyhow!("LLM : silence depuis {} s", TIMEOUT_LLM.as_secs()))?;
        let Some(event) = next else { break };
        match event.context("flux LLM")? {
            LLMEvent::Text(chunk) => {
                if !bubble_created {
                    bubble_created = true;
                    ui::append(weak, "assistant", "");
                }
                text.push_str(&chunk);
                ui::update_last(weak, "assistant", &text);
            }
            LLMEvent::Thinking(_) => {}
            LLMEvent::Done(r) => result = Some(r),
            _ => {}
        }
    }

    let result = result.context("flux terminé sans résultat final")?;
    if bubble_created && text.is_empty() {
        // bulle créée puis restée vide (tour outils sans texte) : on la retire
        ui::remove_last(weak);
    }
    Ok((text, result))
}

/// Les arguments arrivent en chaîne JSON (format OpenAI-like) → map.
fn parse_tool_arguments(raw: &str) -> Map<String, Value> {
    serde_json::from_str(raw).unwrap_or_else(|_| {
        let mut m = Map::new();
        m.insert("__arguments_bruts".into(), Value::String(raw.to_string()));
        m
    })
}

fn compact_args(raw: &str) -> String {
    truncate_str(raw.trim_matches('"'), 160)
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_pieces() {
        assert_eq!(classify_attachment(Path::new("/x/a.png")), AttachmentKind::Image);
        assert_eq!(classify_attachment(Path::new("/x/A.JPG")), AttachmentKind::Image);
        assert_eq!(classify_attachment(Path::new("/x/hyprland.lua")), AttachmentKind::Text);
        assert_eq!(classify_attachment(Path::new("/x/doc.pdf")), AttachmentKind::Unsupported);
        assert_eq!(classify_attachment(Path::new("/x/sans-ext")), AttachmentKind::Unsupported);
    }

    #[test]
    fn message_utilisateur_avec_pieces() {
        let png = PathBuf::from("/tmp/fake.png");
        let lua = PathBuf::from("/tmp/config.lua");
        let pdf = PathBuf::from("/tmp/doc.pdf");
        let read = |p: &Path| -> std::io::Result<Vec<u8>> {
            match p.extension().and_then(|e| e.to_str()) {
                Some("png") => Ok(vec![1, 2, 3, 4]),
                Some("lua") => Ok(b"gaps_in = 5".to_vec()),
                _ => unreachable!(),
            }
        };
        let (msg, notes) = build_user_message("voici ma config", &[png, lua, pdf], &read);
        assert!(msg.content.contains("voici ma config"));
        assert!(msg.content.contains("— fichier config.lua —"));
        assert!(msg.content.contains("gaps_in = 5"));
        assert_eq!(msg.images.as_ref().map(|v| v.len()), Some(1));
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("type non pris en charge"));
    }

    #[test]
    fn piece_seule_sans_texte() {
        let png = PathBuf::from("/tmp/img.png");
        let (msg, notes) = build_user_message("", &[png], &|_p| Ok(vec![9u8; 3]));
        assert!(notes.is_empty());
        assert_eq!(msg.images.as_ref().map(|v| v.len()), Some(1));
        assert!(!msg.content.is_empty());
    }

    #[test]
    fn rien_du_tout_message_de_secours() {
        let (msg, _) = build_user_message("ok", &[], &|_p| unreachable!());
        assert_eq!(msg.content, "ok");
        let (msg, _) = build_user_message("", &[], &|_p| unreachable!());
        assert_eq!(msg.content, "(pièce jointe illisible)");
    }

    #[test]
    fn parsing_nom_de_backup() {
        assert_eq!(
            parse_backup_name("backup créé : 20261108-073100-lyra-turn-001"),
            Some("20261108-073100-lyra-turn-001".into())
        );
        assert_eq!(parse_backup_name("aucun backup dans /x"), None);
        assert_eq!(parse_backup_name(""), None);
    }

    #[test]
    fn arguments_outil_parses_ou_recuperees() {
        let m = parse_tool_arguments(r#"{"option":"general:gaps_in","value":"12"}"#);
        assert_eq!(m.get("option").and_then(Value::as_str), Some("general:gaps_in"));
        let m = parse_tool_arguments("{json cassé");
        assert!(m.contains_key("__arguments_bruts"));
    }

    #[test]
    fn registre_persiste_et_reprend() {
        let path = std::env::temp_dir().join(format!("lyra-hist-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut ledger = Ledger::load(path.clone());
        assert_eq!(ledger.next_turn(), 1);
        ledger.push(TurnRecord {
            turn: 1,
            timestamp: "t".into(),
            preview: "p".into(),
            backup: Some("b".into()),
        });
        ledger.push(TurnRecord {
            turn: 2,
            timestamp: "t".into(),
            preview: "p".into(),
            backup: None,
        });
        drop(ledger);
        let reloaded = Ledger::load(path.clone());
        assert_eq!(reloaded.records.len(), 2);
        assert_eq!(reloaded.next_turn(), 3);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn troncature_utf8() {
        assert_eq!(truncate_str("abc", 5), "abc");
        let s = "é".repeat(10);
        let t = truncate_str(&s, 11);
        assert!(t.ends_with('…') && t.len() < 14);
    }
}
