//! hyprpanel-chat — fenêtre native (Slint) cliente chatbot du panneau de
//! configuration Hyprland. Boucle d'agent intégrée (protocole Anthropic
//! Messages, streaming) qui exécute les outils du daemon via MCP.

mod agent;
mod chat_config;
mod mcp;
mod ui;

use anyhow::Context;

slint::include_modules!();

const USAGE: &str = "\
hyprpanel-chat — fenêtre de chat du panneau de configuration Hyprland

USAGE
  hyprpanel-chat           Ouvre la fenêtre (config chargée/créée si absente)
  hyprpanel-chat init      Crée ~/.config/hyprpanel/chat.toml (clé reprise de
                           la config Zcode si disponible) et affiche l'état
  hyprpanel-chat --help    Cette aide
";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("init") => return cmd_init(),
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

    let app = ChatWindow::new().context("création de la fenêtre (session Wayland active ?)")?;
    app.set_model_label(cfg.model.clone().into());
    {
        let model: slint::ModelRc<MessageRow> =
            std::rc::Rc::new(slint::VecModel::<MessageRow>::from(Vec::new())).into();
        app.set_messages(model);
        let chips: slint::ModelRc<AttachmentRow> =
            std::rc::Rc::new(slint::VecModel::<AttachmentRow>::from(Vec::new())).into();
        app.set_attachments(chips);
    }

    // Miroir Rust des pièces jointes (le modèle Slint ne s'itère pas depuis
    // les callbacks ; les deux restent synchronisés sous ce mutex).
    let pending_files: std::sync::Arc<std::sync::Mutex<Vec<std::path::PathBuf>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    // UI → agent : canal non borné ; l'agent tourne dans un runtime tokio
    // hébergé par un thread dédié, la boucle d'événements Slint garde le main.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<agent::UserEvent>();
    {
        let tx_reset = tx.clone();
        let files_for_send = pending_files.clone();
        let weak_send = app.as_weak();
        app.on_send(move |text| {
            let files: Vec<_> = std::mem::take(&mut *files_for_send.lock().expect("pièces empoisonnées"));
            if text.trim().is_empty() && files.is_empty() {
                return;
            }
            ui::attachments_clear(&weak_send);
            let _ = tx.send(agent::UserEvent::Input { text: text.trim().to_string(), files });
        });
        app.on_new_conversation(move || {
            let _ = tx_reset.send(agent::UserEvent::Reset);
        });

        // Sélecteur de fichiers (zenity) dans un thread dédié — il bloque
        // jusqu'à la fermeture de la boîte de dialogue.
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
                    files_for_pick.lock().expect("pièces empoisonnées").push(path.clone());
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| line.to_string());
                    ui::attachment_push(&weak_pick, &name, &agent::attachment_meta(&path));
                }
            });
        });

        let files_for_remove = pending_files.clone();
        app.on_remove_attachment(move |idx| {
            let idx = idx as usize;
            let mut guard = files_for_remove.lock().expect("pièces empoisonnées");
            if idx < guard.len() {
                guard.remove(idx);
            }
        });
    }

    let weak = app.as_weak();
    std::thread::Builder::new()
        .name("hyprpanel-agent".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("runtime tokio : {e}");
                    return;
                }
            };
            runtime.block_on(agent::run(cfg, rx, weak));
        })
        .context("thread agent")?;

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
                    println!("api_key : absente (aucune clé Z.ai trouvée) — renseignez-la dans le fichier");
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
