//! LYRA — fenêtre native (Slint) cliente chatbot du panneau de configuration
//! Hyprland. Boucle d'agent **LangChain Rust** (module claude, streaming),
//! exécution des outils du daemon via MCP.

mod agent;
mod avatar;
mod chat_config;
mod choices;
mod filesystem;
mod i18n;
mod mcp;
mod providers;
mod rich_text;
mod scope;
mod ui;

use anyhow::Context;

slint::include_modules!();

const USAGE: &str = "\
LYRA — assistante de configuration Wayland (hyprpanel-chat)

USAGE
  hyprpanel-chat           Ouvre la fenêtre (config chargée/créée si absente)
  hyprpanel-chat init      Crée ~/.config/hyprpanel/chat.toml (clé reprise de
                           la config Zcode si disponible) et affiche l'état
  hyprpanel-chat --help    Cette aide
  hyprpanel-chat check-providers [--live] [--scope]
                           Vérifie les connexions ; --live teste les trois
                           abonnements et les choix sans modifier le bureau
                           --scope teste le périmètre par de vrais appels
";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("init") => return cmd_init(),
        Some("check-providers") => {
            let cfg = chat_config::ChatConfig::load().map_err(anyhow::Error::msg)?;
            return tokio::runtime::Runtime::new()?.block_on(providers::diagnose(
                &cfg,
                args.iter().any(|a| a == "--live"),
                args.iter().any(|a| a == "--scope"),
            ));
        }
        Some("--help") | Some("-h") | Some("help") => {
            print!("{USAGE}");
            return Ok(());
        }
        Some(other) => {
            eprintln!("argument inconnu : {other}\n\n{USAGE}");
            std::process::exit(2);
        }
        None => {}
    }

    let (cfg, _) = chat_config::ChatConfig::load_or_init()
        .map_err(|e| anyhow::anyhow!("configuration du chat : {e}"))?;
    let fs_backups = chat_config::state_history_path()
        .parent()
        .context("dossier de données")?
        .join("file-backups");
    let filesystem = std::sync::Arc::new(
        filesystem::FilesystemAccess::new(filesystem::FilesystemAccess::default_root(), fs_backups)
            .protect_directory(
                chat_config::config_path()
                    .parent()
                    .context("dossier des réglages")?
                    .to_path_buf(),
            ),
    );

    let app = ChatWindow::new().context("création de la fenêtre (session Wayland active ?)")?;
    avatar::install(&app);
    app.set_provider_index(cfg.provider.index());
    app.set_model_label(cfg.model_label().into());
    app.set_animations_enabled(cfg.animate_avatar);
    app.set_app_version(env!("CARGO_PKG_VERSION").into());
    app.set_tr(i18n::tr(cfg.lang()).to_slint());
    app.set_fs_root(filesystem.root().to_string_lossy().to_string().into());
    app.set_fs_approved(false);
    {
        let model: slint::ModelRc<MessageRow> =
            std::rc::Rc::new(slint::VecModel::<MessageRow>::from(Vec::new())).into();
        app.set_messages(model);
        let chips: slint::ModelRc<AttachmentRow> =
            std::rc::Rc::new(slint::VecModel::<AttachmentRow>::from(Vec::new())).into();
        app.set_attachments(chips);
        let history: slint::ModelRc<HistoryRow> =
            std::rc::Rc::new(slint::VecModel::<HistoryRow>::from(Vec::new())).into();
        app.set_history(history);
    }
    // Statut clients externes pour le panneau de réglages.
    {
        let regs = hyprpanel_core::detect_clients();
        app.set_clients_claude_ok(regs.claude_code);
        app.set_clients_zcode_ok(regs.zcode);
        if let Ok(settings) = hyprpanel_core::Settings::load() {
            let snippets = hyprpanel_core::registration_snippets(&settings.token, settings.port);
            app.set_clients_claude_cmd(snippets.claude_code.into());
            app.set_clients_zcode_cmd(snippets.zcode.into());
        }
    }

    // Miroir Rust des pièces jointes (le modèle Slint n'est que l'affichage).
    let pending_files: std::sync::Arc<std::sync::Mutex<Vec<std::path::PathBuf>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    // UI → agent : canal non borné ; l'agent tourne dans un runtime tokio
    // hébergé par un thread dédié, la boucle d'événements Slint garde le main.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<agent::UserEvent>();
    {
        let fs = filesystem.clone();
        let tx_fs = tx.clone();
        let weak = app.as_weak();
        app.on_filesystem_decision(move |approved| {
            fs.set_approved(approved);
            if let Some(app) = weak.upgrade() {
                app.invoke_finish_filesystem(approved);
                app.set_status(if approved {
                    app.get_tr().fs_granted
                } else {
                    app.get_tr().fs_denied
                });
                if app.get_overlay() == "none" {
                    app.invoke_focus_composer();
                }
            }
            let _ = tx_fs.send(agent::UserEvent::FilesystemChanged);
        });
    }
    {
        let tx_send = tx.clone();
        let tx_reset = tx.clone();
        let tx_restore = tx.clone();
        let tx_cfg = tx.clone();
        let tx_choice = tx.clone();
        let weak_choice = app.as_weak();
        app.on_choice_selected(move |index, id| {
            let Some(app) = weak_choice.upgrade() else {
                return;
            };
            if app.get_busy() || app.get_overlay() != "none" {
                return;
            }
            let Some((display, context)) = choices::select_choice(&app, index, &id) else {
                return;
            };
            app.set_activity("thinking".into());
            app.set_busy(true);
            app.set_follow_output(true);
            if let Err(error) = tx_choice.send(agent::UserEvent::Choice { display, context }) {
                app.set_busy(false);
                ui::append(&weak_choice, "error", error.to_string());
            }
        });
        app.on_open_source(move |url| {
            if rich_text::safe_link(url.as_str()) {
                let url = url.to_string();
                std::thread::spawn(move || {
                    let _ = std::process::Command::new("xdg-open").arg(url).status();
                });
            }
        });

        let files_for_send = pending_files.clone();
        let weak_send = app.as_weak();
        app.on_send(move |text| {
            let Some(app) = weak_send.upgrade() else {
                return;
            };
            if app.get_busy() {
                return;
            }
            let files: Vec<_> =
                std::mem::take(&mut *files_for_send.lock().expect("pièces empoisonnées"));
            if text.trim().is_empty() && files.is_empty() {
                return;
            }
            choices::finish_pending_choices(&app);
            app.set_activity("thinking".into());
            app.set_busy(true);
            ui::attachments_clear(&weak_send);
            if let Err(error) = tx_send.send(agent::UserEvent::Input {
                text: text.trim().to_string(),
                files,
            }) {
                app.set_busy(false);
                ui::append(
                    &weak_send,
                    "error",
                    format!("{} : {error}", app.get_tr().label_error),
                );
            }
        });

        let weak_reset = app.as_weak();
        let files_for_reset = pending_files.clone();
        app.on_new_conversation(move || {
            files_for_reset.lock().expect("pièces empoisonnées").clear();
            ui::attachments_clear(&weak_reset);
            if let Some(app) = weak_reset.upgrade() {
                app.set_draft("".into());
                app.invoke_focus_composer();
            }
            let _ = tx_reset.send(agent::UserEvent::Reset);
        });

        app.on_restore_requested(move |backup| {
            if !backup.is_empty() {
                let _ = tx_restore.send(agent::UserEvent::Restore(backup.to_string()));
            }
        });

        // ── Réglages ────────────────────────────────────────────────────
        let weak_settings = app.as_weak();
        let cfg_for_settings = std::sync::Arc::new(std::sync::Mutex::new(cfg.clone()));
        let cfg_for_provider = cfg_for_settings.clone();
        let tx_provider = tx.clone();
        let weak_provider = app.as_weak();
        app.on_provider_selected(move |index| {
            let Some(app) = weak_provider.upgrade() else {
                return;
            };
            let mut saved = cfg_for_provider.lock().expect("cfg empoisonnée");
            if app.get_busy() {
                app.set_provider_index(saved.provider.index());
                return;
            }
            let Some(provider) = chat_config::Provider::from_index(index) else {
                return;
            };
            let mut next = saved.clone();
            next.provider = provider;
            if let Err(error) = next.save() {
                app.set_provider_index(saved.provider.index());
                ui::append(&weak_provider, "error", format!("config : {error}"));
                return;
            }
            app.set_provider_index(provider.index());
            app.set_model_label(next.model_label().into());
            let _ = tx_provider.send(agent::UserEvent::ConfigUpdated(Box::new(next.clone())));
            *saved = next;
            app.invoke_focus_composer();
        });
        let cfg_for_open = cfg_for_settings.clone();
        app.on_open_settings(move || {
            let weak = weak_settings.clone();
            let cfg = cfg_for_open.lock().expect("cfg empoisonnée").clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(app) = weak.upgrade() else { return };
                app.set_cfg_lang_index(if cfg.lang() == i18n::Lang::En { 1 } else { 0 });
                app.set_cfg_model(cfg.active_model().into());
                app.set_cfg_base_url(chat_config::ZCODE_SUBSCRIPTION_URL.into());
                app.set_cfg_max_tokens(cfg.max_tokens.to_string().into());
                app.set_cfg_api_key(cfg.api_key.clone().into());
                app.set_cfg_animate_avatar(cfg.animate_avatar);
                app.set_cfg_system_context(cfg.system_context.clone().into());
                app.set_settings_error("".into());
                app.set_overlay("settings".into());
            });
        });

        let weak_save = app.as_weak();
        app.on_save_settings(move || {
            let Some(app) = weak_save.upgrade() else {
                return;
            };
            let mut cfg = cfg_for_settings.lock().expect("cfg empoisonnée").clone();
            cfg.language = if app.get_cfg_lang_index() == 1 {
                "en".into()
            } else {
                "fr".into()
            };
            cfg.animate_avatar = app.get_cfg_animate_avatar();
            cfg.system_context = app.get_cfg_system_context().trim().to_string();
            if cfg.system_context.chars().count() > chat_config::MAX_SYSTEM_CONTEXT_CHARS {
                app.set_settings_error(app.get_tr().settings_context_invalid.clone());
                return;
            }
            cfg.set_active_model(app.get_cfg_model().trim().to_string());
            // Zcode est un accès Coding Plan : l'endpoint n'est pas un choix API.
            cfg.base_url = chat_config::ZCODE_SUBSCRIPTION_URL.into();
            if cfg.provider == chat_config::Provider::Zcode
                && (cfg.model.is_empty() || cfg.base_url.is_empty())
            {
                app.set_settings_error(app.get_tr().settings_invalid.clone());
                return;
            }
            cfg.max_tokens = match app.get_cfg_max_tokens().trim().parse::<u32>() {
                Ok(n) if n >= 256 && n <= 200_000 => n,
                _ => {
                    app.set_settings_error(app.get_tr().settings_invalid.clone());
                    return;
                }
            };
            // clé : vide = inchangée (le champ password est prérempli, mais
            // on accepte une saisie vide comme « ne pas changer »)
            let key = app.get_cfg_api_key().trim().to_string();
            if !key.is_empty() {
                cfg.api_key = key;
            }
            match cfg.save() {
                Ok(()) => {
                    *cfg_for_settings.lock().expect("cfg empoisonnée") = cfg.clone();
                    app.set_model_label(cfg.model_label().into());
                    app.set_animations_enabled(cfg.animate_avatar);
                    let _ = tx_cfg.send(agent::UserEvent::ConfigUpdated(Box::new(cfg)));
                    app.set_overlay("none".into());
                    app.invoke_focus_composer();
                }
                Err(e) => {
                    app.set_settings_error(format!("config : {e}").into());
                }
            }
        });

        let weak_close = app.as_weak();
        app.on_close_overlay(move || {
            if let Some(app) = weak_close.upgrade() {
                if app.get_overlay() == "filesystem" {
                    app.invoke_finish_filesystem(app.get_fs_approved());
                } else {
                    app.set_overlay("none".into());
                }
                if app.get_overlay() == "none" {
                    app.invoke_focus_composer();
                }
            }
        });

        // ── Sélecteur de fichiers (zenity) ──────────────────────────────
        let files_for_pick = pending_files.clone();
        let weak_pick = app.as_weak();
        app.on_attach_clicked(move || {
            let files_for_pick = files_for_pick.clone();
            let weak_pick = weak_pick.clone();
            std::thread::spawn(move || {
                let output = std::process::Command::new("zenity")
                    .args([
                        "--file-selection",
                        "--multiple",
                        "--separator=\n",
                        "--title=Joindre des fichiers à LYRA",
                    ])
                    .output();
                let Ok(out) = output else { return };
                if !out.status.success() {
                    return; // annulé par l'utilisateur
                }
                let chosen = String::from_utf8_lossy(&out.stdout);
                for line in chosen.lines().filter(|l| !l.is_empty()) {
                    let path = std::path::PathBuf::from(line);
                    files_for_pick
                        .lock()
                        .expect("pièces empoisonnées")
                        .push(path.clone());
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| line.to_string());
                    ui::attachment_push(&weak_pick, &name, &agent::attachment_meta(&path));
                }
            });
        });

        let files_for_remove = pending_files.clone();
        let weak_remove = app.as_weak();
        app.on_remove_attachment(move |idx| {
            let idx = idx as usize;
            let mut guard = files_for_remove.lock().expect("pièces empoisonnées");
            if idx < guard.len() {
                guard.remove(idx);
                ui::attachment_remove(&weak_remove, idx);
            }
        });
    }

    let weak = app.as_weak();
    std::thread::Builder::new()
        .name("lyra-agent".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("runtime tokio : {e}");
                    return;
                }
            };
            runtime.block_on(agent::run(cfg, rx, weak, filesystem));
        })
        .context("thread agent")?;

    app.invoke_request_filesystem();
    app.run().context("boucle d'événements Slint")?;
    Ok(())
}

fn cmd_init() -> anyhow::Result<()> {
    match chat_config::ChatConfig::init() {
        Ok((cfg, created)) => {
            let path = chat_config::config_path();
            if created {
                println!("configuration créée : {}", path.display());
                if cfg.api_key.is_empty() {
                    println!(
                        "api_key : absente (aucune clé Z.ai trouvée) — renseignez-la dans le fichier"
                    );
                } else {
                    println!("api_key : reprise de la config Zcode (non affichée)");
                }
                println!("modèle : {} — endpoint : {}", cfg.model, cfg.base_url);
            } else {
                println!("configuration existante : {}", path.display());
            }
            Ok(())
        }
        Err(e) => anyhow::bail!("init impossible : {e}"),
    }
}
