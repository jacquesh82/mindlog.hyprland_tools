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
    }

    // UI → agent : canal non borné ; l'agent tourne dans un runtime tokio
    // hébergé par un thread dédié, la boucle d'événements Slint garde le main.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<agent::UserEvent>();
    {
        let tx_reset = tx.clone();
        app.on_send(move |text| {
            let text = text.trim().to_string();
            if !text.is_empty() {
                let _ = tx.send(agent::UserEvent::Text(text));
            }
        });
        app.on_new_conversation(move || {
            let _ = tx_reset.send(agent::UserEvent::Reset);
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
