//! Rendu de l'interface réelle sans Wayland, serveur MCP, clé API ni appel LLM.
//! cargo run -p hyprpanel-chat --example preview --offline
//! Les captures et vérifications d'interaction vont dans target/lyra-previews.

#[path = "../src/avatar.rs"]
mod avatar;
#[path = "../src/choices.rs"]
mod choices;
#[path = "../src/i18n.rs"]
mod i18n;
#[path = "../src/rich_text.rs"]
mod rich_text;

use std::{cell::RefCell, path::Path, rc::Rc, time::Instant};

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, Model, ModelRc, PhysicalSize, SharedPixelBuffer, VecModel};

slint::include_modules!();

struct PreviewPlatform {
    window: Rc<MinimalSoftwareWindow>,
    started: Instant,
    clipboard: Rc<RefCell<String>>,
}

impl Platform for PreviewPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> std::time::Duration {
        self.started.elapsed()
    }
    fn set_clipboard_text(&self, text: &str, clipboard: slint::platform::Clipboard) {
        if matches!(clipboard, slint::platform::Clipboard::DefaultClipboard) {
            *self.clipboard.borrow_mut() = text.into();
        }
    }
    fn clipboard_text(&self, clipboard: slint::platform::Clipboard) -> Option<String> {
        matches!(clipboard, slint::platform::Clipboard::DefaultClipboard)
            .then(|| self.clipboard.borrow().clone())
    }
}

fn snapshot(
    app: &ChatWindow,
    window: &MinimalSoftwareWindow,
    path: &Path,
    width: u32,
    height: u32,
) -> anyhow::Result<()> {
    window.set_size(PhysicalSize::new(width, height));
    let messages = app.get_messages();
    for index in 0..messages.row_count() {
        if let Some(mut row) = messages.row_data(index) {
            if row.kind == "assistant" && row.html.is_empty() {
                rich_text::parse(&row.text).apply(&mut row);
                messages.set_row_data(index, row);
            }
        }
    }
    let mut pixels = SharedPixelBuffer::<slint::Rgb8Pixel>::new(width, height);
    // Donner aux callbacks de géométrie et au suivi du streaming un tour de boucle.
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(pixels.make_mut_slice(), width as usize);
        });
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
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
    let text_clipboard = Rc::new(RefCell::new(String::new()));
    slint::platform::set_platform(Box::new(PreviewPlatform {
        window: window.clone(),
        started: Instant::now(),
        clipboard: text_clipboard.clone(),
    }))?;
    let app = ChatWindow::new()?;
    avatar::install(&app);
    app.set_animations_enabled(false);
    let pasted = Rc::new(RefCell::new(Vec::new()));
    let pasted_callback = pasted.clone();
    app.on_paste_clipboard(move |image_only| pasted_callback.borrow_mut().push(image_only));
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
    let opened_links = Rc::new(RefCell::new(Vec::new()));
    let opened_callback = opened_links.clone();
    app.on_open_source(move |url| opened_callback.borrow_mut().push(url.to_string()));
    let selections = Rc::new(RefCell::new(Vec::new()));
    let selected_callback = selections.clone();
    let weak = app.as_weak();
    app.on_choice_selected(move |index, id| {
        if let Some(app) = weak.upgrade() {
            if let Some((_, context)) = choices::select_choice(&app, index, &id) {
                selected_callback.borrow_mut().push(context);
            }
        }
    });
    let weak = app.as_weak();
    app.on_close_overlay(move || {
        if let Some(app) = weak.upgrade() {
            if app.get_overlay() == "filesystem" {
                app.invoke_finish_filesystem(app.get_fs_approved());
            } else if app.get_overlay() == "command" {
                app.invoke_command_decision(app.get_command_id(), false);
            } else if app.get_overlay() == "memory" {
                app.set_overlay("settings".into());
            } else {
                app.set_overlay("none".into());
            }
            if app.get_overlay() == "none" {
                app.invoke_focus_composer();
            }
        }
    });
    let weak = app.as_weak();
    app.on_open_settings(move || {
        if let Some(app) = weak.upgrade() {
            app.set_overlay("settings".into());
        }
    });
    app.show()?;

    snapshot(&app, &window, &out.join("welcome-desktop.png"), 1080, 800)?;
    snapshot(&app, &window, &out.join("welcome-wide.png"), 1366, 768)?;
    snapshot(&app, &window, &out.join("welcome-compact.png"), 480, 720)?;
    snapshot(&app, &window, &out.join("welcome-minimum.png"), 480, 600)?;
    app.set_tr(i18n::tr(i18n::Lang::En).to_slint());
    app.set_status(
        i18n::tr(i18n::Lang::En)
            .mcp_tools
            .replace("{}", "20")
            .into(),
    );
    snapshot(&app, &window, &out.join("welcome-english.png"), 1080, 800)?;
    snapshot(
        &app,
        &window,
        &out.join("welcome-english-wide.png"),
        1366,
        768,
    )?;
    app.set_tr(i18n::tr(i18n::Lang::Fr).to_slint());
    app.set_status(
        i18n::tr(i18n::Lang::Fr)
            .mcp_tools
            .replace("{}", "20")
            .into(),
    );

    messages.set_vec(vec![
        MessageRow { kind: "user".into(), text: "Peux-tu rendre mon bureau un peu plus aéré ?".into(), ..Default::default() },
        MessageRow { kind: "assistant".into(), text: "Bien sûr. Je vais essayer des espaces de 10 pixels entre vos fenêtres. Vous pourrez vérifier le résultat avant de conserver ce réglage.".into(), ..Default::default() },
        MessageRow { kind: "tool".into(), text: "hypr_set_option_live · general:gaps_in → 10\nAperçu appliqué. La configuration enregistrée est conservée.".into(), ..Default::default() },
        MessageRow { kind: "assistant".into(), text: "Voilà, votre bureau respire un peu plus. Souhaitez-vous conserver ce réglage ou revenir à la disposition précédente ?".into(), ..Default::default() },
    ]);
    snapshot(&app, &window, &out.join("conversation.png"), 1080, 800)?;
    snapshot(
        &app,
        &window,
        &out.join("conversation-compact.png"),
        480,
        720,
    )?;
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
    snapshot(&app, &window, &out.join("attachments.png"), 1080, 800)?;
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
        snapshot(&app, &window, &out.join(format!("{panel}.png")), 1080, 800)?;
        snapshot(
            &app,
            &window,
            &out.join(format!("{panel}-compact.png")),
            480,
            600,
        )?;
    }
    app.set_overlay("none".into());
    snapshot(&app, &window, &out.join("interaction-start.png"), 1080, 800)?;

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
    snapshot(&app, &window, &out.join("draft-ready.png"), 1080, 800)?;

    app.set_draft("".into());
    app.set_busy(true);
    click(&app, 300., 410.);
    assert!(
        app.get_draft().is_empty(),
        "les suggestions sont désactivées pendant un tour"
    );
    app.set_busy(false);
    app.set_overlay("settings".into());
    snapshot(&app, &window, &out.join("settings.png"), 1080, 800)?;
    click(&app, 540., 90.);
    assert_eq!(
        app.get_overlay().as_str(),
        "settings",
        "un clic dans le panneau ne doit pas le fermer"
    );
    // Une édition doit modifier la propriété lue par le bouton Enregistrer.
    click(&app, 295., 225.);
    assert!(
        !app.get_cfg_animate_avatar(),
        "la préférence d’animation doit être modifiable"
    );
    // Les onglets natifs changent de panneau au clic.
    click(&app, 445., 116.);
    assert_eq!(app.get_settings_tab(), 1);
    snapshot(&app, &window, &out.join("settings-access.png"), 1080, 800)?;
    click(&app, 570., 116.);
    assert_eq!(app.get_settings_tab(), 2);
    snapshot(
        &app,
        &window,
        &out.join("settings-assistant.png"),
        1080,
        800,
    )?;
    click(&app, 400., 190.);
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
    // Modèle -> endpoint en lecture seule -> tokens -> clé -> contexte SYSTEM.
    for _ in 0..4 {
        app.window().dispatch_event(WindowEvent::KeyPressed {
            text: slint::platform::Key::Tab.into(),
        });
        app.window().dispatch_event(WindowEvent::KeyReleased {
            text: slint::platform::Key::Tab.into(),
        });
    }
    let context = "You are my desktop assistant.\nUse short replies and accessible colors.";
    for ch in context.chars() {
        let text: slint::SharedString = if ch == '\n' {
            slint::platform::Key::Return.into()
        } else {
            ch.to_string().into()
        };
        app.window()
            .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        app.window()
            .dispatch_event(WindowEvent::KeyReleased { text });
    }
    assert_eq!(
        app.get_cfg_system_context().as_str(),
        context,
        "l'éditeur SYSTEM doit accepter le clavier et plusieurs lignes"
    );
    snapshot(
        &app,
        &window,
        &out.join("settings-system-context.png"),
        1080,
        800,
    )?;
    app.set_cfg_model("GLM-5.3".into());
    app.set_settings_error(app.get_tr().settings_invalid);
    snapshot(
        &app,
        &window,
        &out.join("settings-validation.png"),
        1080,
        800,
    )?;
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
    // Le Markdown est converti en HTML et en blocs natifs, avant comme pendant le streaming.
    app.set_overlay("none".into());
    app.set_follow_output(false);
    let formatted = "# Votre bureau, plus lisible\n\nDu **gras**, de l’*italique*, du `code en ligne` et un [lien Hyprland](https://wiki.hypr.land).\n\n## Les réglages proposés\n\n- **Espacement** : 10 pixels entre les fenêtres.\n- **Bordures** : un accent cyan discret.\n  - Les écrans restent inchangés.\n\n> Ces changements sont d’abord proposés à chaud.\n\n```lua\nhl.config({\n    general = { gaps_in = 10 },\n})\n```\n\n| Réglage | Avant | Après |\n|---|---|---|\n| Gaps | 5 px | **10 px** |\n| Bordures | 1 px | **2 px** |\n\n1. Essayer les réglages.\n2. Conserver uniquement si le résultat vous plaît.\n";
    messages.set_vec(vec![MessageRow {
        kind: "assistant".into(),
        text: formatted.into(),
        ..Default::default()
    }]);
    snapshot(&app, &window, &out.join("rich-response.png"), 1080, 1000)?;
    let row = messages.row_data(0).unwrap();
    assert!(row.html.contains("<h1>"));
    assert!(row.rich_blocks.iter().any(|block| block.kind == "table"));
    click(&app, 520., 194.);
    assert!(
        opened_links
            .borrow()
            .iter()
            .any(|url| url == "https://wiki.hypr.land"),
        "le lien du contenu mis en forme doit être cliquable"
    );
    snapshot(
        &app,
        &window,
        &out.join("rich-response-compact.png"),
        480,
        800,
    )?;
    app.set_follow_output(true);
    app.invoke_scroll_bottom();
    snapshot(
        &app,
        &window,
        &out.join("rich-response-bottom.png"),
        480,
        800,
    )?;
    assert!(
        app.invoke_is_at_bottom(),
        "le texte mis en forme conserve le suivi du scroll"
    );

    // Captures enregistrées à des instants précis : respiration, tête et paupières.
    messages.set_vec(Vec::new());
    app.invoke_scroll_bottom();
    app.set_animations_enabled(true);
    app.set_avatar_time(0.);
    snapshot(&app, &window, &out.join("avatar-open.png"), 1080, 800)?;
    app.set_avatar_time(1.05);
    snapshot(&app, &window, &out.join("avatar-breath.png"), 1080, 800)?;
    app.set_avatar_time(4.86);
    snapshot(&app, &window, &out.join("avatar-blink.png"), 1080, 800)?;
    let avatar_pixels = |name: &str| -> anyhow::Result<Vec<u8>> {
        Ok(image::open(out.join(name))?
            .crop_imm(480, 100, 120, 130)
            .to_rgba8()
            .into_raw())
    };
    assert!(avatar_pixels("avatar-open.png")? != avatar_pixels("avatar-blink.png")?);
    assert!(
        avatar_pixels("avatar-open.png")? != avatar_pixels("avatar-breath.png")?,
        "la respiration doit aussi rester visible avec le rendu logiciel"
    );
    app.set_avatar_time(-1.);
    snapshot(&app, &window, &out.join("avatar-live-a.png"), 1080, 800)?;
    // Le timer avance au rythme de la vraie boucle d'événements (20 Hz).
    let before = app.invoke_animation_time();
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        slint::platform::update_timers_and_animations();
    }
    snapshot(&app, &window, &out.join("avatar-live-b.png"), 1080, 800)?;
    assert!(app.invoke_animation_time() > before + 0.2);
    assert!(
        avatar_pixels("avatar-live-a.png")? != avatar_pixels("avatar-live-b.png")?,
        "l’horloge native doit animer la tête en temps réel"
    );
    app.set_animations_enabled(false);
    app.set_avatar_time(0.);
    snapshot(&app, &window, &out.join("avatar-disabled.png"), 1080, 800)?;
    app.set_avatar_time(4.86);
    snapshot(
        &app,
        &window,
        &out.join("avatar-disabled-later.png"),
        1080,
        800,
    )?;
    assert!(
        avatar_pixels("avatar-disabled.png")? == avatar_pixels("avatar-disabled-later.png")?,
        "désactiver l’animation doit figer l’avatar"
    );

    // L'activité apparaît avant le premier mot, suit la phase, puis disparaît.
    app.set_overlay("none".into());
    app.set_tr(i18n::tr(i18n::Lang::Fr).to_slint());
    messages.set_vec(vec![MessageRow {
        kind: "user".into(),
        text: "Propose-moi une ambiance pour mon bureau.".into(),
        ..Default::default()
    }]);
    app.set_busy(true);
    app.set_activity("thinking".into());
    snapshot(&app, &window, &out.join("activity-thinking.png"), 1080, 800)?;
    assert_eq!(app.invoke_activity_label(), app.get_tr().status_thinking);
    messages.push(MessageRow {
        kind: "assistant".into(),
        text: "Je vous propose trois ambiances…".into(),
        ..Default::default()
    });
    app.set_activity("writing".into());
    snapshot(&app, &window, &out.join("activity-writing.png"), 1080, 800)?;
    assert_eq!(app.invoke_activity_label(), app.get_tr().status_writing);
    app.set_activity("tools".into());
    snapshot(&app, &window, &out.join("activity-tools.png"), 480, 600)?;
    assert_eq!(app.invoke_activity_label(), app.get_tr().status_tools);
    app.set_tr(i18n::tr(i18n::Lang::En).to_slint());
    app.set_activity("thinking".into());
    snapshot(
        &app,
        &window,
        &out.join("activity-thinking-english.png"),
        1080,
        800,
    )?;
    app.set_busy(false);
    snapshot(&app, &window, &out.join("activity-complete.png"), 1080, 800)?;
    app.set_tr(i18n::tr(i18n::Lang::Fr).to_slint());

    // Régression : les fragments allongent une réponse au-delà du viewport.
    app.set_overlay("none".into());
    app.set_follow_output(true);
    let paragraph =
        "LYRA décrit le réglage en cours. Le texte doit rester visible pendant la génération.\n";
    for step in 1..=5 {
        let row = MessageRow {
            kind: "assistant".into(),
            text: paragraph.repeat(step * 45).into(),
            ..Default::default()
        };
        if step == 1 {
            messages.set_vec(vec![row]);
        } else {
            messages.set_row_data(0, row);
        }
        app.invoke_scroll_bottom();
        snapshot(&app, &window, &out.join("streaming-bottom.png"), 1080, 800)?;
        assert!(
            app.invoke_scroll_overflow() > 0.,
            "le scénario doit déborder"
        );
        assert!(
            app.invoke_is_at_bottom(),
            "fragment {step}: suivi={}, position={}, débordement={}",
            app.get_follow_output(),
            app.invoke_scroll_position(),
            app.invoke_scroll_overflow()
        );
    }
    snapshot(
        &app,
        &window,
        &out.join("streaming-bottom-compact.png"),
        480,
        600,
    )?;
    assert!(
        app.invoke_is_at_bottom(),
        "le suivi doit résister au retour à la ligne après redimensionnement"
    );

    // Une lecture manuelle au-dessus suspend le suivi jusqu'au retour en bas.
    app.window().dispatch_event(WindowEvent::PointerScrolled {
        position: slint::LogicalPosition::new(300., 200.),
        delta_x: 0.,
        delta_y: 250.,
    });
    std::thread::sleep(std::time::Duration::from_millis(220));
    snapshot(
        &app,
        &window,
        &out.join("streaming-manual-scroll.png"),
        480,
        600,
    )?;
    assert!(
        !app.get_follow_output(),
        "la lecture de l'historique doit suspendre le suivi"
    );
    messages.set_row_data(
        0,
        MessageRow {
            kind: "assistant".into(),
            text: paragraph.repeat(250).into(),
            ..Default::default()
        },
    );
    app.invoke_scroll_bottom();
    snapshot(
        &app,
        &window,
        &out.join("streaming-manual-scroll.png"),
        480,
        600,
    )?;
    assert!(
        !app.invoke_is_at_bottom(),
        "un fragment ne doit pas interrompre la lecture manuelle"
    );
    app.window().dispatch_event(WindowEvent::PointerScrolled {
        position: slint::LogicalPosition::new(300., 200.),
        delta_x: 0.,
        delta_y: -100_000.,
    });
    std::thread::sleep(std::time::Duration::from_millis(220));
    snapshot(&app, &window, &out.join("streaming-resumed.png"), 480, 600)?;
    assert!(
        app.invoke_is_at_bottom() && app.get_follow_output(),
        "revenir en bas doit reprendre le suivi"
    );

    // Les détails restent fermés, puis s'ouvrent au clic et se ferment par Entrée.
    messages.set_vec(vec![MessageRow {
        kind: "tool".into(),
        text: "hypr_monitors\nDP-1 : 2560 × 1440\n".repeat(35).into(),
        ..Default::default()
    }]);
    snapshot(&app, &window, &out.join("tools-collapsed.png"), 1080, 800)?;
    assert_eq!(app.invoke_scroll_overflow(), 0.);
    click(&app, 250., 124.);
    snapshot(&app, &window, &out.join("tools-expanded.png"), 1080, 800)?;
    assert!(
        app.invoke_scroll_overflow() > 0.,
        "le clic doit révéler les détails"
    );
    let expanded_height = app.invoke_scroll_overflow();
    messages.set_row_data(
        0,
        MessageRow {
            kind: "tool".into(),
            text: "Résultat détaillé\n".repeat(120).into(),
            ..Default::default()
        },
    );
    snapshot(&app, &window, &out.join("tools-expanded.png"), 1080, 800)?;
    assert!(
        app.invoke_scroll_overflow() > expanded_height,
        "une mise à jour conserve le bloc ouvert"
    );
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Return.into(),
    });
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Return.into(),
    });
    snapshot(&app, &window, &out.join("tools-collapsed.png"), 1080, 800)?;
    assert_eq!(app.invoke_scroll_overflow(), 0.);

    // Palettes réelles du catalogue : source, clic, valeur exacte et double clic.
    messages.set_vec(Vec::new());
    let catalog = choices::palette_catalog();
    let request = choices::ChoiceRequest::parse(&serde_json::json!({
        "question": "Quelle ambiance préférez-vous pour votre bureau ?", "options": catalog["palettes"],
    }).to_string())?;
    app.invoke_prepare_choice();
    messages.push(request.to_row());
    snapshot(&app, &window, &out.join("choices-palettes.png"), 1080, 800)?;
    app.invoke_choice_selected(0, "inconnu".into());
    assert!(
        selections.borrow().is_empty(),
        "un identifiant inconnu est ignoré"
    );
    click(&app, 350., 185.);
    assert_eq!(
        selections.borrow().len(),
        1,
        "un clic doit transmettre le choix"
    );
    let selection: serde_json::Value = serde_json::from_str(&selections.borrow()[0])?;
    assert_eq!(selection["id"], "graphite-cyan");
    assert_eq!(selection["colors"][2], "#00ADB5");
    app.invoke_choice_selected(0, "ocean".into());
    assert_eq!(
        selections.borrow().len(),
        1,
        "une proposition utilisée ne se renvoie pas"
    );
    snapshot(&app, &window, &out.join("choices-selected.png"), 1080, 800)?;

    let request = choices::ChoiceRequest::parse(
        r#"{"question":"Quel style de fenêtres souhaitez-vous ?","options":[{"id":"compact","label":"Compact","description":"Petits espaces, bordures discrètes","value":"gaps_in=4;rounding=4"},{"id":"airy","label":"Aéré","description":"Plus d’espace entre les fenêtres","value":"gaps_in=12;rounding=12"}]}"#,
    )?;
    messages.set_vec(vec![request.to_row()]);
    snapshot(&app, &window, &out.join("choices-text.png"), 1080, 800)?;
    choices::finish_pending_choices(&app);
    assert_eq!(
        messages.row_data(0).unwrap().choice_state.as_str(),
        "skipped"
    );
    assert!(choices::select_choice(&app, 0, "airy").is_none());

    let diagram = |vertical: bool| {
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="360" height="180" viewBox="0 0 360 180"><rect width="360" height="180" rx="12" fill="#101319"/><rect x="{}" y="{}" width="130" height="65" rx="6" fill="#1C323B" stroke="#7BDACD" stroke-width="2"/><rect x="{}" y="{}" width="130" height="65" rx="6" fill="#24384B" stroke="#8BB6E8" stroke-width="2"/></svg>"##,
            if vertical { 115 } else { 30 },
            if vertical { 15 } else { 55 },
            if vertical { 115 } else { 195 },
            if vertical { 100 } else { 55 }
        )
    };
    let request = choices::ChoiceRequest::parse(
        &serde_json::json!({"question":"Comment disposer les écrans ?","options":[
            {"id":"horizontal","label":"Côte à côte","value":"horizontal","svg":diagram(false)},
            {"id":"vertical","label":"Superposés","value":"vertical","svg":diagram(true)}
        ]})
        .to_string(),
    )?;
    let row = request.to_row();
    assert!(
        row.choices.row_data(0).unwrap().preview.size().width > 0,
        "le SVG doit être rendu"
    );
    messages.set_vec(vec![row]);
    snapshot(&app, &window, &out.join("choices-diagrams.png"), 1080, 800)?;
    snapshot(
        &app,
        &window,
        &out.join("choices-diagrams-compact.png"),
        480,
        720,
    )?;

    let request = choices::ChoiceRequest::parse(&serde_json::json!({"question":"Quel aperçu préférez-vous ?","options":[
        {"id":"lyra","label":"LYRA","image_path":format!("{}/assets/lyra-avatar.png", env!("CARGO_MANIFEST_DIR"))},
        {"id":"diagram","label":"Disposition","svg":diagram(false)}
    ]}).to_string())?;
    messages.set_vec(vec![request.to_row()]);
    snapshot(&app, &window, &out.join("choices-images.png"), 1080, 800)?;

    // Les propositions sont aussi utilisables au clavier et sur petite fenêtre.
    let request = choices::ChoiceRequest::parse(
        r#"{"question":"Which style?","options":[{"id":"compact","label":"Compact"},{"id":"airy","label":"Airy"}]}"#,
    )?;
    messages.set_vec(vec![request.to_row()]);
    app.set_tr(i18n::tr(i18n::Lang::En).to_slint());
    app.set_follow_output(false);
    snapshot(
        &app,
        &window,
        &out.join("choices-english-compact.png"),
        480,
        720,
    )?;
    app.invoke_focus_composer();
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
    assert_eq!(
        selections.borrow().len(),
        2,
        "les options doivent être activables au clavier"
    );

    // Consentement FS : refus et fermeture restent fermés, seule une action
    // explicite autorise, et la révocation ne ferme pas les réglages.
    app.set_tr(i18n::tr(i18n::Lang::Fr).to_slint());
    app.set_busy(false);
    app.set_overlay("none".into());
    app.set_fs_approved(false);
    let decisions = Rc::new(RefCell::new(Vec::new()));
    let captured = decisions.clone();
    let weak_fs = app.as_weak();
    app.on_filesystem_decision(move |approved| {
        captured.borrow_mut().push(approved);
        if let Some(app) = weak_fs.upgrade() {
            app.invoke_finish_filesystem(approved);
        }
    });
    app.invoke_request_filesystem();
    snapshot(
        &app,
        &window,
        &out.join("filesystem-consent.png"),
        1080,
        800,
    )?;
    assert!(!app.get_fs_approved());
    click(&app, 625., 577.);
    assert_eq!(
        &*decisions.borrow(),
        &[false],
        "refuser ne doit pas autoriser l'accès"
    );
    assert!(!app.get_fs_approved());
    assert_eq!(app.get_overlay(), "none");
    app.invoke_request_filesystem();
    snapshot(
        &app,
        &window,
        &out.join("filesystem-consent.png"),
        1080,
        800,
    )?;
    click(&app, 735., 577.);
    assert_eq!(
        &*decisions.borrow(),
        &[false, true],
        "autorisation explicite par le bouton"
    );
    assert!(app.get_fs_approved());
    app.set_overlay("settings".into());
    app.invoke_filesystem_decision(false);
    assert!(!app.get_fs_approved());
    assert_eq!(
        app.get_overlay(),
        "settings",
        "la révocation conserve le panneau"
    );
    app.invoke_request_filesystem();
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Escape.into(),
    });
    assert_eq!(app.get_overlay(), "settings");
    assert!(!app.get_fs_approved(), "fermer sans choisir n'accorde rien");
    app.set_overlay("none".into());
    app.set_tr(i18n::tr(i18n::Lang::En).to_slint());
    app.invoke_request_filesystem();
    snapshot(
        &app,
        &window,
        &out.join("filesystem-consent-compact.png"),
        480,
        600,
    )?;
    app.invoke_close_overlay();

    // Commandes : chaque clic porte sur l'identifiant courant ; fermer et
    // Échap refusent. L'affichage reste lisible sur une petite fenêtre.
    let command_decisions = Rc::new(RefCell::new(Vec::new()));
    let captured = command_decisions.clone();
    let weak = app.as_weak();
    app.on_command_decision(move |id, approved| {
        if let Some(app) = weak.upgrade() {
            if app.get_overlay() == "command" && app.get_command_id() == id {
                captured.borrow_mut().push((id, approved));
                app.set_overlay(app.get_command_return_overlay());
                app.set_command_return_overlay("none".into());
            }
        }
    });
    app.set_tr(i18n::tr(i18n::Lang::Fr).to_slint());
    app.set_command_id(1);
    app.set_command_text("hyprctl version -j".into());
    app.set_command_reason(
        "Vérifier la version de Hyprland pour consulter la documentation correspondante.".into(),
    );
    app.set_busy(true);
    app.set_activity("approval".into());
    app.set_overlay("command".into());
    snapshot(
        &app,
        &window,
        &out.join("command-confirmation.png"),
        1080,
        800,
    )?;
    click(&app, 675., 637.);
    assert_eq!(&*command_decisions.borrow(), &[(1, false)]);
    app.set_command_id(2);
    app.set_overlay("command".into());
    snapshot(
        &app,
        &window,
        &out.join("command-confirmation.png"),
        1080,
        800,
    )?;
    click(&app, 750., 637.);
    assert_eq!(&*command_decisions.borrow(), &[(1, false), (2, true)]);
    app.set_command_id(3);
    app.set_overlay("command".into());
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Escape.into(),
    });
    assert_eq!(
        &*command_decisions.borrow(),
        &[(1, false), (2, true), (3, false)]
    );
    app.set_tr(i18n::tr(i18n::Lang::En).to_slint());
    app.set_command_id(4);
    app.set_command_reason(
        "Check the installed Hyprland version before consulting the matching documentation.".into(),
    );
    app.set_overlay("command".into());
    snapshot(
        &app,
        &window,
        &out.join("command-confirmation-compact.png"),
        480,
        600,
    )?;
    app.invoke_close_overlay();
    assert_eq!(command_decisions.borrow().last(), Some(&(4, false)));

    app.set_command_id(5);
    app.set_overlay("command".into());
    snapshot(
        &app,
        &window,
        &out.join("command-confirmation-keyboard.png"),
        1080,
        800,
    )?;
    for key in [slint::platform::Key::Tab, slint::platform::Key::Return] {
        app.window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
        app.window()
            .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
    }
    assert_eq!(
        command_decisions.borrow().last(),
        Some(&(5, true)),
        "la confirmation est accessible avec Tab puis Entrée"
    );

    // Le sélecteur d'abonnement fonctionne aussi sans souris.
    app.set_command_id(6);
    app.set_command_return_overlay("settings".into());
    app.set_overlay("command".into());
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Escape.into(),
    });
    assert_eq!(
        app.get_overlay(),
        "settings",
        "refuser une requête d’inventaire doit revenir aux réglages"
    );
    app.invoke_close_overlay();
    messages.set_vec(Vec::new());
    app.set_tr(i18n::tr(i18n::Lang::Fr).to_slint());
    app.set_draft("".into());
    app.set_provider_index(0);
    app.set_model_label("".into());
    app.set_busy(false);
    app.set_overlay("none".into());
    app.invoke_scroll_bottom();
    snapshot(&app, &window, &out.join("provider-zcode.png"), 1080, 800)?;
    let providers = Rc::new(RefCell::new(Vec::new()));
    let changed = providers.clone();
    app.on_provider_selected(move |index| changed.borrow_mut().push(index));
    app.invoke_focus_composer();
    app.set_draft("keep this draft".into());
    *text_clipboard.borrow_mut() = "pasted text".into();
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
    app.invoke_paste_text();
    assert_eq!(
        app.get_draft(),
        "pasted text",
        "le retour au collage natif de texte doit conserver la sélection"
    );
    app.set_draft("keep this draft".into());
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Control.into(),
    });
    app.window()
        .dispatch_event(WindowEvent::KeyPressed { text: "v".into() });
    app.window()
        .dispatch_event(WindowEvent::KeyReleased { text: "v".into() });
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Control.into(),
    });
    assert_eq!(
        &*pasted.borrow(),
        &[false],
        "Ctrl+V demande un collage sans envoyer le message"
    );
    assert_eq!(app.get_draft(), "keep this draft");
    app.set_paste_in_progress(true);
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Return.into(),
    });
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Return.into(),
    });
    assert_eq!(
        app.get_draft(),
        "keep this draft",
        "Entrée ne doit pas perdre le brouillon pendant la lecture"
    );
    app.set_paste_in_progress(false);
    app.set_draft("".into());
    for key in [
        slint::platform::Key::Tab,
        slint::platform::Key::Tab,
        slint::platform::Key::Tab,
        slint::platform::Key::DownArrow,
        slint::platform::Key::DownArrow,
    ] {
        app.window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
        app.window()
            .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
    }
    assert_eq!(
        &*providers.borrow(),
        &[1, 2],
        "sélection Claude Code puis Codex au clavier"
    );
    assert_eq!(app.get_provider_index(), 2);
    snapshot(
        &app,
        &window,
        &out.join("provider-codex-compact.png"),
        480,
        600,
    )?;
    app.set_busy(true);
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::UpArrow.into(),
    });
    app.window().dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::UpArrow.into(),
    });
    assert_eq!(
        app.get_provider_index(),
        2,
        "pas de changement de fournisseur pendant un tour"
    );
    app.set_busy(false);

    // Variantes du formulaire : les clients natifs n'ont pas les champs Zcode
    // dont la largeur masquait auparavant le rétrécissement du formulaire.
    app.set_cfg_system_context("".into());
    app.set_environment_text(serde_json::to_string_pretty(&serde_json::json!({
        "schema":2,"os":"linux","username":"desktop-user", "package_managers":["pacman","flatpak"],
        "desktop_tools":[{"name":"example-panel", "manager":"pacman", "version":"1.2.3-1", "version_source":"installed_package", "config_files":["~/.config/example-panel/config.toml"], "referenced_by":["~/.config/hypr/hyprland.conf"]}],
        "inventory_sources":[{"manager":"pacman","status":"database_read","packages":1400}]
    }))?.into());
    app.set_clients_claude_ok(true);
    app.set_clients_zcode_ok(true);
    for (name, provider, lang, approved, width, height) in [
        ("zcode-wide", 0, i18n::Lang::Fr, true, 1080, 800),
        ("claude-wide", 1, i18n::Lang::En, true, 1080, 800),
        ("codex", 2, i18n::Lang::Fr, true, 680, 800),
        ("codex-compact", 2, i18n::Lang::Fr, false, 480, 600),
        ("claude-compact", 1, i18n::Lang::En, false, 480, 600),
    ] {
        app.set_overlay("none".into());
        snapshot(
            &app,
            &window,
            &out.join("settings-transition.png"),
            width,
            height,
        )?;
        app.set_tr(i18n::tr(lang).to_slint());
        app.set_provider_index(provider);
        app.set_fs_approved(approved);
        app.set_cfg_model(if provider == 0 { "GLM-5.3" } else { "" }.into());
        app.set_overlay("settings".into());
        snapshot(
            &app,
            &window,
            &out.join(format!("settings-{name}.png")),
            width,
            height,
        )?;
        app.window().dispatch_event(WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(width as f32 / 2., height as f32 / 2.),
            delta_x: 0.,
            delta_y: -2000.,
        });
        std::thread::sleep(std::time::Duration::from_millis(220));
        snapshot(
            &app,
            &window,
            &out.join(format!("settings-{name}-bottom.png")),
            width,
            height,
        )?;
    }

    app.set_overlay("none".into());
    app.set_tr(i18n::tr(i18n::Lang::Fr).to_slint());
    app.set_activity("tools".into());
    app.set_busy(true);
    app.set_task_label("Paquets installés (pacman) · 700/1400".into());
    app.set_task_progress(0.5);
    app.set_task_indeterminate(false);
    snapshot(
        &app,
        &window,
        &out.join("inventory-progress.png"),
        1080,
        800,
    )?;
    app.set_overlay("settings".into());
    snapshot(
        &app,
        &window,
        &out.join("inventory-progress-settings.png"),
        480,
        600,
    )?;
    app.set_busy(false);
    app.set_task_label("".into());
    app.set_overlay("memory".into());
    snapshot(
        &app,
        &window,
        &out.join("memory-confirmation.png"),
        480,
        600,
    )?;
    app.window().dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Escape.into(),
    });
    assert_eq!(
        app.get_overlay().as_str(),
        "settings",
        "Annuler l’effacement revient aux réglages"
    );

    app.set_busy(false);
    app.set_overlay("settings".into());
    for lang in [i18n::Lang::Fr, i18n::Lang::En] {
        app.set_tr(i18n::tr(lang).to_slint());
        for tab in 0..4 {
            app.set_settings_tab(tab);
            for (width, height) in [(1080, 800), (480, 600)] {
                snapshot(
                    &app,
                    &window,
                    &out.join(format!(
                        "settings-tabs-{}-{tab}-{width}.png",
                        if lang == i18n::Lang::Fr { "fr" } else { "en" }
                    )),
                    width,
                    height,
                )?;
            }
        }
        app.set_settings_tab(1);
        for mode in 0..4 {
            app.set_cfg_operation_mode(mode);
            snapshot(
                &app,
                &window,
                &out.join(format!(
                    "settings-mode-{}-{mode}.png",
                    if lang == i18n::Lang::Fr { "fr" } else { "en" }
                )),
                480,
                600,
            )?;
        }
    }
    // Focus initial du dialogue -> fermer -> barre d'onglets native.
    app.set_overlay("none".into());
    snapshot(
        &app,
        &window,
        &out.join("settings-keyboard-transition.png"),
        480,
        600,
    )?;
    app.set_settings_tab(0);
    app.set_overlay("settings".into());
    snapshot(
        &app,
        &window,
        &out.join("settings-keyboard-start.png"),
        480,
        600,
    )?;
    for key in [
        slint::platform::Key::Tab,
        slint::platform::Key::Tab,
        slint::platform::Key::RightArrow,
        slint::platform::Key::Return,
    ] {
        app.window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
        app.window()
            .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
    }
    assert_eq!(
        app.get_settings_tab(),
        1,
        "les onglets doivent être accessibles au clavier"
    );
    app.set_cfg_operation_mode(0);
    for key in [
        slint::platform::Key::Tab,
        slint::platform::Key::Tab,
        slint::platform::Key::DownArrow,
    ] {
        app.window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
        app.window()
            .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
    }
    assert_eq!(
        app.get_cfg_operation_mode(),
        1,
        "le mode doit pouvoir être choisi au clavier"
    );
    app.set_overlay("none".into());
    messages.set_vec(vec![MessageRow {
        kind: "assistant".into(),
        text: "Check the installed package version (command to copy; not executed).\n\n```sh\npacman -Q example-panel\n```".into(),
        ..Default::default()
    }]);
    app.set_follow_output(true);
    snapshot(&app, &window, &out.join("manual-commands.png"), 480, 600)?;
    println!(
        "Captures écrites dans {} ; interactions vérifiées.",
        out.display()
    );
    Ok(())
}
