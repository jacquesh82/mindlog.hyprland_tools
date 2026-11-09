//! Boucle d'agent LYRA sur **LangChain Rust** (module claude) :
//! streaming `LLMEvent` → bulles, tool use natif Anthropic (`FunctionDefinition`
//! + `Message::new_tool_message`), exécution des outils via le client MCP.
//!
//! La boucle d'exécution reste volontairement fine et locale (streaming vers
//! l'UI, pont MCP, timeouts, registre de tours restaurables) : les
//! `AgentExecutor` génériques n'exposent ni l'un ni l'autre.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use base64::Engine as _;
use futures_util::StreamExt;
use langchain_rust::language_models::options::CallOptions;
use langchain_rust::language_models::{GenerateResult, LLMEvent, llm::LLM};
use langchain_rust::llm::claude::Claude;
use langchain_rust::schemas::{FunctionDefinition, ImageContent, Message};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc::UnboundedReceiver;

use hyprpanel_core::timefmt;

use crate::chat_config::ChatConfig;
use crate::i18n::{Lang, Tr, tr};
use crate::mcp::McpSession;
use crate::ui;

/// Événements envoyés par l'UI vers la boucle agent.
pub enum UserEvent {
    Input {
        text: String,
        files: Vec<PathBuf>,
        pasted_images: Vec<tempfile::TempPath>,
    },
    Reset,
    ConfigUpdated(Box<ChatConfig>),
    Restore(String),
    Choice {
        display: String,
        context: String,
    },
    FilesystemChanged,
    EnvironmentRefresh,
    ClearMemory,
    /// Barrière (CLI) : répond une fois les événements précédents traités.
    Sync(tokio::sync::oneshot::Sender<()>),
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
                Ok(_) => notes.push(format!(
                    "📎 {name} : image trop volumineuse (> 5 Mo), non transmise"
                )),
                Err(e) => notes.push(format!("📎 {name} : illisible ({e})")),
            },
            AttachmentKind::Text => match read(file) {
                Ok(bytes) if bytes.len() as u64 <= MAX_TEXT_BYTES => {
                    let inner = String::from_utf8_lossy(&bytes);
                    content.push_str(&format!("\n\n— fichier {name} —\n{inner}\n— fin {name} —"));
                }
                Ok(_) => notes.push(format!(
                    "📎 {name} : fichier texte trop volumineux (> 64 Ko), non transmis"
                )),
                Err(e) => notes.push(format!("📎 {name} : illisible ({e})")),
            },
            AttachmentKind::Unsupported => {
                notes.push(format!(
                    "📎 {name} : type non pris en charge (images et fichiers texte uniquement)"
                ));
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
    (
        Message::new_human_message(content).with_images(images),
        notes,
    )
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

pub async fn run(
    cfg: ChatConfig,
    mut rx: UnboundedReceiver<UserEvent>,
    front: ui::Front,
    filesystem: Arc<crate::filesystem::FilesystemAccess>,
    approvals: crate::commands::Approvals,
) {
    let mut cfg = cfg;
    let mut translations = tr(cfg.lang());

    if cfg.provider == crate::chat_config::Provider::Zcode && cfg.api_key.trim().is_empty() {
        ui::append(
            &front,
            "error",
            "Clé API absente : remplissez « clé API » dans ⚙ réglages.\n/api key missing: fill it in the ⚙ settings panel.",
        );
    }

    // Connexion MCP + définitions d'outils pour le LLM.
    let mcp = match cfg.mcp_url() {
        Ok(url) => match McpSession::connect(&url).await {
            Ok(session) => {
                let n = session.tools().len();
                ui::set_status(
                    &front,
                    if filesystem.approved() {
                        translations.mcp_tools.replace("{}", &n.to_string())
                    } else {
                        translations.fs_denied.clone()
                    },
                );
                Some(session)
            }
            Err(e) => {
                ui::append(
                    &front,
                    "error",
                    format!("{} : {e:#}", translations.mcp_unavailable),
                );
                ui::set_status(&front, "MCP ✗");
                None
            }
        },
        Err(e) => {
            ui::append(
                &front,
                "error",
                format!("Config du daemon introuvable : {e} (lancez `hyprpanel init`)"),
            );
            ui::set_status(&front, "daemon ✗");
            None
        }
    };
    let mut environment_ready = filesystem.approved();
    if environment_ready {
        refresh_environment(&front, &filesystem, &approvals, &translations).await;
    }
    let mut functions = available_functions(mcp.as_ref(), &filesystem);

    let mut llm = build_llm(&cfg, &functions);
    let mut system = runtime_system(&cfg, mcp.as_ref(), &filesystem, None);
    let mut convo: Vec<Message> = vec![Message::new_system_message(system.clone())];

    let mut ledger = Ledger::load(crate::chat_config::state_history_path());
    ui::history_set(&front, &ledger.records);

    while let Some(event) = rx.recv().await {
        let (event, choice_display) = match event {
            UserEvent::Choice { display, context } => (
                UserEvent::Input {
                    text: context,
                    files: Vec::new(),
                    pasted_images: Vec::new(),
                },
                Some(display),
            ),
            event => (event, None),
        };
        match event {
            UserEvent::Sync(done) => {
                let _ = done.send(());
            }
            UserEvent::FilesystemChanged => {
                if filesystem.approved() && !environment_ready {
                    refresh_environment(&front, &filesystem, &approvals, &translations).await;
                    environment_ready = true;
                } else {
                    if !filesystem.approved() {
                        environment_ready = false;
                        set_environment(&front, String::new());
                    }
                }
                functions = available_functions(mcp.as_ref(), &filesystem);
                llm = build_llm(&cfg, &functions);
                system = runtime_system(&cfg, mcp.as_ref(), &filesystem, None);
                convo[0] = Message::new_system_message(system.clone());
                if !filesystem.approved() {
                    convo.truncate(1);
                }
            }
            UserEvent::EnvironmentRefresh => {
                if filesystem.approved() {
                    refresh_environment(&front, &filesystem, &approvals, &translations).await;
                    system = runtime_system(&cfg, mcp.as_ref(), &filesystem, None);
                    convo[0] = Message::new_system_message(system.clone());
                }
            }
            UserEvent::Choice { .. } => unreachable!("choix normalisé en message utilisateur"),
            UserEvent::ClearMemory => match crate::memory::clear() {
                Ok(()) => {
                    system = runtime_system(&cfg, mcp.as_ref(), &filesystem, None);
                    convo = vec![Message::new_system_message(system.clone())];
                    ui::clear(&front);
                    ui::set_status(&front, &translations.memory_cleared);
                }
                Err(error) => {
                    ui::append(&front, "error", format!("Mémoire : {error:#}"));
                }
            },
            UserEvent::Reset => {
                system = runtime_system(&cfg, mcp.as_ref(), &filesystem, None);
                convo = vec![Message::new_system_message(system.clone())];
                ui::clear(&front);
                ui::set_status(&front, &translations.new_conversation);
            }
            UserEvent::ConfigUpdated(new_cfg) => {
                cfg = *new_cfg;
                filesystem.set_operation_mode(cfg.operation_mode);
                functions = available_functions(mcp.as_ref(), &filesystem);
                translations = tr(cfg.lang());
                ui::set_tr(&front, &translations);
                system = runtime_system(&cfg, mcp.as_ref(), &filesystem, None);
                convo[0] = Message::new_system_message(system.clone());
                llm = build_llm(&cfg, &functions);
                ui::set_status(&front, &translations.settings_saved);
                ui::append(&front, "status", &translations.settings_saved);
            }
            UserEvent::Input {
                text,
                files,
                pasted_images: _pasted_images,
            } => {
                // Recherche avant l'ajout de la demande courante : elle ne se retrouve pas elle-même.
                system = runtime_system(&cfg, mcp.as_ref(), &filesystem, Some(&text));
                convo[0] = Message::new_system_message(system.clone());
                if let Err(error) = crate::memory::remember(
                    filesystem.approved(),
                    cfg.memory_enabled,
                    &text,
                    choice_display.is_some(),
                ) {
                    ui::append(&front, "error", format!("Mémoire locale : {error:#}"));
                }
                let visible_text = choice_display.as_deref().unwrap_or(&text);
                let mut bubble = visible_text.to_string();
                for f in &files {
                    bubble.push_str(&format!("\n📎 {} ({})", f.display(), attachment_meta(f)));
                }
                ui::append(&front, "user", bubble.trim());
                ui::set_activity(&front, "tools");
                let turn = ledger.next_turn();
                let preview: String = visible_text.chars().take(60).collect();
                let backup = match mcp
                    .as_ref()
                    .filter(|_| filesystem.allows_mcp("hypr_backup"))
                {
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
                    backup,
                });
                ui::history_set(&front, &ledger.records);
                ui::set_busy(&front, true);
                if let Err(e) = run_turn(
                    &cfg,
                    &functions,
                    &llm,
                    mcp.as_ref(),
                    &mut convo,
                    &text,
                    &files,
                    &translations,
                    &front,
                    &filesystem,
                    &approvals,
                )
                .await
                {
                    ui::append(&front, "error", format!("{e:#}"));
                }
                ui::set_busy(&front, false);
                ui::set_status(
                    &front,
                    if !filesystem.approved() {
                        translations.fs_denied.clone()
                    } else {
                        match &mcp {
                            Some(m) => translations
                                .mcp_tools
                                .replace("{}", &m.tools().len().to_string()),
                            None => translations.mcp_unavailable.clone(),
                        }
                    },
                );
            }
            UserEvent::Restore(name) => {
                if !filesystem.operation_mode().can_operate() {
                    ui::append(&front, "status", &translations.operation_copy_hint);
                    continue;
                }
                if !filesystem.approved() {
                    ui::append(&front, "error", &translations.fs_denied);
                    continue;
                }
                ui::set_activity(&front, "restore");
                ui::set_busy(&front, true);
                ui::append(&front, "status", &translations.restore_started);
                let Some(mcp) = mcp.as_ref() else {
                    ui::append(&front, "error", &translations.mcp_unavailable);
                    ui::set_busy(&front, false);
                    continue;
                };
                let mut args = Map::new();
                args.insert("name".into(), json!(name));
                if filesystem.operation_mode().confirm_desktop()
                    && !approvals
                        .request(
                            &front,
                            format!(
                                "hypr_restore_backup({})\nhypr_reload({{}})",
                                Value::Object(args.clone())
                            ),
                            translations.command_restore.clone(),
                        )
                        .await
                {
                    ui::append(&front, "status", &translations.command_declined);
                    ui::set_busy(&front, false);
                    continue;
                }
                if !filesystem.allows_mcp("hypr_restore_backup") {
                    ui::set_busy(&front, false);
                    continue;
                }
                let outcome = match mcp.call("hypr_restore_backup", args).await {
                    Ok(report) => mcp.call("hypr_reload", Map::new()).await.map(|_| report),
                    Err(e) => Err(e),
                };
                match outcome {
                    Ok(_) => {
                        ui::append(&front, "status", &translations.restore_done);
                        ui::set_status(&front, &translations.restore_done);
                    }
                    Err(e) => {
                        ui::append(
                            &front,
                            "error",
                            format!("{} : {e:#}", translations.restore_failed),
                        );
                    }
                };
                ui::set_busy(&front, false);
            }
        }
    }
}

fn set_environment(front: &ui::Front, text: String) {
    let ui::Front::Window(weak) = front.clone() else {
        return;
    };
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = weak.upgrade() {
            app.set_environment_text(text.into());
        }
    });
}

async fn refresh_environment(
    front: &ui::Front,
    filesystem: &Arc<crate::filesystem::FilesystemAccess>,
    approvals: &crate::commands::Approvals,
    translations: &Tr,
) {
    ui::set_busy(front, true);
    ui::set_status(front, &translations.environment_scanning);
    ui::set_activity(front, "tools");
    let result = async {
        filesystem.require_approval()?;
        let mut profile = crate::environment::prepare(&crate::environment::path())?;
        let fs = filesystem.clone();
        let managers = profile.package_managers.clone();
        let progress_front = front.clone();
        let progress_tr = translations.clone();
        let mut inventory = tokio::task::spawn_blocking(move || {
            let mut previous = String::new();
            let mut last = std::time::Instant::now();
            crate::inventory::discover_with_progress(
                &fs,
                Path::new("/"),
                &managers,
                |phase, done, total| {
                    if phase == previous && total != Some(done) && last.elapsed().as_millis() < 80 {
                        return;
                    }
                    previous = phase.into();
                    last = std::time::Instant::now();
                    let name = match phase {
                        "configurations" => progress_tr.inventory_configurations.clone(),
                        "launchers" => progress_tr.inventory_launchers.clone(),
                        "packages" => progress_tr.inventory_packages.clone(),
                        other => format!("{} ({other})", progress_tr.inventory_packages),
                    };
                    let label = match total {
                        Some(total) => format!("{name} · {done}/{total}"),
                        None => format!("{name} · {done}"),
                    };
                    ui::set_progress(
                        &progress_front,
                        label,
                        total.map(|total| {
                            if total == 0 {
                                1.
                            } else {
                                done as f32 / total as f32
                            }
                        }),
                    );
                },
            )
        })
        .await??;
        let mut declined = false;
        let plans = std::mem::take(&mut inventory.plans);
        let total = plans.len();
        for (index, query) in plans.into_iter().enumerate() {
            if declined {
                inventory.record(query.manager, "not_queried_after_decline", Vec::new(), None);
                continue;
            }
            filesystem.require_approval()?;
            ui::set_progress(
                front,
                format!(
                    "{} · {}/{} · {}",
                    translations.inventory_queries,
                    index + 1,
                    total,
                    query.manager
                ),
                None,
            );
            let reason = translations.environment_query.replace("{}", query.manager);
            let confirmation = approvals.request(front, query.command.into(), reason.clone());
            let request = crate::commands::Request {
                command: query.command.into(),
                reason,
            };
            match crate::commands::run_inventory(request, filesystem, confirmation).await {
                Ok(output) if output["status"] == "manual_only" => {
                    show_manual_command(front, &output, translations);
                    inventory.record(query.manager, "manual_only", Vec::new(), None);
                }
                Ok(output) if output["status"] == "declined" => {
                    declined = true;
                    inventory.record(query.manager, "declined", Vec::new(), None);
                }
                Ok(output) if output["success"] == true && output["truncated"] != true => {
                    match crate::inventory::parse_query(
                        query.manager,
                        output["stdout"].as_str().unwrap_or(""),
                    ) {
                        Ok(packages) => {
                            inventory.record(query.manager, "confirmed_query", packages, None)
                        }
                        Err(error) => inventory.record(
                            query.manager,
                            "failed",
                            Vec::new(),
                            Some(error.to_string()),
                        ),
                    }
                }
                Ok(output) => inventory.record(
                    query.manager,
                    "failed",
                    Vec::new(),
                    Some(
                        if output["truncated"] == true {
                            "query output exceeded limit"
                        } else {
                            "query failed or timed out"
                        }
                        .into(),
                    ),
                ),
                Err(error) => {
                    inventory.record(query.manager, "failed", Vec::new(), Some(error.to_string()))
                }
            }
        }
        filesystem.require_approval()?;
        ui::set_progress(front, translations.inventory_saving.clone(), None);
        profile.desktop_tools = inventory.finish();
        profile.inventory_sources = inventory.sources;
        profile.inventory_limits = inventory.limits.into_iter().collect();
        for source in &profile.inventory_sources {
            if !profile.package_managers.contains(&source.manager) {
                profile.package_managers.push(source.manager.clone());
            }
        }
        crate::environment::save(&crate::environment::path(), &profile)?;
        Ok::<_, anyhow::Error>(profile)
    }
    .await;
    match result {
        Ok(profile) => {
            set_environment(
                front,
                serde_json::to_string_pretty(&profile).unwrap_or_default(),
            );
            ui::set_status(front, &translations.environment_refreshed);
        }
        Err(error) => {
            ui::append(
                front,
                "error",
                format!("Profil de l’environnement : {error:#}"),
            );
        }
    };
    ui::set_busy(front, false);
}

fn available_functions(
    mcp: Option<&McpSession>,
    filesystem: &crate::filesystem::FilesystemAccess,
) -> Vec<FunctionDefinition> {
    let mut functions: Vec<FunctionDefinition> = mcp
        .as_ref()
        .map(|m| {
            m.tools()
                .iter()
                .filter(|t| filesystem.allows_mcp(&t.name))
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
    functions.extend(crate::choices::tool_definitions());
    functions.push(crate::contrast::definition());
    functions.extend(crate::web::definitions());
    functions.extend(filesystem.tool_definitions());
    if filesystem.approved() || !filesystem.operation_mode().can_operate() {
        functions.push(crate::commands::definition());
    }
    functions
}

fn runtime_system(
    cfg: &ChatConfig,
    mcp: Option<&McpSession>,
    filesystem: &crate::filesystem::FilesystemAccess,
    query: Option<&str>,
) -> String {
    let mut instructions = mcp.and_then(|m| m.instructions()).unwrap_or_default();
    let directory = if filesystem.approved() {
        filesystem.root().display().to_string()
    } else {
        "~/.config".into()
    };
    instructions.push_str(&format!(
        "\n\nFILESYSTEM PERMISSION: {}. Configuration directory: {directory}. ",
        if filesystem.approved() {
            "GRANTED for this session"
        } else {
            "NOT GRANTED"
        }
    ));
    instructions.push_str(if filesystem.approved() && !filesystem.operation_mode().can_operate() {
        "Read-only analysis: use lyra_list_directory and lyra_read_file. Copy-only mode blocks file writes, all desktop/MCP operations, command execution, backups and restores. lyra_run_command only displays a command to copy."
    } else if filesystem.approved() {
        "Use lyra_list_directory, lyra_read_file and lyra_write_file to work with configuration files. Never claim an edit succeeded without its tool result. Only write when the user has requested that change. Private LYRA files and generated files remain protected. Raw Lua and arbitrary dispatch tools are unavailable."
    } else {
        "No filesystem or desktop tools are authorized. Do not read, back up or modify local configuration through another mechanism. Ask the user to open Settings and allow filesystem access when needed. Explicitly attached files can still be discussed."
    });
    instructions.push_str("\n\n");
    instructions.push_str(&crate::environment::context(filesystem.approved()));
    if let Some(query) = query {
        instructions.push_str(
            &crate::memory::recall(filesystem.approved(), cfg.memory_enabled, query)
                .unwrap_or_default(),
        );
    }
    instructions.push_str(&format!(
        "\nCurrent UTC time: {}",
        timefmt::iso(std::time::SystemTime::now())
    ));
    build_system_with_policy(
        cfg.lang(),
        Some(&instructions),
        &cfg.system_context,
        cfg.limit_wayland_scope,
        cfg.operation_mode,
    )
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

pub(crate) fn build_system(
    lang: Lang,
    server_instructions: Option<&str>,
    custom_context: &str,
) -> String {
    build_system_with_scope(lang, server_instructions, custom_context, true)
}

pub(crate) fn build_system_with_scope(
    lang: Lang,
    server_instructions: Option<&str>,
    custom_context: &str,
    limit_wayland_scope: bool,
) -> String {
    build_system_with_policy(
        lang,
        server_instructions,
        custom_context,
        limit_wayland_scope,
        crate::operations::Mode::ConfirmAll,
    )
}

fn build_system_with_policy(
    lang: Lang,
    server_instructions: Option<&str>,
    custom_context: &str,
    limit_wayland_scope: bool,
    operation_mode: crate::operations::Mode,
) -> String {
    let mut system = if custom_context.trim().is_empty() {
        if !limit_wayland_scope {
            match lang {
                Lang::Fr => "Tu es LYRA, une assistante généraliste dans une fenêtre native. Réponds en français aux demandes de l'utilisateur, de façon claire et utile. Utilise les outils disponibles lorsque cela aide, en respectant les autorisations et confirmations.".into(),
                Lang::En => "You are LYRA, a general-purpose assistant in a native chat window. Answer the user's requests clearly and helpfully in English. Use available tools when helpful while preserving permissions and confirmations.".into(),
            }
        } else {
            match lang {
                Lang::Fr => String::from(
                    "Tu es LYRA, l'assistante du bureau Hyprland, de Wayland et de leur outillage, dans une fenêtre de chat native. \
Réponds en français, de façon concise et actionnable. Tu configures l'interface de l'utilisateur \
en direct : utilise les outils disponibles plutôt que de longues explications. Préviens avec \
hypr_notify avant un changement visible important. Si une demande est ambiguë ou risquée \
(perte de données, écrans, saisie), demande confirmation.",
                ),
                Lang::En => String::from(
                    "You are LYRA, the Hyprland, Wayland, and related desktop tools assistant, living in a native chat window. \
Answer concisely and actionably, in English. You configure the user's interface live: prefer using \
the available tools over long explanations. Warn with hypr_notify before any major visible change. \
If a request is ambiguous or risky (data loss, monitors, input), ask for confirmation. \
(The server instructions below are written in French.)",
                ),
            }
        }
    } else {
        custom_context.trim().to_string()
    };
    system.push_str(match lang {
        Lang::Fr => "\n\nCHOIX VISUELS : dès qu'une préférence ou une décision doit venir de l'utilisateur, utilise lyra_offer_choices pour afficher des options cliquables (texte, couleurs/palettes, images locales existantes ou schémas SVG autonomes). Pour les goûts et couleurs, consulte d'abord lyra_palette_catalog : il fournit une sélection Color Hunt avec les liens sources. Reprends les couleurs exactes et source_url ; ne présente pas une palette inventée comme provenant d'un site. Après avoir proposé les choix, attends le message user_choice issu du clic. Le clic exprime une préférence, pas une autorisation générale : respecte les confirmations et sauvegardes nécessaires avant de modifier le bureau. Les détails d'outils sont repliés dans l'interface ; garde tes messages visibles courts et utiles.",
        Lang::En => "\n\nVISUAL CHOICES: whenever a preference or decision belongs to the user, use lyra_offer_choices to show clickable text, colors/palettes, existing local images or self-contained SVG diagrams. For colors, first consult lyra_palette_catalog for a selection from Color Hunt with source URLs. Preserve the exact colors and source_url; do not claim invented palettes come from a site. After offering choices, wait for the user_choice message produced by a click. A click expresses a preference, not blanket authorization: preserve the required confirmations and backups before changing the desktop. Tool details are collapsed in the UI; keep visible messages concise and useful.",
    });
    if let Some(instructions) = server_instructions {
        system.push_str("\n\n—— Server instructions ——\n");
        system.push_str(instructions);
    }
    system.push_str(&crate::contrast::instructions(lang));
    system.push_str(&crate::reliability::instructions_with_mode(
        lang,
        operation_mode,
    ));
    if limit_wayland_scope {
        system.push_str(&crate::scope::instructions(lang));
    } else {
        system.push_str(crate::scope::unrestricted(lang));
    }
    system
}

fn show_manual_command(front: &ui::Front, output: &Value, translations: &Tr) {
    let command = output["command"].as_str().unwrap_or("");
    let reason = output["reason"].as_str().unwrap_or("");
    // Le bloc utilise le bouton Copier du moteur Markdown natif.
    let fence = "`".repeat(
        command
            .split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(3),
    );
    ui::append(
        front,
        "assistant",
        format!(
            "**{}**\n\n{reason}\n\n{fence}sh\n{command}\n{fence}",
            translations.manual_command_notice
        ),
    );
}

async fn run_turn(
    cfg: &ChatConfig,
    functions: &[FunctionDefinition],
    llm: &Claude,
    session: Option<&McpSession>,
    convo: &mut Vec<Message>,
    user_text: &str,
    files: &[PathBuf],
    translations: &Tr,
    front: &ui::Front,
    filesystem: &crate::filesystem::FilesystemAccess,
    approvals: &crate::commands::Approvals,
) -> anyhow::Result<()> {
    cfg.validate_subscription().map_err(anyhow::Error::msg)?;
    let (message, notes) = build_user_message(user_text, files, &|p| std::fs::read(p));
    for note in notes {
        ui::append(front, "error", note);
    }
    convo.push(message);

    for _round in 0..MAX_ROUNDS {
        ui::set_activity(front, "thinking");
        ui::set_status(front, &translations.status_thinking);
        let (text, result) = if cfg.provider == crate::chat_config::Provider::Zcode {
            stream_turn(llm, convo, front).await?
        } else {
            let mut row = None;
            let mut last_flush = std::time::Instant::now();
            let result = crate::providers::generate(cfg, functions, convo, &mut |partial| {
                if partial.is_empty() {
                    return;
                }
                if let Some(index) = row {
                    if last_flush.elapsed() >= Duration::from_millis(33) {
                        ui::set_row(front, index, "assistant", partial);
                        last_flush = std::time::Instant::now();
                    }
                } else {
                    ui::set_activity(front, "writing");
                    row = Some(ui::append(front, "assistant", partial));
                    last_flush = std::time::Instant::now();
                }
            })
            .await?;
            let text = result.generation.clone();
            if let Some(index) = row {
                ui::set_row(front, index, "assistant", &text);
            } else if !text.is_empty() {
                ui::set_activity(front, "writing");
                ui::append(front, "assistant", &text);
            }
            (text, result)
        };

        if text.is_empty() && result.tool_calls.is_empty() {
            bail!("réponse du modèle sans contenu utilisable");
        }
        convo.push(Message::new_ai_message_with_tool_calls(
            &text,
            &result.tool_calls,
        ));

        if result.tool_calls.is_empty() {
            break;
        }

        let mut details_row = None;
        let mut details = String::new();
        let mut awaiting_choice = false;
        let mut declined = false;
        for tc in result.tool_calls {
            let out = if awaiting_choice || declined {
                json!({"status": "deferred", "reason": if declined { "user_declined_previous_action" } else { "awaiting_user_choice" }}).to_string()
            } else if tc.name == crate::choices::OFFER_TOOL {
                match authorized_choices(&tc.arguments, filesystem) {
                    Ok(request) => {
                        ui::offer_choices(front, request);
                        awaiting_choice = true;
                        json!({"status": "awaiting_user_choice", "instruction": "Wait for the user's click; do not choose for them."}).to_string()
                    }
                    Err(error) => json!({"error": error.to_string()}).to_string(),
                }
            } else {
                let row = *details_row.get_or_insert_with(|| ui::append(front, "tool", ""));
                ui::set_activity(front, "tools");
                ui::set_status(front, &translations.status_tools);
                if !details.is_empty() {
                    details.push_str("\n\n");
                }
                details.push_str(&format!("→ {}({})", tc.name, compact_args(&tc.arguments)));
                ui::set_row(front, row, "tool", &details);
                let out = if tc.name == crate::choices::PALETTE_TOOL {
                    crate::choices::palette_catalog().to_string()
                } else if tc.name == crate::contrast::TOOL {
                    let args: Value = serde_json::from_str(&tc.arguments).unwrap_or(Value::Null);
                    match crate::contrast::call(&args) {
                        Ok(value) => value.to_string(),
                        Err(error) => {
                            json!({"error":error.to_string(),"verified":false}).to_string()
                        }
                    }
                } else if crate::web::is_tool(&tc.name) {
                    let args: Value = serde_json::from_str(&tc.arguments).unwrap_or(Value::Null);
                    match crate::web::call(&tc.name, &args).await {
                        Ok(value) => value.to_string(),
                        Err(error) => json!({"error":error.to_string(),"verified":false,"instruction":"Verification failed. Do not invent a result; fetch an official source directly or explain the uncertainty."}).to_string(),
                    }
                } else if tc.name == crate::commands::TOOL {
                    let args: Value = serde_json::from_str(&tc.arguments).unwrap_or(Value::Null);
                    match crate::commands::Request::parse(&args) {
                        Ok(request) => {
                            let confirmation = approvals.request(
                                front,
                                request.command.clone(),
                                request.reason.clone(),
                            );
                            match crate::commands::run(request, filesystem, confirmation).await {
                                Ok(value) => {
                                    if value["status"] == "manual_only" {
                                        show_manual_command(front, &value, translations);
                                    }
                                    value.to_string()
                                }
                                Err(error) => json!({"error":error.to_string()}).to_string(),
                            }
                        }
                        Err(error) => json!({"error":error.to_string()}).to_string(),
                    }
                } else if crate::filesystem::FilesystemAccess::is_tool(&tc.name) {
                    let args: Value = serde_json::from_str(&tc.arguments).unwrap_or(Value::Null);
                    match filesystem.call(&tc.name, &args) {
                        Ok(value) => value.to_string(),
                        Err(error) => json!({"error":error.to_string()}).to_string(),
                    }
                } else if !filesystem.allows_mcp(&tc.name) {
                    json!({"error":"Filesystem/desktop access is not authorized, or this raw tool is unavailable. Ask the user to review permissions in Settings."}).to_string()
                } else if let Some(session) = session {
                    let args = parse_tool_arguments(&tc.arguments);
                    if filesystem.operation_mode().confirm_desktop()
                        && crate::commands::needs_mcp_approval(&tc.name)
                        && !approvals
                            .request(
                                front,
                                format!("{}({})", tc.name, Value::Object(args.clone())),
                                translations.command_desktop.clone(),
                            )
                            .await
                    {
                        json!({"status":"declined","instruction":"The user declined this action. Do not retry or bypass with another tool."}).to_string()
                    } else if !filesystem.allows_mcp(&tc.name) {
                        json!({"error":"Permission revoked"}).to_string()
                    } else {
                        ui::set_activity(front, "tools");
                        let call = session.call(&tc.name, args);
                        match tokio::time::timeout(TIMEOUT_TOOL, call).await {
                            Ok(Ok(o)) => o,
                            Ok(Err(e)) => format!("erreur outil : {e:#}"),
                            Err(_) => format!("outil {} : timeout après 30 s", tc.name),
                        }
                    }
                } else {
                    translations.mcp_unavailable.clone()
                };
                details.push('\n');
                details.push_str(&out);
                ui::set_row(front, row, "tool", &details);
                out
            };
            if serde_json::from_str::<Value>(&out)
                .ok()
                .is_some_and(|v| v["status"] == "declined")
            {
                declined = true;
                ui::append(front, "status", &translations.command_declined);
            }
            // Tous les tool_use reçoivent un résultat, même ceux différés par le choix.
            convo.push(Message::new_tool_message(&out, tc.id.clone()));
        }
        if awaiting_choice || declined {
            break;
        }
    }
    Ok(())
}

fn authorized_choices(
    raw: &str,
    filesystem: &crate::filesystem::FilesystemAccess,
) -> anyhow::Result<crate::choices::ChoiceRequest> {
    anyhow::ensure!(raw.len() <= 600_000, "proposition trop volumineuse");
    let request: crate::choices::ChoiceRequest =
        serde_json::from_str(raw).context("proposition mal formée")?;
    for option in &request.options {
        if let Some(path) = &option.image_path {
            filesystem.check_preview(path)?;
        }
    }
    request.validate()?;
    Ok(request)
}

/// Consomme le flux d'événements LangChain : streame le texte dans la bulle
/// LYRA, renvoie le texte complet et le résultat final (tool calls inclus).
async fn stream_turn(
    llm: &Claude,
    convo: &[Message],
    front: &ui::Front,
) -> anyhow::Result<(String, GenerateResult)> {
    let mut stream = llm.generate_stream(convo);
    let mut text = String::new();
    let mut bubble_created = false;
    let mut writing = false;
    let mut result: Option<GenerateResult> = None;
    let mut last_flush = std::time::Instant::now();
    let mut pending_text = false;

    loop {
        let next = match tokio::time::timeout(TIMEOUT_LLM, stream.next()).await {
            Ok(next) => next,
            Err(_) => {
                if pending_text {
                    ui::update_last(front, "assistant", &text);
                }
                return Err(anyhow!("LLM : silence depuis {} s", TIMEOUT_LLM.as_secs()));
            }
        };
        let Some(event) = next else { break };
        let event = match event.context("flux LLM") {
            Ok(event) => event,
            Err(error) => {
                if pending_text {
                    ui::update_last(front, "assistant", &text);
                }
                return Err(error);
            }
        };
        match event {
            LLMEvent::Text(chunk) => {
                if chunk.is_empty() {
                    continue;
                }
                if !writing {
                    writing = true;
                    ui::set_activity(front, "writing");
                }
                let first_chunk = !bubble_created;
                if first_chunk {
                    bubble_created = true;
                    ui::append(front, "assistant", "");
                }
                text.push_str(&chunk);
                pending_text = true;
                if first_chunk || last_flush.elapsed() >= Duration::from_millis(33) {
                    ui::update_last(front, "assistant", &text);
                    pending_text = false;
                    last_flush = std::time::Instant::now();
                }
            }
            LLMEvent::Thinking(_) => {
                if writing {
                    writing = false;
                    ui::set_activity(front, "thinking");
                }
            }
            LLMEvent::Done(r) => result = Some(r),
            _ => {}
        }
    }

    // Toujours vider le dernier fragment, même sur une interruption du flux.
    if pending_text {
        ui::update_last(front, "assistant", &text);
    }
    let result = result.context("flux terminé sans résultat final")?;
    if bubble_created && text.is_empty() {
        // bulle créée puis restée vide (tour outils sans texte) : on la retire
        ui::remove_last(front);
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
    fn denied_access_keeps_pure_choices_but_never_probes_local_images() {
        let dir = tempfile::tempdir().unwrap();
        let fs = crate::filesystem::FilesystemAccess::new(
            dir.path().join("missing-config"),
            dir.path().join("backups"),
        );
        assert_eq!(available_functions(None, &fs).len(), 5);
        let images = json!({"question":"Choose","options":[
            {"id":"a","label":"A","image_path":"/missing-private-file.png"},
            {"id":"b","label":"B"}
        ]})
        .to_string();
        let error = authorized_choices(&images, &fs).unwrap_err().to_string();
        assert!(
            error.contains("non autorisé"),
            "l'autorisation doit précéder tout accès ou test de chemin"
        );
        let colors = json!({"question":"Choose","options":[
            {"id":"a","label":"A","colors":["#112233"]}, {"id":"b","label":"B"}
        ]})
        .to_string();
        assert!(authorized_choices(&colors, &fs).is_ok());
        fs.set_approved(true);
        assert_eq!(available_functions(None, &fs).len(), 9);
    }

    #[test]
    fn system_context_replaces_role_and_retains_tool_instructions() {
        let custom = "You are an accessibility-focused desktop assistant.";
        let system = build_system(
            Lang::En,
            Some("Keep generated configuration files read-only."),
            custom,
        );
        assert!(system.starts_with(custom));
        assert!(!system.contains("living in a native chat window"));
        assert!(system.contains("lyra_offer_choices"));
        assert!(system.contains("Keep generated configuration files read-only."));
        assert!(system.contains("lyra_web_search"));
        assert!(system.contains("lyra_web_fetch"));
        assert!(system.contains("lyra_run_command"));
        assert!(system.contains("lyra_check_contrast"));
        assert!(system.contains("WCAG 2.2 AA"));
        assert!(system.contains("requires native user confirmation"));
        assert!(
            system.ends_with(crate::scope::redirect(Lang::En)),
            "le périmètre commun s'applique aussi au rôle personnalisé"
        );
        assert!(build_system(Lang::Fr, None, custom).ends_with(crate::scope::redirect(Lang::Fr)));
        assert!(build_system(Lang::Fr, None, "").starts_with("Tu es LYRA"));
        assert!(build_system(Lang::En, None, "   ").starts_with("You are LYRA"));
    }

    #[test]
    fn disabling_scope_removes_redirection_and_retains_permissions_and_contrast() {
        for lang in [Lang::Fr, Lang::En] {
            let system = build_system_with_scope(lang, None, "", false);
            assert!(system.ends_with(crate::scope::unrestricted(lang)));
            assert!(!system.contains(crate::scope::redirect(lang)));
            assert!(system.contains("lyra_run_command"));
            assert!(system.contains("lyra_check_contrast"));
            let custom =
                build_system_with_scope(lang, None, "Only answer Hyprland questions.", false);
            assert!(custom.ends_with(crate::scope::unrestricted(lang)));
        }
        assert!(crate::scope::instructions(Lang::Fr).contains("thème pour Brave"));
        assert!(crate::scope::instructions(Lang::En).contains("theme for Brave"));
    }

    #[test]
    fn classification_pieces() {
        assert_eq!(
            classify_attachment(Path::new("/x/a.png")),
            AttachmentKind::Image
        );
        assert_eq!(
            classify_attachment(Path::new("/x/A.JPG")),
            AttachmentKind::Image
        );
        assert_eq!(
            classify_attachment(Path::new("/x/hyprland.lua")),
            AttachmentKind::Text
        );
        assert_eq!(
            classify_attachment(Path::new("/x/doc.pdf")),
            AttachmentKind::Unsupported
        );
        assert_eq!(
            classify_attachment(Path::new("/x/sans-ext")),
            AttachmentKind::Unsupported
        );
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
        assert_eq!(
            m.get("option").and_then(Value::as_str),
            Some("general:gaps_in")
        );
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
