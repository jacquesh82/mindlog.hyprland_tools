//! Mode `serve` : LYRA pilotée par une interface tierce (hyprdesk) en JSON
//! Lines sur stdin/stdout. Même boucle d'agent que la fenêtre et `cli`
//! (fournisseurs, MCP, sauvegardes par tour, mémoire, modes d'exécution,
//! `context.d`) ; stdout ne porte que le protocole, les journaux vont sur
//! stderr. Une conversation par processus ; fin de stdin ⇒ fin propre.
//!
//! Une confirmation n'est lue que dans le protocole (message `confirm`) :
//! sans réponse elle reste en attente, jamais acceptée par défaut. Quand
//! stdin est fermé, plus personne ne peut répondre : elle est refusée.

use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::{Cancel, UserEvent};
use crate::chat_config::{ChatConfig, Provider};
use crate::choices::ChoiceRequest;
use crate::commands::Approvals;
use crate::i18n::Tr;
use crate::ui::Front;

/// Version du protocole annoncée dans `ready` et exigée dans `hello`.
pub const PROTOCOL: u32 = 1;
/// Identifiant de la confirmation d'accès aux fichiers (avant `ready`).
pub const FS_REQUEST: &str = "fs";
const RESULT_CHARS: usize = 200;

pub const USAGE: &str = "\
  hyprpanel-chat serve [--provider <zcode|claude_code|codex>] [--fs | --no-fs]
                           Protocole JSON Lines sur stdin/stdout pour une
                           interface tierce (hyprdesk) : même agent et mêmes
                           règles, une conversation par processus. Sans --fs
                           ni --no-fs, l'accès fichiers est demandé par un
                           confirm_request « fs ».
";

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq)]
pub struct Options {
    pub provider: Option<Provider>,
    pub filesystem: Option<bool>,
}

pub fn parse_args(args: &[String]) -> anyhow::Result<Options> {
    let mut options = Options::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let provider = match arg.as_str() {
            "--fs" => {
                options.filesystem = Some(true);
                continue;
            }
            "--no-fs" => {
                options.filesystem = Some(false);
                continue;
            }
            "--provider" => iter
                .next()
                .context("--provider attend une valeur : zcode, claude_code ou codex")?
                .as_str(),
            other => match other.strip_prefix("--provider=") {
                Some(value) => value,
                None => bail!("option inconnue : {other}"),
            },
        };
        options.provider = Some(Provider::parse(provider).map_err(anyhow::Error::msg)?);
    }
    Ok(options)
}

// ---------------------------------------------------------------------------
// Messages client → LYRA
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Attachment {
    pub path: PathBuf,
}

/// Message reçu ; les champs inconnus sont ignorés.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Incoming {
    Hello {
        #[serde(default)]
        client: String,
        protocol: u32,
        #[serde(default)]
        language: Option<String>,
    },
    Message {
        #[serde(default)]
        id: Option<String>,
        text: String,
        #[serde(default)]
        attachments: Vec<Attachment>,
    },
    Confirm {
        request: String,
        accept: bool,
    },
    Choice {
        request: String,
        value: String,
    },
    Cancel,
    NewConversation,
    Provider {
        value: String,
    },
}

/// Analyse une ligne ; l'erreur est prête à renvoyer dans un `error`.
pub fn parse_incoming(line: &str) -> Result<Incoming, String> {
    serde_json::from_str(line).map_err(|error| format!("message invalide : {error}"))
}

// ---------------------------------------------------------------------------
// Messages LYRA → client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Low,
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChoiceOptionOut {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outgoing {
    Ready {
        protocol: u32,
        provider: Provider,
        /// Fournisseurs utilisables au démarrage de la session.
        providers: Vec<Provider>,
        fs: bool,
    },
    TurnStart {
        id: String,
    },
    Delta {
        text: String,
    },
    Thinking {
        active: bool,
    },
    Progress {
        phase: String,
        fraction: Option<f64>,
    },
    Tool {
        id: String,
        name: String,
        summary: String,
        status: ToolStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<String>,
    },
    ConfirmRequest {
        id: String,
        command: String,
        reason: String,
        risk: Risk,
    },
    ChoiceRequest {
        id: String,
        kind: String,
        prompt: String,
        options: Vec<ChoiceOptionOut>,
    },
    TurnEnd {
        id: String,
        text: String,
    },
    Error {
        message: String,
        fatal: bool,
    },
    /// Information sans erreur (repli de fournisseur, annulation, réglages).
    Notice {
        text: String,
    },
}

/// Une ligne JSON, sans saut de ligne interne (serde échappe les `\n`).
pub fn encode(message: &Outgoing) -> String {
    serde_json::to_string(message).expect("message sérialisable")
}

/// Type d'une proposition, d'après le contenu de ses options.
pub fn choice_kind(request: &ChoiceRequest) -> &'static str {
    let options = &request.options;
    if options.iter().any(|o| o.svg.is_some()) {
        "diagram"
    } else if options.iter().any(|o| o.image_path.is_some()) {
        "image"
    } else if options.iter().any(|o| !o.colors.is_empty()) {
        "color"
    } else {
        "text"
    }
}

fn choice_options(request: &ChoiceRequest) -> Vec<ChoiceOptionOut> {
    request
        .options
        .iter()
        .map(|option| ChoiceOptionOut {
            label: option.label.clone(),
            value: option.id.clone(),
            preview: option
                .svg
                .clone()
                .or_else(|| option.image_path.clone())
                .unwrap_or_else(|| {
                    if option.colors.is_empty() {
                        option.description.clone()
                    } else {
                        option
                            .colors
                            .iter()
                            .map(|c| c.to_uppercase())
                            .collect::<Vec<_>>()
                            .join(" ")
                    }
                }),
        })
        .collect()
}

/// Résultat d'outil : statut et résumé court sur une ligne.
pub fn tool_outcome(output: &str) -> (ToolStatus, String) {
    let value = serde_json::from_str::<Value>(output).ok();
    let failed = value.as_ref().is_some_and(|v| {
        v.get("error").is_some() || v["status"] == "declined" || v["verified"] == false
    }) || output.starts_with("erreur outil")
        || (output.starts_with("outil ") && output.contains("timeout"));
    let text = match &value {
        Some(v) if v.get("error").is_some() => {
            v["error"].as_str().map(str::to_string).unwrap_or_default()
        }
        Some(v) if v["status"] == "declined" => "refusé par l'utilisateur".to_string(),
        _ => output.to_string(),
    };
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut short: String = collapsed.chars().take(RESULT_CHARS).collect();
    if collapsed.chars().count() > RESULT_CHARS {
        short.push('…');
    }
    (
        if failed {
            ToolStatus::Error
        } else {
            ToolStatus::Done
        },
        short,
    )
}

// ---------------------------------------------------------------------------
// Interface Server : traduit les appels de la boucle d'agent en messages
// ---------------------------------------------------------------------------

struct Row {
    kind: String,
    text: String,
}

struct Turn {
    id: String,
    first_row: usize,
}

struct State {
    ready: bool,
    /// Messages émis avant `ready`, envoyés juste après lui.
    queued: Vec<Outgoing>,
    closed: bool,
    fs: bool,
    provider: Provider,
    rows: Vec<Row>,
    turn: Option<Turn>,
    pending_ids: VecDeque<String>,
    next_message: u64,
    choice: Option<(String, ChoiceRequest)>,
    next_choice: u64,
    confirms: HashSet<i32>,
    tool: Option<(String, String, String)>,
    next_tool: u64,
    thinking: bool,
    tr: Tr,
}

pub struct Server {
    out: Mutex<Box<dyn Write + Send>>,
    state: Mutex<State>,
}

impl Server {
    pub fn new(out: Box<dyn Write + Send>, provider: Provider, tr: Tr) -> Self {
        Self {
            out: Mutex::new(out),
            state: Mutex::new(State {
                ready: false,
                queued: Vec::new(),
                closed: false,
                fs: false,
                provider,
                rows: Vec::new(),
                turn: None,
                pending_ids: VecDeque::new(),
                next_message: 0,
                choice: None,
                next_choice: 0,
                confirms: HashSet::new(),
                tool: None,
                next_tool: 0,
                thinking: false,
                tr,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self, message: &Outgoing) {
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        // Client parti : rien à faire de plus, la fin de stdin arrêtera tout.
        let _ = writeln!(out, "{}", encode(message));
        let _ = out.flush();
    }

    /// Émet tout de suite (avant la session : erreurs, accès fichiers).
    pub fn emit_now(&self, message: Outgoing) {
        self.write(&message);
    }

    fn emit(&self, st: &mut State, message: Outgoing) {
        if st.ready {
            self.write(&message);
        } else {
            st.queued.push(message);
        }
    }

    pub fn error(&self, message: impl Into<String>) {
        let mut st = self.lock();
        let message = Outgoing::Error {
            message: message.into(),
            fatal: false,
        };
        self.emit(&mut st, message);
    }

    pub fn set_fs(&self, fs: bool) {
        self.lock().fs = fs;
    }

    pub fn set_tr(&self, tr: Tr) {
        self.lock().tr = tr;
    }

    pub fn set_provider(&self, provider: Provider) {
        self.lock().provider = provider;
    }

    /// Résultat de la vérification des fournisseurs ; la première fois, la
    /// session est prête (`ready`) et les messages en attente partent.
    pub fn providers(&self, ready: bool, available: &[Provider], details: &str) {
        let mut st = self.lock();
        if !st.ready {
            st.ready = true;
            self.write(&Outgoing::Ready {
                protocol: PROTOCOL,
                provider: st.provider,
                providers: available.to_vec(),
                fs: st.fs,
            });
            for message in std::mem::take(&mut st.queued) {
                self.write(&message);
            }
        }
        if !ready {
            self.emit(
                &mut st,
                Outgoing::Error {
                    message: details.to_string(),
                    fatal: false,
                },
            );
        }
    }

    // --- Bulles -----------------------------------------------------------

    fn grow(&self, st: &mut State, idx: usize, kind: &str, text: &str) {
        let old = std::mem::take(&mut st.rows[idx].text);
        st.rows[idx].kind = kind.to_string();
        if kind == "assistant" {
            // Seul l'ajout en fin de texte est transmis ; une réécriture
            // complète arrive de toute façon dans `turn_end`.
            if let Some(delta) = text.strip_prefix(old.as_str())
                && !delta.is_empty()
            {
                self.emit(
                    st,
                    Outgoing::Delta {
                        text: delta.to_string(),
                    },
                );
            }
        }
        st.rows[idx].text = text.to_string();
    }

    pub fn append(&self, kind: &str, text: &str) -> usize {
        let mut st = self.lock();
        let idx = st.rows.len();
        st.rows.push(Row {
            kind: kind.to_string(),
            text: String::new(),
        });
        match kind {
            "error" => {
                st.rows[idx].text = text.to_string();
                self.emit(
                    &mut st,
                    Outgoing::Error {
                        message: text.trim().to_string(),
                        fatal: false,
                    },
                );
            }
            "status" => {
                st.rows[idx].text = text.to_string();
                self.emit(
                    &mut st,
                    Outgoing::Notice {
                        text: text.trim().to_string(),
                    },
                );
            }
            _ => self.grow(&mut st, idx, kind, text),
        }
        idx
    }

    pub fn set_row(&self, idx: usize, kind: &str, text: &str) {
        let mut st = self.lock();
        if idx < st.rows.len() {
            self.grow(&mut st, idx, kind, text);
        }
    }

    pub fn update_last(&self, kind: &str, text: &str) {
        let last = self.lock().rows.len().checked_sub(1);
        match last {
            Some(idx) => self.set_row(idx, kind, text),
            None => {
                self.append(kind, text);
            }
        }
    }

    pub fn remove_last(&self) {
        self.lock().rows.pop();
    }

    pub fn clear(&self) {
        let mut st = self.lock();
        st.rows.clear();
        st.choice = None;
        if let Some(turn) = st.turn.as_mut() {
            turn.first_row = 0;
        }
    }

    // --- Activité -----------------------------------------------------------

    pub fn activity(&self, activity: &str) {
        let mut st = self.lock();
        let thinking = activity == "thinking";
        if st.thinking != thinking {
            st.thinking = thinking;
            self.emit(&mut st, Outgoing::Thinking { active: thinking });
        }
    }

    pub fn progress(&self, phase: &str, fraction: Option<f32>) {
        let mut st = self.lock();
        let fraction = fraction.map(|f| (f64::from(f.clamp(0., 1.)) * 1000.).round() / 1000.);
        self.emit(
            &mut st,
            Outgoing::Progress {
                phase: phase.to_string(),
                fraction,
            },
        );
    }

    // --- Tours --------------------------------------------------------------

    /// Identifiant du prochain tour (message ou choix du client).
    pub fn queue_turn(&self, id: Option<String>) -> String {
        let mut st = self.lock();
        let id = id.filter(|id| !id.trim().is_empty()).unwrap_or_else(|| {
            st.next_message += 1;
            format!("m{}", st.next_message)
        });
        st.pending_ids.push_back(id.clone());
        id
    }

    pub fn turn_start(&self) {
        let mut st = self.lock();
        let id = st.pending_ids.pop_front().unwrap_or_else(|| {
            st.next_message += 1;
            format!("m{}", st.next_message)
        });
        st.turn = Some(Turn {
            id: id.clone(),
            first_row: st.rows.len(),
        });
        self.emit(&mut st, Outgoing::TurnStart { id });
    }

    pub fn turn_end(&self) {
        let mut st = self.lock();
        if st.thinking {
            st.thinking = false;
            self.emit(&mut st, Outgoing::Thinking { active: false });
        }
        let Some(turn) = st.turn.take() else { return };
        let text = st
            .rows
            .iter()
            .skip(turn.first_row)
            .filter(|row| row.kind == "assistant" && !row.text.trim().is_empty())
            .map(|row| row.text.trim())
            .collect::<Vec<_>>()
            .join("\n\n");
        self.emit(&mut st, Outgoing::TurnEnd { id: turn.id, text });
    }

    // --- Outils -------------------------------------------------------------

    pub fn tool_started(&self, name: &str, summary: &str) {
        let mut st = self.lock();
        st.next_tool += 1;
        let id = format!("t{}", st.next_tool);
        st.tool = Some((id.clone(), name.to_string(), summary.to_string()));
        self.emit(
            &mut st,
            Outgoing::Tool {
                id,
                name: name.to_string(),
                summary: summary.to_string(),
                status: ToolStatus::Running,
                result: None,
            },
        );
    }

    pub fn tool_finished(&self, output: &str) {
        let mut st = self.lock();
        let Some((id, name, summary)) = st.tool.take() else {
            return;
        };
        let (status, result) = tool_outcome(output);
        self.emit(
            &mut st,
            Outgoing::Tool {
                id,
                name,
                summary,
                status,
                result: Some(result),
            },
        );
    }

    // --- Confirmations --------------------------------------------------------

    /// Émet `confirm_request` ; faux si plus personne ne peut répondre.
    pub fn confirm_request(&self, id: i32, command: &str, reason: &str, high: bool) -> bool {
        let mut st = self.lock();
        if st.closed {
            return false;
        }
        st.confirms.insert(id);
        self.emit(
            &mut st,
            Outgoing::ConfirmRequest {
                id: format!("c{id}"),
                command: command.to_string(),
                reason: reason.to_string(),
                risk: if high { Risk::High } else { Risk::Low },
            },
        );
        true
    }

    /// Identifiant interne d'une confirmation en attente (`c12` → 12).
    pub fn take_confirm(&self, request: &str) -> Option<i32> {
        let id = request.strip_prefix('c')?.parse::<i32>().ok()?;
        self.lock().confirms.remove(&id).then_some(id)
    }

    /// Fin de stdin : plus de réponse possible. Renvoie les confirmations
    /// en attente, à refuser.
    pub fn close(&self) -> Vec<i32> {
        let mut st = self.lock();
        st.closed = true;
        st.confirms.drain().collect()
    }

    // --- Choix --------------------------------------------------------------

    pub fn offer_choices(&self, request: ChoiceRequest) {
        let mut st = self.lock();
        st.next_choice += 1;
        let id = format!("q{}", st.next_choice);
        let message = Outgoing::ChoiceRequest {
            id: id.clone(),
            kind: choice_kind(&request).to_string(),
            prompt: request.question.clone(),
            options: choice_options(&request),
        };
        st.rows.push(Row {
            kind: "choice".into(),
            text: request.question.clone(),
        });
        st.choice = Some((id, request));
        self.emit(&mut st, message);
    }

    /// Un message libre ferme la proposition en cours (comme la fenêtre).
    pub fn close_choice(&self) {
        self.lock().choice = None;
    }

    /// Choix du client : (texte affiché, contexte pour le modèle).
    pub fn take_choice(&self, request: &str, value: &str) -> Result<(String, String), String> {
        let mut st = self.lock();
        let Some((id, choice)) = st.choice.as_ref() else {
            return Err(format!("aucune proposition en attente pour « {request} »"));
        };
        if id != request {
            return Err(format!("proposition inconnue ou close : « {request} »"));
        }
        let index = choice
            .options
            .iter()
            .position(|o| o.id == value)
            .or_else(|| choice.options.iter().position(|o| o.label == value))
            .ok_or_else(|| format!("valeur inconnue pour « {request} » : « {value} »"))?;
        let selected = choice
            .select(index, &st.tr.choice_selected)
            .ok_or_else(|| format!("valeur inconnue pour « {request} »"))?;
        st.choice = None;
        Ok(selected)
    }
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// Écrit sur stdout ; le verrou n'est pris que pour chaque ligne.
struct Stdout;

impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        std::io::stdout().lock().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stdout().lock().flush()
    }
}

/// Point d'entrée de `hyprpanel-chat serve` ; renvoie le code de sortie.
pub fn run(args: &[String]) -> anyhow::Result<i32> {
    let options = parse_args(args)?;
    let (mut cfg, _) =
        ChatConfig::load_or_init().map_err(|e| anyhow::anyhow!("configuration du chat : {e}"))?;
    if let Some(provider) = options.provider {
        cfg = cfg.with_session_provider(provider);
    }
    let stdin = std::io::stdin();
    serve(cfg, options.filesystem, stdin.lock(), Box::new(Stdout))
}

/// Lignes non vides de l'entrée ; `None` en fin de flux (ou erreur de lecture).
fn next_line(input: &mut impl BufRead) -> Option<String> {
    loop {
        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) | Err(_) => return None,
            Ok(_) if line.trim().is_empty() => continue,
            Ok(_) => return Some(line),
        }
    }
}

/// Session complète sur une entrée et une sortie quelconques.
pub fn serve(
    mut cfg: ChatConfig,
    filesystem_option: Option<bool>,
    mut input: impl BufRead,
    output: Box<dyn Write + Send>,
) -> anyhow::Result<i32> {
    let server = Arc::new(Server::new(
        output,
        cfg.provider,
        crate::i18n::tr(cfg.lang()),
    ));

    // 1. Poignée de main : rien ne démarre avant `hello`.
    loop {
        let Some(line) = next_line(&mut input) else {
            return Ok(0);
        };
        match parse_incoming(&line) {
            Ok(Incoming::Hello {
                protocol, language, ..
            }) => {
                if protocol != PROTOCOL {
                    server.emit_now(Outgoing::Error {
                        message: format!(
                            "protocole {protocol} non pris en charge (version attendue : {PROTOCOL})"
                        ),
                        fatal: true,
                    });
                    return Ok(2);
                }
                if let Some(language) = language {
                    cfg.language = crate::i18n::Lang::parse(&language).as_str().to_string();
                }
                break;
            }
            Ok(_) => server.emit_now(Outgoing::Error {
                message: "hello attendu avant tout autre message".into(),
                fatal: false,
            }),
            Err(message) => server.emit_now(Outgoing::Error {
                message,
                fatal: false,
            }),
        }
    }
    let translations = crate::i18n::tr(cfg.lang());
    server.set_tr(translations.clone());

    // 2. Accès aux fichiers : option, sinon confirmation dédiée.
    let filesystem = crate::build_filesystem(&cfg)?;
    let mut backlog = Vec::new();
    let approved = match filesystem_option {
        Some(approved) => approved,
        None => {
            server.emit_now(Outgoing::ConfirmRequest {
                id: FS_REQUEST.into(),
                command: format!(
                    "{} — {}",
                    translations.fs_title,
                    filesystem.root().display()
                ),
                reason: format!("{}\n{}", translations.fs_body, translations.fs_note),
                risk: Risk::High,
            });
            loop {
                let Some(line) = next_line(&mut input) else {
                    return Ok(0);
                };
                match parse_incoming(&line) {
                    Ok(Incoming::Confirm { request, accept }) if request == FS_REQUEST => {
                        break accept;
                    }
                    Ok(Incoming::Confirm { request, .. }) => server.emit_now(Outgoing::Error {
                        message: format!("demande de confirmation inconnue : « {request} »"),
                        fatal: false,
                    }),
                    // Traités dès que la session est prête.
                    Ok(other) => backlog.push(other),
                    Err(message) => server.emit_now(Outgoing::Error {
                        message,
                        fatal: false,
                    }),
                }
            }
        }
    };
    filesystem.set_approved(approved);
    server.set_fs(approved);

    // 3. Boucle d'agent (comme la fenêtre et le CLI), sur son propre runtime.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<UserEvent>();
    let approvals = Approvals::default();
    let cancel = Cancel::default();
    let agent = {
        let front = Front::Server(server.clone());
        let filesystem = filesystem.clone();
        let approvals = approvals.clone();
        let cancel = cancel.clone();
        let cfg = cfg.clone();
        std::thread::Builder::new()
            .name("lyra-agent".into())
            .spawn(move || {
                match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(crate::agent::run(
                        cfg, rx, front, filesystem, approvals, cancel,
                    )),
                    Err(e) => eprintln!("runtime tokio : {e}"),
                }
            })
            .context("thread agent")?
    };

    let session = Session {
        server: &server,
        tx: &tx,
        approvals: &approvals,
        cancel: &cancel,
        cfg: &cfg,
    };
    for incoming in backlog {
        session.dispatch(incoming);
    }
    while let Some(line) = next_line(&mut input) {
        match parse_incoming(&line) {
            Ok(incoming) => session.dispatch(incoming),
            Err(message) => server.error(message),
        }
    }

    // 4. Fin de stdin : les demandes sans réponse possible sont refusées, les
    // messages déjà reçus sont traités, puis l'agent s'arrête.
    for id in server.close() {
        approvals.answer(id, false);
    }
    drop(tx);
    let _ = agent.join();
    Ok(0)
}

struct Session<'a> {
    server: &'a Server,
    tx: &'a tokio::sync::mpsc::UnboundedSender<UserEvent>,
    approvals: &'a Approvals,
    cancel: &'a Cancel,
    cfg: &'a ChatConfig,
}

impl Session<'_> {
    fn send(&self, event: UserEvent) {
        if self.tx.send(event).is_err() {
            self.server.error("boucle d'agent arrêtée");
        }
    }

    fn dispatch(&self, incoming: Incoming) {
        match incoming {
            Incoming::Hello { .. } => self.server.error("session déjà ouverte : hello ignoré"),
            Incoming::Message {
                id,
                text,
                attachments,
            } => {
                self.server.close_choice();
                self.server.queue_turn(id);
                self.send(UserEvent::Input {
                    text,
                    files: attachments.into_iter().map(|a| a.path).collect(),
                    pasted_images: Vec::new(),
                });
            }
            Incoming::Confirm { request, accept } => match self.server.take_confirm(&request) {
                Some(id) => self.approvals.answer(id, accept),
                None => self.server.error(format!(
                    "demande de confirmation inconnue ou déjà traitée : « {request} »"
                )),
            },
            Incoming::Choice { request, value } => {
                match self.server.take_choice(&request, &value) {
                    Ok((display, context)) => {
                        self.server.queue_turn(Some(request));
                        self.send(UserEvent::Choice { display, context });
                    }
                    Err(message) => self.server.error(message),
                }
            }
            Incoming::Cancel => self.cancel.cancel(),
            Incoming::NewConversation => {
                self.server.close_choice();
                self.send(UserEvent::Reset);
            }
            Incoming::Provider { value } => match Provider::parse(&value) {
                Ok(provider) => {
                    self.server.set_provider(provider);
                    self.send(UserEvent::ConfigUpdated(Box::new(
                        self.cfg.clone().with_session_provider(provider),
                    )));
                }
                Err(message) => self.server.error(message),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::choices::ChoiceOption;
    use crate::i18n::{Lang, tr};

    /// Sortie partagée pour relire les lignes émises.
    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Buffer {
        fn lines(&self) -> Vec<Value> {
            String::from_utf8(self.0.lock().unwrap().clone())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    fn server() -> (Arc<Server>, Buffer) {
        let buffer = Buffer::default();
        let server = Arc::new(Server::new(
            Box::new(buffer.clone()),
            Provider::ClaudeCode,
            tr(Lang::Fr),
        ));
        (server, buffer)
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn option(id: &str) -> ChoiceOption {
        ChoiceOption {
            id: id.into(),
            label: format!("Option {id}"),
            description: String::new(),
            value: format!("valeur-{id}"),
            colors: Vec::new(),
            image_path: None,
            svg: None,
            source_url: String::new(),
        }
    }

    #[test]
    fn arguments_serve() {
        assert_eq!(parse_args(&[]).unwrap(), Options::default());
        let parsed = parse_args(&args(&["--provider", "codex", "--no-fs"])).unwrap();
        assert_eq!(parsed.provider, Some(Provider::Codex));
        assert_eq!(parsed.filesystem, Some(false));
        assert_eq!(
            parse_args(&args(&["--provider=zcode", "--fs"])).unwrap(),
            Options {
                provider: Some(Provider::Zcode),
                filesystem: Some(true)
            }
        );
        assert!(parse_args(&args(&["--provider", "gpt"])).is_err());
        assert!(parse_args(&args(&["--provider"])).is_err());
        assert!(parse_args(&args(&["message"])).is_err());
    }

    #[test]
    fn analyse_de_chaque_message_client() {
        assert_eq!(
            parse_incoming(
                r#"{"type":"hello","client":"hyprdesk","protocol":1,"language":"fr","extra":true}"#
            ),
            Ok(Incoming::Hello {
                client: "hyprdesk".into(),
                protocol: 1,
                language: Some("fr".into())
            })
        );
        assert_eq!(
            parse_incoming(
                r#"{"type":"message","id":"m1","text":"bonjour","attachments":[{"path":"/tmp/a.png","size":3}]}"#
            ),
            Ok(Incoming::Message {
                id: Some("m1".into()),
                text: "bonjour".into(),
                attachments: vec![Attachment {
                    path: "/tmp/a.png".into()
                }],
            })
        );
        assert_eq!(
            parse_incoming(r#"{"type":"message","text":"sans id"}"#),
            Ok(Incoming::Message {
                id: None,
                text: "sans id".into(),
                attachments: Vec::new()
            })
        );
        assert_eq!(
            parse_incoming(r#"{"type":"confirm","request":"c1","accept":true}"#),
            Ok(Incoming::Confirm {
                request: "c1".into(),
                accept: true
            })
        );
        assert_eq!(
            parse_incoming(r#"{"type":"choice","request":"q1","value":"a"}"#),
            Ok(Incoming::Choice {
                request: "q1".into(),
                value: "a".into()
            })
        );
        assert_eq!(parse_incoming(r#"{"type":"cancel"}"#), Ok(Incoming::Cancel));
        assert_eq!(
            parse_incoming(r#"{"type":"new_conversation","reason":"x"}"#),
            Ok(Incoming::NewConversation)
        );
        assert_eq!(
            parse_incoming(r#"{"type":"provider","value":"codex"}"#),
            Ok(Incoming::Provider {
                value: "codex".into()
            })
        );
        for invalid in [
            "pas du json",
            r#"{"type":"inconnu"}"#,
            r#"{"type":"message"}"#,
            r#"{"type":"hello","client":"x"}"#,
            r#"{"type":"confirm","request":"c1","accept":"oui"}"#,
            r#"{"text":"sans type"}"#,
        ] {
            let error = parse_incoming(invalid).unwrap_err();
            assert!(error.starts_with("message invalide"), "{invalid}");
        }
    }

    #[test]
    fn emission_de_chaque_message() {
        let cases = [
            (
                Outgoing::Ready {
                    protocol: 1,
                    provider: Provider::ClaudeCode,
                    providers: vec![Provider::Zcode, Provider::ClaudeCode, Provider::Codex],
                    fs: false,
                },
                r#"{"type":"ready","protocol":1,"provider":"claude_code","providers":["zcode","claude_code","codex"],"fs":false}"#,
            ),
            (
                Outgoing::TurnStart { id: "m1".into() },
                r#"{"type":"turn_start","id":"m1"}"#,
            ),
            (
                Outgoing::Delta {
                    text: "**Bon**jour\n".into(),
                },
                r#"{"type":"delta","text":"**Bon**jour\n"}"#,
            ),
            (
                Outgoing::Thinking { active: true },
                r#"{"type":"thinking","active":true}"#,
            ),
            (
                Outgoing::Progress {
                    phase: "Paquets".into(),
                    fraction: Some(0.4),
                },
                r#"{"type":"progress","phase":"Paquets","fraction":0.4}"#,
            ),
            (
                Outgoing::Progress {
                    phase: "Recherche".into(),
                    fraction: None,
                },
                r#"{"type":"progress","phase":"Recherche","fraction":null}"#,
            ),
            (
                Outgoing::Tool {
                    id: "t1".into(),
                    name: "lyra_read_file".into(),
                    summary: "path=a".into(),
                    status: ToolStatus::Running,
                    result: None,
                },
                r#"{"type":"tool","id":"t1","name":"lyra_read_file","summary":"path=a","status":"running"}"#,
            ),
            (
                Outgoing::Tool {
                    id: "t1".into(),
                    name: "lyra_read_file".into(),
                    summary: "path=a".into(),
                    status: ToolStatus::Done,
                    result: Some("ok".into()),
                },
                r#"{"type":"tool","id":"t1","name":"lyra_read_file","summary":"path=a","status":"done","result":"ok"}"#,
            ),
            (
                Outgoing::ConfirmRequest {
                    id: "c1".into(),
                    command: "rm a".into(),
                    reason: "nettoyer".into(),
                    risk: Risk::High,
                },
                r#"{"type":"confirm_request","id":"c1","command":"rm a","reason":"nettoyer","risk":"high"}"#,
            ),
            (
                Outgoing::ChoiceRequest {
                    id: "q1".into(),
                    kind: "color".into(),
                    prompt: "Palette ?".into(),
                    options: vec![ChoiceOptionOut {
                        label: "Nuit".into(),
                        value: "a".into(),
                        preview: "#000000".into(),
                    }],
                },
                r##"{"type":"choice_request","id":"q1","kind":"color","prompt":"Palette ?","options":[{"label":"Nuit","value":"a","preview":"#000000"}]}"##,
            ),
            (
                Outgoing::TurnEnd {
                    id: "m1".into(),
                    text: "Fini".into(),
                },
                r#"{"type":"turn_end","id":"m1","text":"Fini"}"#,
            ),
            (
                Outgoing::Error {
                    message: "x".into(),
                    fatal: false,
                },
                r#"{"type":"error","message":"x","fatal":false}"#,
            ),
            (
                Outgoing::Notice { text: "y".into() },
                r#"{"type":"notice","text":"y"}"#,
            ),
        ];
        for (message, expected) in cases {
            let line = encode(&message);
            assert_eq!(line, expected);
            assert!(!line.contains('\n'));
        }
    }

    #[test]
    fn messages_avant_ready_differes_puis_tour_complet() {
        let (server, buffer) = server();
        server.set_fs(true);
        // Erreur du service avant la fin de l'initialisation : différée.
        server.append("error", "Service injoignable");
        assert!(buffer.lines().is_empty());
        server.providers(true, &[Provider::ClaudeCode], "");
        server.queue_turn(Some("m1".into()));
        server.turn_start();
        server.activity("thinking");
        let row = server.append("assistant", "Bon");
        server.set_row(row, "assistant", "Bonjour");
        server.set_row(row, "assistant", "Bonjour !");
        server.activity("writing");
        server.append("status", "Réglages enregistrés");
        server.turn_end();
        let lines = buffer.lines();
        let types: Vec<&str> = lines.iter().map(|l| l["type"].as_str().unwrap()).collect();
        assert_eq!(
            types,
            [
                "ready",
                "error",
                "turn_start",
                "thinking",
                "delta",
                "delta",
                "delta",
                "thinking",
                "notice",
                "turn_end"
            ]
        );
        assert_eq!(lines[0]["fs"], true);
        assert_eq!(lines[0]["providers"], serde_json::json!(["claude_code"]));
        assert_eq!(lines[2]["id"], "m1");
        let streamed: String = lines
            .iter()
            .filter(|l| l["type"] == "delta")
            .map(|l| l["text"].as_str().unwrap())
            .collect();
        assert_eq!(streamed, "Bonjour !");
        assert_eq!(lines[9]["text"], "Bonjour !");
        assert_eq!(lines[9]["id"], "m1");
    }

    #[test]
    fn aucun_fournisseur_signale_apres_ready() {
        let (server, buffer) = server();
        server.providers(false, &[], "Aucun fournisseur disponible");
        let lines = buffer.lines();
        assert_eq!(lines[0]["type"], "ready");
        assert_eq!(lines[0]["providers"], serde_json::json!([]));
        assert_eq!(lines[1]["type"], "error");
        assert_eq!(lines[1]["fatal"], false);
    }

    #[test]
    fn outils_et_resultats_resumes() {
        let (server, buffer) = server();
        server.providers(true, &[Provider::ClaudeCode], "");
        server.tool_started("lyra_run_command", "command=ls");
        server.tool_finished(r#"{"status":"ok","stdout":"a\nb"}"#);
        server.tool_started("lyra_run_command", "command=rm a");
        server.tool_finished(r#"{"status":"declined","instruction":"…"}"#);
        let lines = buffer.lines();
        assert_eq!(lines[1]["status"], "running");
        assert_eq!(lines[2]["status"], "done");
        assert_eq!(lines[2]["id"], lines[1]["id"]);
        assert_eq!(lines[4]["status"], "error");
        assert_eq!(lines[4]["result"], "refusé par l'utilisateur");
        assert_ne!(lines[3]["id"], lines[1]["id"]);
        let (status, short) = tool_outcome(&"x ".repeat(500));
        assert_eq!(status, ToolStatus::Done);
        assert!(short.chars().count() <= RESULT_CHARS + 1);
        assert_eq!(
            tool_outcome(r#"{"error":"Permission revoked"}"#),
            (ToolStatus::Error, "Permission revoked".into())
        );
    }

    #[test]
    fn choix_proposes_puis_selectionnes() {
        let (server, buffer) = server();
        server.providers(true, &[Provider::ClaudeCode], "");
        let mut color = option("b");
        color.colors = vec!["#7bdacd".into()];
        server.offer_choices(ChoiceRequest {
            question: "Quelle palette ?".into(),
            options: vec![option("a"), color],
        });
        let lines = buffer.lines();
        assert_eq!(lines[1]["type"], "choice_request");
        assert_eq!(lines[1]["kind"], "color");
        assert_eq!(lines[1]["options"][1]["preview"], "#7BDACD");
        assert_eq!(lines[1]["options"][0]["value"], "a");
        let id = lines[1]["id"].as_str().unwrap().to_string();
        assert!(server.take_choice("q99", "a").is_err());
        assert!(server.take_choice(&id, "zz").is_err());
        let (display, context) = server.take_choice(&id, "b").unwrap();
        assert!(display.contains("Option b"));
        assert!(context.contains("Quelle palette ?"));
        // Une seule réponse par proposition.
        assert!(server.take_choice(&id, "a").is_err());

        let mut diagram = option("d");
        diagram.svg = Some("<svg/>".into());
        assert_eq!(
            choice_kind(&ChoiceRequest {
                question: String::new(),
                options: vec![option("a"), diagram]
            }),
            "diagram"
        );
    }

    #[tokio::test]
    async fn confirmation_acceptee_et_refusee() {
        let (server, buffer) = server();
        server.providers(true, &[Provider::ClaudeCode], "");
        let approvals = Approvals::default();
        let front = Front::Server(server.clone());
        for accept in [true, false] {
            let task = {
                let approvals = approvals.clone();
                let front = front.clone();
                tokio::spawn(async move {
                    approvals
                        .request_with_risk(&front, "rm a".into(), "test".into(), true)
                        .await
                })
            };
            // La demande reste en attente tant qu'aucune réponse n'arrive.
            let request = loop {
                if let Some(line) = buffer
                    .lines()
                    .into_iter()
                    .rev()
                    .find(|l| l["type"] == "confirm_request")
                    && server.lock().confirms.len() == 1
                {
                    break line;
                }
                tokio::task::yield_now().await;
            };
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            assert!(!task.is_finished(), "jamais acceptée par défaut");
            assert_eq!(request["risk"], "high");
            let id = server
                .take_confirm(request["id"].as_str().unwrap())
                .unwrap();
            approvals.answer(id, accept);
            assert_eq!(task.await.unwrap(), accept);
            // Une réponse en double est refusée.
            assert!(
                server
                    .take_confirm(request["id"].as_str().unwrap())
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn fin_de_stdin_refuse_les_confirmations_sans_reponse() {
        let (server, _buffer) = server();
        server.providers(true, &[Provider::ClaudeCode], "");
        let approvals = Approvals::default();
        let front = Front::Server(server.clone());
        let task = {
            let approvals = approvals.clone();
            let front = front.clone();
            tokio::spawn(async move {
                approvals
                    .request_with_risk(&front, "ls".into(), "test".into(), false)
                    .await
            })
        };
        while server.lock().confirms.is_empty() {
            tokio::task::yield_now().await;
        }
        for id in server.close() {
            approvals.answer(id, false);
        }
        assert!(!task.await.unwrap());
        // Après la fermeture, une nouvelle demande est refusée d'office.
        assert!(
            !approvals
                .request_with_risk(&front, "ls".into(), "test".into(), false)
                .await
        );
    }

    #[test]
    fn session_sans_hello_ou_protocole_inconnu() {
        let cfg = ChatConfig::defaults();
        let buffer = Buffer::default();
        let input = std::io::Cursor::new(
            "{\"type\":\"message\",\"text\":\"x\"}\n\n{\"type\":\"hello\",\"protocol\":2}\n",
        );
        let code = serve(cfg.clone(), Some(false), input, Box::new(buffer.clone())).unwrap();
        assert_eq!(code, 2);
        let lines = buffer.lines();
        assert_eq!(lines[0]["type"], "error");
        assert_eq!(lines[0]["fatal"], false);
        assert_eq!(lines[1]["fatal"], true);

        // Fin de flux avant hello : sortie propre, rien d'autre.
        let buffer = Buffer::default();
        let code = serve(
            cfg,
            Some(false),
            std::io::Cursor::new(""),
            Box::new(buffer.clone()),
        )
        .unwrap();
        assert_eq!(code, 0);
        assert!(buffer.lines().is_empty());
    }
}
