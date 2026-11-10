//! LYRA en terminal (`hyprpanel-chat cli`) : même boucle d'agent que la
//! fenêtre (MCP, sauvegardes par tour, mémoire, modes d'exécution), rendue en
//! texte — réponse en flux, appels d'outils résumés, choix numérotés,
//! confirmations o/N lues sur stdin.
//!
//! Sortie : réponses et outils sur stdout, erreurs sur stderr, statut
//! éphémère sur stderr quand c'est un terminal. Sans terminal en entrée
//! (pipe), toute confirmation est refusée.

use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, bail};

use crate::agent::{self, TurnRecord, UserEvent};
use crate::chat_config::ChatConfig;
use crate::choices::ChoiceRequest;
use crate::i18n::Tr;
use crate::ui::Front;

/// Longueur affichée d'un résultat d'outil hors `--verbose`.
const TOOL_RESULT_CHARS: usize = 160;
const HISTORY_SHOWN: usize = 10;

pub const USAGE: &str = "\
  hyprpanel-chat cli [OPTIONS] [MESSAGE…]
                           LYRA dans le terminal. Sans MESSAGE : session
                           interactive (/help). Avec MESSAGE, ou un message
                           sur stdin (pipe) : un seul tour, puis sortie
                           (code 1 si une erreur a été affichée).
      --fs | --no-fs       Accorde / refuse l'accès fichiers sans le demander
                           (défaut : demandé si stdin est un terminal, sinon refusé)
  -f, --file CHEMIN        Joint un fichier au premier message (répétable)
      --provider <zcode|claude_code|codex>
                           Fournisseur pour cette session (chat.toml inchangé)
  -v, --verbose            Résultats d'outils complets
      --color | --no-color Force / désactive les couleurs (NO_COLOR respecté)
";

// ---------------------------------------------------------------------------
// Arguments et saisie (purs, testés)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq)]
pub struct Options {
    pub filesystem: Option<bool>,
    pub verbose: bool,
    pub color: Option<bool>,
    pub files: Vec<PathBuf>,
    pub provider: Option<crate::chat_config::Provider>,
    pub message: Option<String>,
}

pub fn parse_args(args: &[String]) -> anyhow::Result<Options> {
    let mut options = Options::default();
    let mut words = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--fs" => options.filesystem = Some(true),
            "--no-fs" => options.filesystem = Some(false),
            "-v" | "--verbose" => options.verbose = true,
            "--color" => options.color = Some(true),
            "--no-color" => options.color = Some(false),
            "-f" | "--file" => {
                let path = iter.next().context("--file attend un chemin")?;
                options.files.push(expand_home(path));
            }
            "--provider" => {
                let value = iter
                    .next()
                    .context("--provider attend une valeur : zcode, claude_code ou codex")?;
                options.provider =
                    Some(crate::chat_config::Provider::parse(value).map_err(anyhow::Error::msg)?);
            }
            flag if flag.starts_with("--provider=") => {
                options.provider = Some(
                    crate::chat_config::Provider::parse(&flag["--provider=".len()..])
                        .map_err(anyhow::Error::msg)?,
                );
            }
            "--" => {
                words.extend(iter.by_ref().cloned());
            }
            flag if flag.starts_with('-') && flag.len() > 1 => {
                bail!("option inconnue : {flag}")
            }
            word => words.push(word.to_string()),
        }
    }
    let message = words.join(" ");
    if !message.trim().is_empty() {
        options.message = Some(message.trim().to_string());
    }
    Ok(options)
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(rest))
            .unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
    }
}

#[derive(Debug, PartialEq)]
enum Input {
    Help,
    New,
    Attach(Option<String>),
    Detach,
    History,
    Restore(Option<String>),
    Refresh,
    Quit,
    Unknown(String),
    Message(String),
}

fn parse_input(line: &str) -> Input {
    let line = line.trim();
    // « //texte » envoie un message qui commence par « / ».
    if let Some(literal) = line.strip_prefix("//") {
        return Input::Message(format!("/{literal}"));
    }
    let Some(command) = line.strip_prefix('/') else {
        return Input::Message(line.to_string());
    };
    let (name, rest) = command
        .split_once(char::is_whitespace)
        .map(|(n, r)| (n, r.trim()))
        .unwrap_or((command, ""));
    let argument = (!rest.is_empty()).then(|| rest.to_string());
    match name {
        "help" | "h" | "?" => Input::Help,
        "new" | "reset" => Input::New,
        "attach" | "a" => Input::Attach(argument),
        "detach" => Input::Detach,
        "history" => Input::History,
        "restore" => Input::Restore(argument),
        "refresh" => Input::Refresh,
        "quit" | "exit" | "q" => Input::Quit,
        other => Input::Unknown(format!("/{other}")),
    }
}

fn is_yes(answer: &str) -> bool {
    matches!(
        answer.trim().to_lowercase().as_str(),
        "o" | "oui" | "y" | "yes"
    )
}

/// Résumé d'un incrément de la bulle outil : appels « → nom(args) » gardés,
/// résultats repliés sur une ligne (complets avec `verbose`).
fn format_tool_delta(delta: &str, verbose: bool) -> String {
    fn flush(result: &mut Vec<&str>, out: &mut String, verbose: bool) {
        if result.is_empty() {
            return;
        }
        if verbose {
            for line in result.iter() {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        } else {
            let joined = result
                .iter()
                .flat_map(|line| line.split_whitespace())
                .collect::<Vec<_>>()
                .join(" ");
            let mut short: String = joined.chars().take(TOOL_RESULT_CHARS).collect();
            if joined.chars().count() > TOOL_RESULT_CHARS {
                short.push('…');
            }
            out.push_str("    ⤷ ");
            out.push_str(&short);
            out.push('\n');
        }
        result.clear();
    }
    let mut out = String::new();
    let mut result = Vec::new();
    for line in delta.lines() {
        if line.starts_with("→ ") {
            flush(&mut result, &mut out, verbose);
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        } else if !line.trim().is_empty() {
            result.push(line);
        }
    }
    flush(&mut result, &mut out, verbose);
    out
}

// ---------------------------------------------------------------------------
// Rendu
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Style {
    color: bool,
    /// Ligne de statut éphémère (stderr est un terminal).
    status: bool,
}

impl Style {
    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn swatch(&self, hex: &str) -> String {
        let rgb = hex
            .strip_prefix('#')
            .and_then(|h| u32::from_str_radix(h, 16).ok());
        match rgb {
            Some(rgb) if self.color => format!(
                "\x1b[48;2;{};{};{}m  \x1b[0m {hex}",
                rgb >> 16,
                (rgb >> 8) & 0xff,
                rgb & 0xff
            ),
            _ => hex.to_string(),
        }
    }
}

struct Row {
    kind: String,
    text: String,
}

struct State {
    rows: Vec<Row>,
    /// Ligne dont le texte termine la sortie : un incrément s'y ajoute.
    cursor: Option<usize>,
    at_line_start: bool,
    /// Statut éphémère affiché sur la ligne courante (stderr).
    transient: bool,
    last_status: String,
    choice: Option<ChoiceRequest>,
    history: Vec<TurnRecord>,
    tr: Tr,
}

pub struct Terminal {
    state: Mutex<State>,
    style: Style,
    verbose: bool,
    errors: AtomicUsize,
}

impl Terminal {
    fn new(style: Style, verbose: bool, tr: Tr) -> Self {
        Self {
            state: Mutex::new(State {
                rows: Vec::new(),
                cursor: None,
                at_line_start: true,
                transient: false,
                last_status: String::new(),
                choice: None,
                history: Vec::new(),
                tr,
            }),
            style,
            verbose,
            errors: AtomicUsize::new(0),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn tr(&self) -> Tr {
        self.lock().tr.clone()
    }

    fn clear_transient(&self, st: &mut State) {
        if st.transient {
            eprint!("\r\x1b[2K");
            let _ = std::io::stderr().flush();
            st.transient = false;
            st.last_status.clear();
        }
    }

    /// Nouveau bloc : efface le statut, revient en début de ligne.
    fn start_block(&self, st: &mut State, blank_line: bool) {
        self.clear_transient(st);
        let mut out = std::io::stdout();
        if !st.at_line_start {
            let _ = writeln!(out);
        }
        if blank_line {
            let _ = writeln!(out);
        }
        let _ = out.flush();
        st.at_line_start = true;
        st.cursor = None;
    }

    fn emit(&self, st: &mut State, idx: usize, kind: &str, delta: &str, continuing: bool) {
        let style = self.style;
        match kind {
            "assistant" => {
                if continuing {
                    self.clear_transient(st);
                } else {
                    self.start_block(st, true);
                    print!("{} ", style.paint("1;35", "LYRA ›"));
                }
                print!("{delta}");
                st.at_line_start = delta.ends_with('\n');
            }
            "tool" => {
                let text = format_tool_delta(delta, self.verbose);
                if text.is_empty() {
                    return;
                }
                if continuing {
                    self.clear_transient(st);
                } else {
                    self.start_block(st, false);
                }
                print!("{}", style.paint("2", &text));
                st.at_line_start = true;
            }
            "user" => {
                // La saisie est déjà à l'écran : seules les pièces jointes s'affichent.
                let attached: Vec<&str> = delta.lines().filter(|l| l.starts_with('📎')).collect();
                if attached.is_empty() {
                    return;
                }
                self.start_block(st, false);
                for line in attached {
                    println!("{}", style.paint("2", line));
                }
            }
            "error" => {
                self.start_block(st, false);
                self.errors.fetch_add(1, Ordering::Relaxed);
                eprintln!(
                    "{}",
                    style.paint("1;31", &format!("✗ {}", delta.trim_end()))
                );
                st.cursor = None;
                return;
            }
            "status" => {
                self.start_block(st, false);
                println!("{}", style.paint("2", &format!("· {}", delta.trim_end())));
            }
            _ => {
                self.start_block(st, false);
                println!("{}", delta.trim_end());
            }
        }
        let _ = std::io::stdout().flush();
        st.cursor = Some(idx);
    }

    fn grow(&self, st: &mut State, idx: usize, kind: &str, text: &str) {
        let old = std::mem::take(&mut st.rows[idx].text);
        st.rows[idx].kind = kind.to_string();
        let (delta, prefix) = match text.strip_prefix(old.as_str()) {
            Some(delta) => (delta, true),
            None => (text, false),
        };
        if !delta.is_empty() {
            let continuing = prefix && !old.is_empty() && st.cursor == Some(idx);
            self.emit(st, idx, kind, delta, continuing);
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
        self.grow(&mut st, idx, kind, text);
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
        let mut st = self.lock();
        if st.rows.pop().is_some() && st.cursor == Some(st.rows.len()) {
            st.cursor = None;
        }
    }

    pub fn clear(&self) {
        let mut st = self.lock();
        st.rows.clear();
        st.choice = None;
        self.start_block(&mut st, true);
        println!("{}", self.style.paint("2", &"─".repeat(40)));
    }

    pub fn offer_choices(&self, request: ChoiceRequest) {
        let mut st = self.lock();
        let style = self.style;
        self.start_block(&mut st, true);
        println!("{}", style.paint("1", &request.question));
        for (i, option) in request.options.iter().enumerate() {
            let mut line = format!("  {}. {}", i + 1, style.paint("1;36", &option.label));
            for hex in &option.colors {
                line.push_str("  ");
                line.push_str(&style.swatch(&hex.to_uppercase()));
            }
            println!("{line}");
            if !option.description.is_empty() {
                println!("     {}", style.paint("2", &option.description));
            }
            if let Some(path) = &option.image_path {
                println!("     {}", style.paint("2", &format!("🖼 {path}")));
            } else if option.svg.is_some() {
                println!("     {}", style.paint("2", "🖼 SVG"));
            }
            if !option.source_url.is_empty() {
                println!("     {}", style.paint("2;4", &option.source_url));
            }
        }
        println!("{}", style.paint("2", &st.tr.cli_choice_hint));
        let _ = std::io::stdout().flush();
        st.rows.push(Row {
            kind: "choice".into(),
            text: request.question.clone(),
        });
        st.choice = Some(request);
    }

    pub fn set_status(&self, status: &str) {
        let mut st = self.lock();
        if !self.style.status || !st.at_line_start || status.trim().is_empty() {
            return;
        }
        if st.transient && st.last_status == status {
            return;
        }
        eprint!(
            "\r\x1b[2K{}",
            self.style.paint("2", &format!("… {}", status.trim()))
        );
        let _ = std::io::stderr().flush();
        st.transient = true;
        st.last_status = status.to_string();
    }

    pub fn history_set(&self, records: &[TurnRecord]) {
        self.lock().history = records.to_vec();
    }

    pub fn set_tr(&self, tr: Tr) {
        self.lock().tr = tr;
    }

    /// Question o/N bloquante sur stdin ; refus sans terminal ou en fin de flux.
    fn ask(&self, st: &mut State, question: &str) -> bool {
        if !std::io::stdin().is_terminal() {
            println!("{}", self.style.paint("2", &st.tr.cli_noninteractive));
            return false;
        }
        print!(
            "{} {} ",
            self.style.paint("1;33", question),
            st.tr.cli_yes_no
        );
        let _ = std::io::stdout().flush();
        st.at_line_start = false;
        let mut answer = String::new();
        let read = std::io::stdin().lock().read_line(&mut answer);
        st.at_line_start = true;
        matches!(read, Ok(n) if n > 0) && is_yes(&answer)
    }

    /// Confirmation d'une action (contenu exact affiché) — appelée hors runtime.
    pub fn confirm(&self, details: &str, reason: &str) -> bool {
        let mut st = self.lock();
        let style = self.style;
        self.start_block(&mut st, true);
        println!(
            "{}",
            style.paint("1;33", &format!("⚠ {}", st.tr.command_title))
        );
        println!("{}", reason.trim());
        for line in details.lines() {
            println!("  {} {line}", style.paint("33", "│"));
        }
        println!("{}", style.paint("2", &st.tr.command_note));
        let question = st.tr.cli_command_prompt.clone();
        self.ask(&mut st, &question)
    }

    fn ask_filesystem(&self, root: &std::path::Path) -> bool {
        let mut st = self.lock();
        let style = self.style;
        println!("{}", st.tr.fs_body);
        println!(
            "{}",
            style.paint("2", &format!("{} — {}", root.display(), st.tr.fs_note))
        );
        let question = st.tr.fs_title.clone();
        let approved = self.ask(&mut st, &question);
        println!(
            "{}",
            style.paint(
                "2",
                if approved {
                    &st.tr.fs_granted
                } else {
                    &st.tr.fs_denied
                }
            )
        );
        approved
    }

    /// Un numéro valide sélectionne la proposition ; toute autre saisie la
    /// ferme et part comme réponse libre (comme dans la fenêtre).
    fn take_choice(&self, input: &str) -> Option<(String, String)> {
        let mut st = self.lock();
        let request = st.choice.take()?;
        let index = input.trim().parse::<usize>().ok()?.checked_sub(1)?;
        request.select(index, &st.tr.choice_selected)
    }

    fn begin_prompt(&self) {
        let mut st = self.lock();
        self.start_block(&mut st, true);
        print!("{} ", self.style.paint("1;36", "›"));
        let _ = std::io::stdout().flush();
        st.at_line_start = false;
    }

    fn end_prompt(&self) {
        let mut st = self.lock();
        st.at_line_start = true;
        st.cursor = None;
    }

    fn note(&self, text: &str) {
        let mut st = self.lock();
        self.start_block(&mut st, false);
        println!("{}", self.style.paint("2", text));
    }

    fn print_history(&self) {
        let st = self.lock();
        if st.history.is_empty() {
            println!("{}", self.style.paint("2", &st.tr.cli_history_empty));
            return;
        }
        let skip = st.history.len().saturating_sub(HISTORY_SHOWN);
        for record in st.history.iter().skip(skip).rev() {
            println!(
                "  n°{:<4} {}  {}  {}",
                record.turn,
                self.style.paint("2", &record.timestamp),
                self.style
                    .paint("36", record.backup.as_deref().unwrap_or("—")),
                record.preview
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// Attend que la boucle d'agent ait traité tous les événements envoyés.
fn sync(tx: &tokio::sync::mpsc::UnboundedSender<UserEvent>) -> anyhow::Result<()> {
    let (done, wait) = tokio::sync::oneshot::channel();
    tx.send(UserEvent::Sync(done))
        .ok()
        .context("boucle d'agent arrêtée")?;
    wait.blocking_recv()
        .ok()
        .context("boucle d'agent interrompue")
}

fn attachment(path: PathBuf, terminal: &Terminal) -> Option<PathBuf> {
    let tr = terminal.tr();
    if !path.is_file() {
        terminal.append(
            "error",
            &tr.cli_attach_missing
                .replace("{}", &path.display().to_string()),
        );
        return None;
    }
    let label = format!("{} ({})", path.display(), agent::attachment_meta(&path));
    terminal.note(&tr.cli_attached.replace("{}", &label));
    Some(path)
}

/// Point d'entrée de `hyprpanel-chat cli` ; renvoie le code de sortie.
pub fn run(args: &[String]) -> anyhow::Result<i32> {
    let options = parse_args(args)?;
    let (mut cfg, _) =
        ChatConfig::load_or_init().map_err(|e| anyhow::anyhow!("configuration du chat : {e}"))?;
    if let Some(provider) = options.provider {
        cfg = cfg.with_session_provider(provider);
    }
    let filesystem = crate::build_filesystem(&cfg)?;
    let stdin_is_terminal = std::io::stdin().is_terminal();

    let message = match options.message {
        Some(message) => Some(message),
        None if !stdin_is_terminal => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .context("lecture de stdin")?;
            let text = text.trim().to_string();
            if text.is_empty() {
                bail!("aucun message sur stdin");
            }
            Some(text)
        }
        None => None,
    };

    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let style = Style {
        color: options
            .color
            .unwrap_or(!no_color && std::io::stdout().is_terminal()),
        status: std::io::stderr().is_terminal(),
    };
    let translations = crate::i18n::tr(cfg.lang());
    let terminal = Arc::new(Terminal::new(style, options.verbose, translations.clone()));

    if message.is_none() {
        // Modèle vide = choisi par le client officiel de l'abonnement.
        let model = match cfg.model_label() {
            label if label.trim().is_empty() => cfg.provider.label().to_string(),
            label => format!("{} · {label}", cfg.provider.label()),
        };
        println!(
            "{}",
            style.paint(
                "1;35",
                &translations
                    .cli_banner
                    .replacen("{}", env!("CARGO_PKG_VERSION"), 1)
                    .replacen("{}", &model, 1)
            )
        );
    }
    let approved = match options.filesystem {
        Some(approved) => approved,
        None if stdin_is_terminal => terminal.ask_filesystem(filesystem.root()),
        None => false,
    };
    filesystem.set_approved(approved);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<UserEvent>();
    {
        let front = Front::Terminal(terminal.clone());
        let filesystem = filesystem.clone();
        let approvals = crate::commands::Approvals::default();
        std::thread::Builder::new()
            .name("lyra-agent".into())
            .spawn(move || {
                match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => {
                        runtime.block_on(agent::run(cfg, rx, front, filesystem, approvals))
                    }
                    Err(e) => eprintln!("runtime tokio : {e}"),
                }
            })
            .context("thread agent")?;
    }
    // Connexion MCP et profil d'environnement avant la première saisie.
    sync(&tx)?;

    let mut pending: Vec<PathBuf> = options
        .files
        .into_iter()
        .filter_map(|path| attachment(path, &terminal))
        .collect();

    if let Some(text) = message {
        tx.send(UserEvent::Input {
            text,
            files: pending,
            pasted_images: Vec::new(),
        })
        .ok()
        .context("boucle d'agent arrêtée")?;
        sync(&tx)?;
        let mut st = terminal.lock();
        terminal.start_block(&mut st, false);
        return Ok(if terminal.errors.load(Ordering::Relaxed) > 0 {
            1
        } else {
            0
        });
    }

    let stdin = std::io::stdin();
    loop {
        terminal.begin_prompt();
        let mut line = String::new();
        let read = stdin
            .lock()
            .read_line(&mut line)
            .context("lecture de stdin")?;
        terminal.end_prompt();
        if read == 0 {
            println!();
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let tr = terminal.tr();
        let event = match parse_input(&line) {
            Input::Help => {
                println!("{}", tr.cli_help);
                continue;
            }
            Input::Quit => break,
            Input::Unknown(name) => {
                terminal.note(&tr.cli_unknown_command.replace("{}", &name));
                continue;
            }
            Input::Attach(None) => {
                if pending.is_empty() {
                    terminal.note(&tr.cli_attachments_none);
                }
                for path in &pending {
                    terminal.note(&format!(
                        "📎 {} ({})",
                        path.display(),
                        agent::attachment_meta(path)
                    ));
                }
                continue;
            }
            Input::Attach(Some(path)) => {
                if let Some(path) = attachment(expand_home(&path), &terminal) {
                    pending.push(path);
                }
                continue;
            }
            Input::Detach => {
                pending.clear();
                terminal.note(&tr.cli_attachments_cleared);
                continue;
            }
            Input::History => {
                terminal.print_history();
                continue;
            }
            Input::Restore(None) => {
                terminal.note(&tr.cli_restore_usage);
                continue;
            }
            Input::Restore(Some(name)) => UserEvent::Restore(name),
            Input::Refresh => UserEvent::EnvironmentRefresh,
            Input::New => {
                pending.clear();
                UserEvent::Reset
            }
            Input::Message(text) => match terminal.take_choice(&text) {
                Some((display, context)) => {
                    terminal.note(&display);
                    UserEvent::Choice { display, context }
                }
                None => UserEvent::Input {
                    text,
                    files: std::mem::take(&mut pending),
                    pasted_images: Vec::new(),
                },
            },
        };
        tx.send(event).ok().context("boucle d'agent arrêtée")?;
        sync(&tx)?;
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn arguments_options_et_message() {
        let parsed = parse_args(&args(&[
            "--fs",
            "-v",
            "-f",
            "/tmp/a.png",
            "règle",
            "les",
            "gaps",
        ]))
        .unwrap();
        assert_eq!(parsed.filesystem, Some(true));
        assert!(parsed.verbose);
        assert_eq!(parsed.files, vec![PathBuf::from("/tmp/a.png")]);
        assert_eq!(parsed.message.as_deref(), Some("règle les gaps"));

        let parsed = parse_args(&args(&["--no-fs", "--no-color", "--", "-v", "littéral"])).unwrap();
        assert_eq!(parsed.filesystem, Some(false));
        assert_eq!(parsed.color, Some(false));
        assert!(!parsed.verbose);
        assert_eq!(parsed.message.as_deref(), Some("-v littéral"));

        assert_eq!(parse_args(&[]).unwrap(), Options::default());
        assert!(parse_args(&args(&["--inconnue"])).is_err());
        assert!(parse_args(&args(&["--file"])).is_err());
    }

    #[test]
    fn arguments_provider() {
        use crate::chat_config::Provider;
        let parsed = parse_args(&args(&["--provider", "codex", "bonjour"])).unwrap();
        assert_eq!(parsed.provider, Some(Provider::Codex));
        assert_eq!(parsed.message.as_deref(), Some("bonjour"));
        let parsed = parse_args(&args(&["--provider=claude_code"])).unwrap();
        assert_eq!(parsed.provider, Some(Provider::ClaudeCode));
        assert_eq!(parsed.message, None);
        let error = parse_args(&args(&["--provider", "gpt"])).unwrap_err();
        assert!(error.to_string().contains("gpt"));
        assert!(parse_args(&args(&["--provider"])).is_err());
        // Après « -- », --provider fait partie du message.
        let parsed = parse_args(&args(&["--", "--provider", "codex"])).unwrap();
        assert_eq!(parsed.provider, None);
        assert_eq!(parsed.message.as_deref(), Some("--provider codex"));
    }

    #[test]
    fn saisie_commandes_et_messages() {
        assert_eq!(parse_input("/help"), Input::Help);
        assert_eq!(parse_input("  /new "), Input::New);
        assert_eq!(
            parse_input("/attach  ~/img.png"),
            Input::Attach(Some("~/img.png".into()))
        );
        assert_eq!(parse_input("/attach"), Input::Attach(None));
        assert_eq!(
            parse_input("/restore lyra-turn-003"),
            Input::Restore(Some("lyra-turn-003".into()))
        );
        assert_eq!(parse_input("/q"), Input::Quit);
        assert_eq!(parse_input("/foo bar"), Input::Unknown("/foo".into()));
        assert_eq!(parse_input("bonjour"), Input::Message("bonjour".into()));
        assert_eq!(
            parse_input("//etc/hosts ?"),
            Input::Message("/etc/hosts ?".into())
        );
    }

    #[test]
    fn reponses_oui_non() {
        for yes in ["o", "Oui", " y\n", "YES"] {
            assert!(is_yes(yes), "{yes}");
        }
        for no in ["", "n", "non", "no", "ok", "oo"] {
            assert!(!is_yes(no), "{no}");
        }
    }

    #[test]
    fn resume_des_outils() {
        let long = "x".repeat(400);
        let delta = format!("→ hypr_status({{}})\n{long}\nfin\n\n→ hypr_reload({{}})\nok");
        let short = format_tool_delta(&delta, false);
        let lines: Vec<&str> = short.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "  → hypr_status({})");
        assert!(lines[1].starts_with("    ⤷ xxx") && lines[1].ends_with('…'));
        assert_eq!(
            lines[1].chars().count(),
            "    ⤷ ".chars().count() + TOOL_RESULT_CHARS + 1
        );
        assert_eq!(lines[3], "    ⤷ ok");

        let full = format_tool_delta(&delta, true);
        assert!(full.contains(&long) && full.contains("    fin\n"));
        // incrément ne contenant que le résultat
        assert_eq!(
            format_tool_delta("\n{\"ok\":true}", false),
            "    ⤷ {\"ok\":true}\n"
        );
        assert_eq!(format_tool_delta("", false), "");
    }

    #[test]
    fn flux_par_increments_et_reecriture() {
        let terminal = Terminal::new(
            Style {
                color: false,
                status: false,
            },
            false,
            crate::i18n::tr(crate::i18n::Lang::Fr),
        );
        let idx = terminal.append("assistant", "");
        terminal.update_last("assistant", "Bon");
        terminal.update_last("assistant", "Bonjour");
        assert_eq!(terminal.lock().cursor, Some(idx));
        assert_eq!(terminal.lock().rows[idx].text, "Bonjour");
        let tool = terminal.append("tool", "");
        terminal.set_row(tool, "tool", "→ hypr_status({})");
        terminal.set_row(tool, "tool", "→ hypr_status({})\nok");
        assert_eq!(terminal.lock().cursor, Some(tool));
        terminal.remove_last();
        assert_eq!(terminal.lock().rows.len(), 1);
        assert_eq!(terminal.lock().cursor, None);
        terminal.append("error", "boom");
        assert_eq!(terminal.errors.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn choix_numerotes() {
        let terminal = Terminal::new(
            Style {
                color: false,
                status: false,
            },
            false,
            crate::i18n::tr(crate::i18n::Lang::Fr),
        );
        let raw = r##"{"question":"Quelle ambiance ?","options":[{"id":"a","label":"A"},{"id":"b","label":"B"}]}"##;
        terminal.offer_choices(ChoiceRequest::parse(raw).unwrap());
        let (display, context) = terminal.take_choice("2").unwrap();
        assert_eq!(display, "Je choisis « B ».");
        assert!(context.contains("\"id\":\"b\""));
        // une fois choisie, elle est fermée
        assert!(terminal.take_choice("1").is_none());
        // une réponse libre ferme aussi la proposition
        terminal.offer_choices(ChoiceRequest::parse(raw).unwrap());
        assert!(terminal.take_choice("plutôt du vert").is_none());
        assert!(terminal.take_choice("1").is_none());
        // un numéro hors plage est une réponse libre
        terminal.offer_choices(ChoiceRequest::parse(raw).unwrap());
        assert!(terminal.take_choice("3").is_none());
        assert!(terminal.take_choice("1").is_none());
    }
}
