//! Pont UI : les tâches tokio manipulent la fenêtre Slint via
//! `invoke_from_event_loop`. Seul le thread agent écrit dans le modèle ;
//! le compteur `ROWS` suit donc fidèlement les indices de lignes (les
//! opérations sont exécutées dans l'ordre par la boucle d'événements).

use std::sync::atomic::{AtomicUsize, Ordering};

use slint::{Model, ModelRc, Weak};

use crate::ChatWindow;
use crate::MessageRow;

static ROWS: AtomicUsize = AtomicUsize::new(0);

fn queue(weak: &Weak<ChatWindow>, op: impl FnOnce(&ChatWindow, &slint::VecModel<MessageRow>) + Send + 'static) {
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
pub fn append(weak: &Weak<ChatWindow>, kind: &str, text: impl Into<String>) -> usize {
    let idx = ROWS.fetch_add(1, Ordering::SeqCst);
    let kind = kind.to_string();
    let text = text.into();
    queue(weak, move |_, model| {
        model.push(MessageRow { kind: kind.into(), text: text.into() });
    });
    idx
}

/// Met à jour la dernière bulle (streaming du texte assistant).
pub fn update_last(weak: &Weak<ChatWindow>, kind: &str, text: &str) {
    let idx = ROWS.load(Ordering::SeqCst).saturating_sub(1);
    set_row(weak, idx, kind, text);
}

pub fn set_row(weak: &Weak<ChatWindow>, idx: usize, kind: &str, text: &str) {
    let kind = kind.to_string();
    let text = text.to_string();
    queue(weak, move |_, model| {
        if idx < model.row_count() {
            model.set_row_data(idx, MessageRow { kind: kind.into(), text: text.into() });
        }
    });
}

/// Supprime la dernière ligne (bulle restée vide après un tour outils).
pub fn remove_last(weak: &Weak<ChatWindow>) {
    let idx = ROWS.fetch_sub(1, Ordering::SeqCst).saturating_sub(1);
    queue(weak, move |_, model| {
        if idx < model.row_count() {
            model.remove(idx);
        }
    });
}

/// Vide le journal (nouvelle conversation).
pub fn clear(weak: &Weak<ChatWindow>) {
    ROWS.store(0, Ordering::SeqCst);
    queue(weak, |_, model| {
        model.set_vec(Vec::new());
    });
}

pub fn set_status(weak: &Weak<ChatWindow>, status: impl Into<String>) {
    let status = status.into();
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_status(status.into());
        }
    });
}

pub fn set_busy(weak: &Weak<ChatWindow>, busy: bool) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(busy);
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
        if let Some(m) = model.as_any().downcast_ref::<slint::VecModel<crate::AttachmentRow>>() {
            m.push(crate::AttachmentRow { name: name.into(), meta: meta.into() });
        }
    });
}

/// Retire la chip d'index donné.
pub fn attachment_remove(weak: &Weak<ChatWindow>, idx: usize) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = weak.upgrade() else { return };
        let model = ui.get_attachments();
        if let Some(m) = model.as_any().downcast_ref::<slint::VecModel<crate::AttachmentRow>>() {
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
        if let Some(m) = model.as_any().downcast_ref::<slint::VecModel<crate::AttachmentRow>>() {
            m.set_vec(Vec::new());
        }
    });
}

/// Remplace tout l'historique (l'affichage veut l'ordre antéchronologique).
pub fn history_set(weak: &Weak<ChatWindow>, records: &[crate::agent::TurnRecord]) {
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
        if let Some(m) = model.as_any().downcast_ref::<slint::VecModel<crate::HistoryRow>>() {
            m.set_vec(rows);
        }
    });
}

/// Remplace le dictionnaire i18n (changement de langue à chaud).
pub fn set_tr(weak: &Weak<ChatWindow>, texts: crate::TrTexts) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_tr(texts);
        }
    });
}
