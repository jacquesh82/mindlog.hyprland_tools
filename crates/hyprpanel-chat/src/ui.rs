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
