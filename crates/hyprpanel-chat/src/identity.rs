//! Identité de LYRA et environnement d'exécution : toujours présents dans le
//! contexte système, à chaque tour et quel que soit l'accord fichiers. Le
//! profil détaillé (paquets, outils, configurations) reste soumis à l'accord.

use std::path::Path;

use crate::chat_config::ChatConfig;
use crate::i18n::Lang;
use crate::operations::Mode;

/// Interface par laquelle l'utilisateur parle à LYRA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interface {
    Window,
    Terminal,
    /// Interface tierce (hyprdesk) via `hyprpanel-chat serve`.
    Server,
}

/// Faits observés sur la machine, sans commande ni accord nécessaire.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub desktop: String,
    pub session: String,
    pub hyprland: bool,
    pub shell: String,
    pub locale: String,
}

/// Lit les faits : `/etc/os-release`, `/proc`, variables de session.
pub fn facts() -> Facts {
    let env = |key: &str| std::env::var(key).unwrap_or_default();
    let os = crate::environment::read_small(Path::new("/etc/os-release"))
        .or_else(|| crate::environment::read_small(Path::new("/usr/lib/os-release")))
        .map(|text| crate::environment::os_release(&text))
        .and_then(|values| {
            values
                .get("PRETTY_NAME")
                .or_else(|| values.get("ID"))
                .cloned()
        })
        .unwrap_or_default();
    let kernel = crate::environment::read_small(Path::new("/proc/sys/kernel/osrelease"))
        .map(|text| text.trim().to_string())
        .unwrap_or_default();
    let session = if !env("WAYLAND_DISPLAY").is_empty() {
        "wayland".to_string()
    } else {
        env("XDG_SESSION_TYPE")
    };
    Facts {
        os,
        kernel,
        arch: std::env::consts::ARCH.to_string(),
        desktop: env("XDG_CURRENT_DESKTOP"),
        session,
        hyprland: !env("HYPRLAND_INSTANCE_SIGNATURE").is_empty(),
        shell: env("SHELL")
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string(),
        locale: env("LANG"),
    }
}

/// État de l'application utile au bloc (séparé pour rester testable).
pub struct State<'a> {
    pub interface: Interface,
    pub filesystem_approved: bool,
    pub root: &'a Path,
    /// Nombre d'outils MCP du daemon, `None` s'il est injoignable.
    pub mcp_tools: Option<usize>,
}

fn or_unknown(value: &str, lang: Lang) -> &str {
    match (value.trim().is_empty(), lang) {
        (true, Lang::Fr) => "inconnu",
        (true, Lang::En) => "unknown",
        (false, _) => value.trim(),
    }
}

/// Bloc « identité et environnement d'exécution » — pur, testé.
pub fn block(lang: Lang, cfg: &ChatConfig, state: &State<'_>, facts: &Facts) -> String {
    let fr = lang == Lang::Fr;
    let model = match cfg.active_model().trim() {
        "" if fr => "choisi par le client officiel de l'abonnement",
        "" => "chosen by the subscription's official client",
        model => model,
    };
    let interface = match (state.interface, fr) {
        (Interface::Window, true) => "fenêtre de chat native (Slint, Wayland, app_id lyra)",
        (Interface::Window, false) => "native chat window (Slint, Wayland, app_id lyra)",
        (Interface::Terminal, true) => "terminal (hyprpanel-chat cli)",
        (Interface::Terminal, false) => "terminal (hyprpanel-chat cli)",
        (Interface::Server, true) => {
            "panneau d'une application tierce (hyprdesk) via hyprpanel-chat serve ; réponses en Markdown"
        }
        (Interface::Server, false) => {
            "panel of a third-party application (hyprdesk) via hyprpanel-chat serve; replies in Markdown"
        }
    };
    let mode = match (cfg.operation_mode, fr) {
        (Mode::ConfirmOnce, true) => {
            "confirmer une fois — chaque programme est confirmé une fois par session, les commandes dangereuses à chaque fois"
        }
        (Mode::ConfirmOnce, false) => {
            "confirm once — each program is confirmed once per session, dangerous commands every time"
        }
        (Mode::FullAccess, true) => "FULL access — exécution sans confirmation",
        (Mode::FullAccess, false) => "FULL access — runs without confirmation",
        (Mode::SudoOnly, true) => {
            "confirmer sudo — commandes ordinaires libres, commandes dangereuses confirmées"
        }
        (Mode::SudoOnly, false) => {
            "confirm sudo — ordinary commands run freely, dangerous ones are confirmed"
        }
        (Mode::CopyOnly, true) => "commandes à copier — rien n'est exécuté ni modifié",
        (Mode::CopyOnly, false) => "copy-only — nothing is executed or modified",
    };
    // Le chemin réel révèle le nom d'utilisateur : seulement après accord.
    let root = if state.filesystem_approved {
        state.root.display().to_string()
    } else {
        "~".to_string()
    };
    let files = match (state.filesystem_approved, fr) {
        (true, true) => format!("accordé pour la session, dossier {root}"),
        (true, false) => format!("granted for this session, folder {root}"),
        (false, true) => "non accordé (à demander dans les réglages)".to_string(),
        (false, false) => "not granted (ask for it in Settings)".to_string(),
    };
    let desktop = match (state.mcp_tools, fr) {
        (Some(n), true) => format!("daemon hyprpanel connecté ({n} outils MCP)"),
        (Some(n), false) => format!("hyprpanel daemon connected ({n} MCP tools)"),
        (None, true) => "daemon hyprpanel injoignable : pas d'outils Hyprland".to_string(),
        (None, false) => "hyprpanel daemon unreachable: no Hyprland tools".to_string(),
    };
    let session = format!(
        "{} / {}{}",
        or_unknown(&facts.desktop, lang),
        or_unknown(&facts.session, lang),
        match (facts.hyprland, fr) {
            (true, true) => ", Hyprland actif",
            (true, false) => ", Hyprland running",
            (false, true) => ", Hyprland non détecté",
            (false, false) => ", Hyprland not detected",
        }
    );
    let version = env!("CARGO_PKG_VERSION");
    if fr {
        format!(
            "IDENTITÉ ET ENVIRONNEMENT D'EXÉCUTION — fournis par l'application à chaque tour, ils font foi\n\
- Toi : LYRA, agente de bureau de hyprpanel-chat {version}. Fournisseur : {} ; modèle : {model}.\n\
- Interface : {interface}.\n\
- Outils : fichiers (lyra_list_directory, lyra_read_file, lyra_write_file), commandes (lyra_run_command : /bin/sh, dossier courant {root}), web (lyra_web_search, lyra_web_fetch), contraste, choix cliquables ; bureau : {desktop}.\n\
- Accès fichiers : {files}.\n\
- Mode d'exécution : {mode}.\n\
- Machine : {} ; noyau {} ; {} ; session {session} ; shell {} ; langue système {}.",
            cfg.provider.label(),
            or_unknown(&facts.os, lang),
            or_unknown(&facts.kernel, lang),
            facts.arch,
            or_unknown(&facts.shell, lang),
            or_unknown(&facts.locale, lang),
        )
    } else {
        format!(
            "IDENTITY AND EXECUTION ENVIRONMENT — supplied by the application on every turn; authoritative\n\
- You: LYRA, the desktop agent of hyprpanel-chat {version}. Provider: {}; model: {model}.\n\
- Interface: {interface}.\n\
- Tools: files (lyra_list_directory, lyra_read_file, lyra_write_file), commands (lyra_run_command: /bin/sh, working directory {root}), web (lyra_web_search, lyra_web_fetch), contrast, clickable choices; desktop: {desktop}.\n\
- Filesystem access: {files}.\n\
- Execution mode: {mode}.\n\
- Machine: {}; kernel {}; {}; session {session}; shell {}; system locale {}.",
            cfg.provider.label(),
            or_unknown(&facts.os, lang),
            or_unknown(&facts.kernel, lang),
            facts.arch,
            or_unknown(&facts.shell, lang),
            or_unknown(&facts.locale, lang),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Facts {
        Facts {
            os: "Arch Linux".into(),
            kernel: "7.2.14-arch1-1".into(),
            arch: "x86_64".into(),
            desktop: "Hyprland".into(),
            session: "wayland".into(),
            hyprland: true,
            shell: "bash".into(),
            locale: "fr_FR.UTF-8".into(),
        }
    }

    #[test]
    fn bloc_identite_et_environnement_toujours_complet() {
        let mut cfg = ChatConfig::defaults();
        cfg.provider = crate::chat_config::Provider::Codex;
        let root = Path::new("/home/someone");
        let state = State {
            interface: Interface::Terminal,
            filesystem_approved: false,
            root,
            mcp_tools: None,
        };
        let text = block(Lang::Fr, &cfg, &state, &sample());
        for expected in [
            "Toi : LYRA",
            "Fournisseur : Codex",
            "choisi par le client officiel",
            "terminal (hyprpanel-chat cli)",
            "Arch Linux",
            "7.2.14-arch1-1",
            "Hyprland actif",
            "shell bash",
            "non accordé",
            "injoignable",
            "confirmer une fois",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        // Sans accord, aucun chemin personnel (nom d'utilisateur).
        assert!(!text.contains("/home/someone"));

        let approved = State {
            interface: Interface::Window,
            filesystem_approved: true,
            root,
            mcp_tools: Some(18),
        };
        let text = block(Lang::En, &cfg, &approved, &Facts::default());
        assert!(text.contains("You: LYRA"));
        assert!(text.contains("app_id lyra"));
        assert!(text.contains("/home/someone"));
        assert!(text.contains("18 MCP tools"));
        assert!(text.contains("kernel unknown"));
    }
}
