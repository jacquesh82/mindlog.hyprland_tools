//! Rendu de l'interface réelle sans Wayland, serveur MCP, clé API ni appel LLM.
//! cargo run -p hyprpanel-chat --example preview --offline
//! Les captures et vérifications d'interaction vont dans target/lyra-previews.

#[path = "../src/i18n.rs"]
mod i18n;

use std::{cell::RefCell, path::Path, rc::Rc, time::Instant};

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, ModelRc, PhysicalSize, SharedPixelBuffer, VecModel};

slint::include_modules!();

struct PreviewPlatform {
    window: Rc<MinimalSoftwareWindow>,
    started: Instant,
}

impl Platform for PreviewPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> std::time::Duration {
        self.started.elapsed()
    }
}

fn snapshot(
    window: &MinimalSoftwareWindow,
    path: &Path,
    width: u32,
    height: u32,
) -> anyhow::Result<()> {
    window.set_size(PhysicalSize::new(width, height));
    slint::platform::update_timers_and_animations();
    window.request_redraw();
    let mut pixels = SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
    window.draw_if_needed(|renderer| {
        renderer.render(pixels.make_mut_slice(), width as usize);
    });
    image::save_buffer(
        path,
        pixels.as_bytes(),
        width,
        height,
        image::ColorType::Rgb8,
    )?;
    Ok(())
}

fn click(app: &ChatWindow, x: f32, y: f32) {
    let position = slint::LogicalPosition::new(x, y);
    app.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    app.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    app.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    slint::platform::update_timers_and_animations();
}

fn main() -> anyhow::Result<()> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/lyra-previews".into());
    let out = Path::new(&out);
    std::fs::create_dir_all(out)?;
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(PreviewPlatform {
        window: window.clone(),
        started: Instant::now(),
    }))?;
    let app = ChatWindow::new()?;
    app.set_tr(i18n::tr(i18n::Lang::Fr).to_slint());
    app.set_status("MCP : 20 outils".into());
    app.set_model_label("GLM-5.3".into());
    let messages = Rc::new(VecModel::<MessageRow>::default());
    app.set_messages(ModelRc::from(messages.clone()));
    let attachments = Rc::new(VecModel::<AttachmentRow>::default());
    app.set_attachments(ModelRc::from(attachments.clone()));
    let history = Rc::new(VecModel::<HistoryRow>::default());
    app.set_history(ModelRc::from(history.clone()));
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sent_callback = sent.clone();
    app.on_send(move |text| sent_callback.borrow_mut().push(text.to_string()));
    let weak = app.as_weak();
    app.on_close_overlay(move || {
        if let Some(app) = weak.upgrade() {
            app.set_overlay("none".into());
            app.invoke_focus_composer();
        }
    });
    let weak = app.as_weak();
    app.on_open_settings(move || {
        if let Some(app) = weak.upgrade() {
            app.set_overlay("settings".into());
        }
    });
    app.show()?;

    snapshot(&window, &out.join("welcome-desktop.png"), 1080, 800)?;
    snapshot(&window, &out.join("welcome-wide.png"), 1366, 768)?;
    snapshot(&window, &out.join("welcome-compact.png"), 480, 720)?;
    snapshot(&window, &out.join("welcome-minimum.png"), 480, 600)?;
    app.set_tr(i18n::tr(i18n::Lang::En).to_slint());
    snapshot(&window, &out.join("welcome-english.png"), 1080, 800)?;
    app.set_tr(i18n::tr(i18n::Lang::Fr).to_slint());

    messages.set_vec(vec![
        MessageRow { kind: "user".into(), text: "Peux-tu rendre mon bureau un peu plus aéré ?".into() },
        MessageRow { kind: "assistant".into(), text: "Bien sûr. Je vais essayer des espaces de 10 pixels entre vos fenêtres. Vous pourrez vérifier le résultat avant de conserver ce réglage.".into() },
        MessageRow { kind: "tool".into(), text: "hypr_set_option_live · general:gaps_in → 10\nAperçu appliqué. La configuration enregistrée est conservée.".into() },
        MessageRow { kind: "assistant".into(), text: "Voilà, votre bureau respire un peu plus. Souhaitez-vous conserver ce réglage ou revenir à la disposition précédente ?".into() },
    ]);
    snapshot(&window, &out.join("conversation.png"), 1080, 800)?;
    snapshot(&window, &out.join("conversation-compact.png"), 480, 720)?;
    messages.set_vec(Vec::new());
    attachments.push(AttachmentRow {
        name: "hyprland.lua".into(),
        meta: "12 Ko · texte".into(),
    });
    attachments.push(AttachmentRow {
        name: "bureau.png".into(),
        meta: "240 Ko · image".into(),
    });
    app.set_draft("Voici ma configuration actuelle.".into());
    snapshot(&window, &out.join("attachments.png"), 1080, 800)?;
    attachments.set_vec(Vec::new());
    app.set_draft("".into());

    app.set_cfg_model("GLM-5.3".into());
    app.set_cfg_base_url("https://api.z.ai/api/anthropic".into());
    app.set_cfg_max_tokens("8192".into());
    app.set_clients_claude_ok(true);
    app.set_clients_zcode_ok(true);
    history.push(HistoryRow {
        turn: "n°1".into(),
        preview: "Aérer le bureau".into(),
        meta: "2026-11-08 · lyra-turn-001".into(),
        has_backup: true,
        backup: "lyra-turn-001".into(),
    });
    for panel in ["help", "settings", "history"] {
        app.set_overlay(panel.into());
        snapshot(&window, &out.join(format!("{panel}.png")), 1080, 800)?;
        snapshot(&window, &out.join(format!("{panel}-compact.png")), 480, 600)?;
    }
    app.set_overlay("none".into());
    snapshot(&window, &out.join("interaction-start.png"), 1080, 800)?;

    // Une suggestion prépare le brouillon, sans déclencher d'action Hyprland.
    click(&app, 300., 410.);
    assert!(
        !app.get_draft().is_empty(),
        "la carte doit préparer le brouillon"
    );
    assert!(
        sent.borrow().is_empty(),
        "une suggestion ne doit pas envoyer automatiquement"
    );
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Return.into(),
    });
    assert_eq!(
        sent.borrow().len(),
        1,
        "Entrée doit envoyer depuis le champ de saisie"
    );
    assert!(
        app.get_draft().is_empty(),
        "le brouillon doit être vidé après envoi"
    );

    // Le bouton standard conserve aussi la navigation clavier des cartes.
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Return.into(),
    });
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Shift.into(),
    });
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Tab.into(),
    });
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Tab.into(),
    });
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Shift.into(),
    });
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Return.into(),
    });
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Return.into(),
    });
    assert!(
        !app.get_draft().is_empty(),
        "une carte doit être activable au clavier"
    );
    assert_eq!(
        sent.borrow().len(),
        1,
        "activer une carte prépare le message"
    );
    snapshot(&window, &out.join("draft-ready.png"), 1080, 800)?;

    app.set_draft("".into());
    app.set_busy(true);
    click(&app, 300., 410.);
    assert!(
        app.get_draft().is_empty(),
        "les suggestions sont désactivées pendant un tour"
    );
    app.set_busy(false);
    app.set_overlay("settings".into());
    snapshot(&window, &out.join("settings.png"), 1080, 800)?;
    click(&app, 540., 600.);
    assert_eq!(app.get_overlay().as_str(), "settings", "un clic dans le panneau ne doit pas le fermer");
    // Une édition doit modifier la propriété lue par le bouton Enregistrer.
    click(&app, 450., 296.);
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Control.into(),
    });
    app.window()
        .dispatch_event(WindowEvent::KeyPressed { text: "a".into() });
    app.window()
        .dispatch_event(WindowEvent::KeyReleased { text: "a".into() });
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Control.into(),
    });
    for ch in "preview-model".chars() {
        app.window().dispatch_event(WindowEvent::KeyPressed {
            text: ch.to_string().into(),
        });
        app.window().dispatch_event(WindowEvent::KeyReleased {
            text: ch.to_string().into(),
        });
    }
    assert_eq!(
        app.get_cfg_model().as_str(),
        "preview-model",
        "la saisie doit mettre à jour les réglages"
    );
    app.set_cfg_model("GLM-5.3".into());
    app.set_settings_error(app.get_tr().settings_invalid);
    snapshot(&window, &out.join("settings-validation.png"), 1080, 800)?;
    app.set_settings_error("".into());
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    assert_eq!(
        app.get_overlay().as_str(),
        "none",
        "Échap doit fermer le panneau"
    );
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Escape.into(),
    });
    println!(
        "Captures écrites dans {} ; interactions vérifiées.",
        out.display()
    );
    Ok(())
}
