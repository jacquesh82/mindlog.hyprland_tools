//! Pont UI : les tâches tokio manipulent la fenêtre Slint via
//! `invoke_from_event_loop`. Seul le thread agent écrit dans le modèle ;
//! le compteur `ROWS` suit donc fidèlement les indices de lignes (les
//! opérations sont exécutées dans l'ordre par la boucle d'événements).
//!
//! `Front` choisit la sortie : la fenêtre Slint ou le terminal (`cli`). La
//! boucle d'agent ne connaît que `Front` ; les fonctions de chips restent
//! propres à la fenêtre.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use slint::{Model, ModelRc, Weak};

use crate::ChatWindow;
use crate::MessageRow;
use crate::cli::Terminal;
use crate::i18n::Tr;

static ROWS: AtomicUsize = AtomicUsize::new(0);

/// Sortie de la boucle d'agent : fenêtre Slint ou terminal.
#[derive(Clone)]
pub enum Front {
    Window(Weak<ChatWindow>),
    Terminal(Arc<Terminal>),
    /// Interface tierce : protocole JSON Lines (`hyprpanel-chat serve`).
    Server(Arc<crate::serve::Server>),
}

impl From<&Weak<ChatWindow>> for Front {
    fn from(weak: &Weak<ChatWindow>) -> Self {
        Self::Window(weak.clone())
    }
}

impl From<&Front> for Front {
    fn from(front: &Front) -> Self {
        front.clone()
    }
}

fn queue(
    weak: &Weak<ChatWindow>,
    op: impl FnOnce(&ChatWindow, &slint::VecModel<MessageRow>) + Send + 'static,
) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = weak.upgrade() else { return };
        let model: ModelRc<MessageRow> = ui.get_messages();
        let Some(model) = model.as_any().downcast_ref::<slint::VecModel<MessageRow>>() else {
            return;
        };
        op(&ui, model);
        ui.invoke_scroll_bottom();
    });
}

/// Ajoute une bulle et renvoie son index (côté compteur logique).
pub fn append(front: impl Into<Front>, kind: &str, text: impl Into<String>) -> usize {
    match front.into() {
        Front::Window(weak) => window_append(&weak, kind, text.into()),
        Front::Terminal(terminal) => terminal.append(kind, &text.into()),
        Front::Server(server) => server.append(kind, &text.into()),
    }
}

fn window_append(weak: &Weak<ChatWindow>, kind: &str, text: String) -> usize {
    let idx = ROWS.fetch_add(1, Ordering::SeqCst);
    let kind = kind.to_string();
    let rich = (kind == "assistant").then(|| crate::rich_text::parse(&text));
    queue(weak, move |ui, model| {
        if kind == "user" {
            ui.set_follow_output(true);
        }
        let mut row = MessageRow {
            kind: kind.into(),
            text: text.into(),
            ..Default::default()
        };
        if let Some(rich) = rich {
            rich.apply(&mut row);
        }
        model.push(row);
    });
    idx
}

/// Met à jour la dernière bulle (streaming du texte assistant).
pub fn update_last(front: impl Into<Front>, kind: &str, text: &str) {
    match front.into() {
        Front::Window(weak) => {
            let idx = ROWS.load(Ordering::SeqCst).saturating_sub(1);
            window_set_row(&weak, idx, kind, text);
        }
        Front::Terminal(terminal) => terminal.update_last(kind, text),
        Front::Server(server) => server.update_last(kind, text),
    }
}

pub fn set_row(front: impl Into<Front>, idx: usize, kind: &str, text: &str) {
    match front.into() {
        Front::Window(weak) => window_set_row(&weak, idx, kind, text),
        Front::Terminal(terminal) => terminal.set_row(idx, kind, text),
        Front::Server(server) => server.set_row(idx, kind, text),
    }
}

fn window_set_row(weak: &Weak<ChatWindow>, idx: usize, kind: &str, text: &str) {
    let kind = kind.to_string();
    let text = text.to_string();
    let rich = (kind == "assistant").then(|| crate::rich_text::parse(&text));
    queue(weak, move |_, model| {
        if idx < model.row_count() {
            let mut row = MessageRow {
                kind: kind.into(),
                text: text.into(),
                ..Default::default()
            };
            if let Some(rich) = rich {
                rich.apply(&mut row);
            }
            model.set_row_data(idx, row);
        }
    });
}

/// Supprime la dernière ligne (bulle restée vide après un tour outils).
pub fn remove_last(front: impl Into<Front>) {
    let weak = match front.into() {
        Front::Window(weak) => weak,
        Front::Terminal(terminal) => return terminal.remove_last(),
        Front::Server(server) => return server.remove_last(),
    };
    let weak = &weak;
    let idx = ROWS.fetch_sub(1, Ordering::SeqCst).saturating_sub(1);
    queue(weak, move |_, model| {
        if idx < model.row_count() {
            model.remove(idx);
        }
    });
}

/// Vide le journal (nouvelle conversation).
pub fn clear(front: impl Into<Front>) {
    let weak = match front.into() {
        Front::Window(weak) => weak,
        Front::Terminal(terminal) => return terminal.clear(),
        Front::Server(server) => return server.clear(),
    };
    let weak = &weak;
    ROWS.store(0, Ordering::SeqCst);
    queue(weak, |ui, model| {
        ui.set_follow_output(true);
        model.set_vec(Vec::new());
    });
}

pub fn offer_choices(front: impl Into<Front>, request: crate::choices::ChoiceRequest) {
    let weak = match front.into() {
        Front::Window(weak) => weak,
        Front::Terminal(terminal) => return terminal.offer_choices(request),
        Front::Server(server) => return server.offer_choices(request),
    };
    let weak = &weak;
    ROWS.fetch_add(1, Ordering::SeqCst);
    queue(weak, move |ui, model| {
        crate::choices::finish_pending_choices(ui);
        ui.invoke_prepare_choice();
        model.push(request.to_row());
    });
}

pub fn set_status(front: impl Into<Front>, status: impl Into<String>) {
    let status = status.into();
    let weak = match front.into() {
        Front::Window(weak) => weak,
        Front::Terminal(terminal) => return terminal.set_status(&status),
        // La barre d'état n'a pas d'équivalent dans le protocole.
        Front::Server(_) => return,
    };
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_status(status.into());
        }
    });
}

pub fn set_busy(front: impl Into<Front>, busy: bool) {
    let Front::Window(weak) = front.into() else {
        return;
    };
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(busy);
            ui.set_task_label("".into());
            ui.set_task_progress(0.);
            ui.set_task_indeterminate(true);
            if !busy && ui.get_overlay() == "none" {
                ui.invoke_focus_composer();
            }
        }
    });
}

/// Progression mesurée par phase ; None signifie une durée/quantité inconnue.
pub fn set_progress(front: impl Into<Front>, label: String, progress: Option<f32>) {
    let weak = match front.into() {
        Front::Window(weak) => weak,
        Front::Terminal(terminal) => return terminal.set_status(&label),
        Front::Server(server) => return server.progress(&label, progress),
    };
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_task_label(label.into());
            ui.set_task_progress(progress.unwrap_or(0.).clamp(0., 1.));
            ui.set_task_indeterminate(progress.is_none());
        }
    });
}

pub fn set_activity(front: impl Into<Front>, activity: &str) {
    let weak = match front.into() {
        Front::Window(weak) => weak,
        Front::Server(server) => return server.activity(activity),
        Front::Terminal(_) => return,
    };
    let activity = activity.to_string();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_activity(activity.into());
        }
    });
}

/// Ajoute une chip de pièce jointe.
pub fn attachment_push(weak: &Weak<ChatWindow>, name: &str, meta: &str) {
    let name = name.to_string();
    let meta = meta.to_string();
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = weak.upgrade() else { return };
        let model = ui.get_attachments();
        if let Some(m) = model
            .as_any()
            .downcast_ref::<slint::VecModel<crate::AttachmentRow>>()
        {
            m.push(crate::AttachmentRow {
                name: name.into(),
                meta: meta.into(),
            });
        }
    });
}

/// Retire la chip d'index donné.
pub fn attachment_remove(weak: &Weak<ChatWindow>, idx: usize) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = weak.upgrade() else { return };
        let model = ui.get_attachments();
        if let Some(m) = model
            .as_any()
            .downcast_ref::<slint::VecModel<crate::AttachmentRow>>()
        {
            if idx < m.row_count() {
                m.remove(idx);
            }
        }
    });
}

/// Vide toutes les chips (au moment de l'envoi).
pub fn attachments_clear(weak: &Weak<ChatWindow>) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = weak.upgrade() else { return };
        let model = ui.get_attachments();
        if let Some(m) = model
            .as_any()
            .downcast_ref::<slint::VecModel<crate::AttachmentRow>>()
        {
            m.set_vec(Vec::new());
        }
    });
}

/// Remplace tout l'historique (l'affichage veut l'ordre antéchronologique).
pub fn history_set(front: impl Into<Front>, records: &[crate::agent::TurnRecord]) {
    let weak = match front.into() {
        Front::Window(weak) => weak,
        Front::Terminal(terminal) => return terminal.history_set(records),
        Front::Server(_) => return,
    };
    let rows: Vec<crate::HistoryRow> = records
        .iter()
        .rev()
        .map(|r| crate::HistoryRow {
            turn: format!("n°{}", r.turn).into(),
            preview: r.preview.clone().into(),
            meta: format!("{} · {}", r.timestamp, r.backup.as_deref().unwrap_or("—")).into(),
            has_backup: r.backup.is_some(),
            backup: r.backup.clone().unwrap_or_default().into(),
        })
        .collect();
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = weak.upgrade() else { return };
        let model = ui.get_history();
        if let Some(m) = model
            .as_any()
            .downcast_ref::<slint::VecModel<crate::HistoryRow>>()
        {
            m.set_vec(rows);
        }
    });
}

/// Remplace le dictionnaire i18n (changement de langue à chaud).
pub fn set_tr(front: impl Into<Front>, translations: &crate::i18n::Tr) {
    let weak = match front.into() {
        Front::Window(weak) => weak,
        Front::Terminal(terminal) => return terminal.set_tr(translations.clone()),
        Front::Server(server) => return server.set_tr(translations.clone()),
    };
    let texts = translations.to_slint();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_tr(texts);
        }
    });
}

/// Une ligne d'état fournisseur : nom, prêt, détail (déjà traduit).
pub type ProviderLine = (String, bool, String);

/// Affiche l'état des fournisseurs. Fenêtre : panneau au-dessus de la saisie
/// et blocage d'Envoyer tant que `ready` est faux. Terminal : bloc imprimé.
pub fn set_providers(
    front: &Front,
    ready: bool,
    available: &[crate::chat_config::Provider],
    lines: Vec<ProviderLine>,
    tr: &Tr,
) {
    match front {
        Front::Server(server) => {
            server.providers(ready, available, &providers_text(&lines, tr));
        }
        Front::Window(weak) => {
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(app) = weak.upgrade() else { return };
                let rows: Vec<crate::ProviderStatusRow> = lines
                    .into_iter()
                    .map(|(name, ready, detail)| crate::ProviderStatusRow {
                        name: name.into(),
                        ready,
                        detail: detail.into(),
                    })
                    .collect();
                app.set_provider_status(std::rc::Rc::new(slint::VecModel::from(rows)).into());
                app.set_providers_ready(ready);
            });
        }
        Front::Terminal(terminal) => {
            if !ready {
                terminal.append("error", &providers_text(&lines, tr));
            }
        }
    }
}

/// Texte du panneau pour le terminal — pur.
pub fn providers_text(lines: &[ProviderLine], tr: &Tr) -> String {
    let mut text = format!("{}\n{}", tr.providers_title, tr.providers_intro_cli);
    for (name, ready, detail) in lines {
        text.push_str(&format!(
            "\n  {} {name} : {detail}",
            if *ready { "✓" } else { "✗" }
        ));
    }
    text
}

pub fn set_providers_checking(front: &Front, checking: bool) {
    if let Front::Window(weak) = front {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = weak.upgrade() {
                app.set_providers_checking(checking);
            }
        });
    }
}

/// Repli de session : la fenêtre met à jour son sélecteur et sa copie de la
/// configuration (sans l'enregistrer).
pub fn provider_fallback(front: &Front, provider: crate::chat_config::Provider) {
    if let Front::Server(server) = front {
        server.set_provider(provider);
    }
    if let Front::Window(weak) = front {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = weak.upgrade() {
                app.invoke_provider_fallback(provider.index());
            }
        });
    }
}

/// Début d'un tour (message ou choix) : `turn_start` côté protocole.
pub fn turn_start(front: &Front) {
    if let Front::Server(server) = front {
        server.turn_start();
    }
}

/// Fin d'un tour, quelle qu'en soit l'issue : `turn_end` côté protocole.
pub fn turn_end(front: &Front) {
    if let Front::Server(server) = front {
        server.turn_end();
    }
}

/// Un outil démarre (nom et arguments résumés).
pub fn tool_started(front: &Front, name: &str, summary: &str) {
    if let Front::Server(server) = front {
        server.tool_started(name, summary);
    }
}

/// Résultat brut de l'outil en cours (JSON ou texte).
pub fn tool_finished(front: &Front, output: &str) {
    if let Front::Server(server) = front {
        server.tool_finished(output);
    }
}
