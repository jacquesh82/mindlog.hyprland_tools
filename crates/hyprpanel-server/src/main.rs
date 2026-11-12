//! hyprpanel — panneau de configuration Hyprland piloté par chatbot.
//!
//! Daemon MCP en HTTP streamable sur 127.0.0.1, protégé par jeton statique.
//! Sous-commandes : `init` (génère la config + le jeton), `serve` (daemon),
//! `check` (diagnostic), `token` (affiche le jeton).

mod auth;
mod state;
mod tools;

use std::sync::Arc;

use hyprpanel_core::{Settings, events, ipc};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::state::AppState;
use crate::tools::Panel;

const USAGE: &str = "\
hyprpanel — panneau de configuration Hyprland (serveur MCP)

USAGE
  hyprpanel init [--force]   Crée ~/.config/hyprpanel/config.toml avec un jeton frais
  hyprpanel serve            Démarre le serveur MCP (HTTP + jeton, 127.0.0.1)
  hyprpanel check            Diagnostic : IPC, config, backups, grim, port
  hyprpanel token            Affiche le jeton d'authentification
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("init") => cmd_init(args.iter().any(|a| a == "--force")),
        Some("serve") => cmd_serve(),
        Some("check") => cmd_check(),
        Some("token") => cmd_token(),
        Some("--help") | Some("-h") | Some("help") | None => {
            print!("{USAGE}");
            0
        }
        Some(other) => {
            eprintln!("sous-commande inconnue : {other}\n\n{USAGE}");
            2
        }
    };
    std::process::exit(code);
}

fn cmd_init(force: bool) -> i32 {
    match Settings::init(force) {
        Ok((s, created)) => {
            let path = s.config_path();
            if created {
                println!("configuration créée : {}", path.display());
                println!("jeton : {}", s.token);
                println!("\nProchaine étape : `hyprpanel serve` puis enregistrez le serveur MCP");
                println!(
                    "  Claude Code : claude mcp add --transport http hyprpanel http://127.0.0.1:{}/mcp --header \"Authorization: Bearer {}\"",
                    s.port, s.token
                );
                println!(
                    "  ZCode (~/.zcode/cli/config.json, mcp.servers) : {{ \"url\": \"http://127.0.0.1:{}/mcp?key={}\" }}",
                    s.port, s.token
                );
            } else {
                println!("configuration existante (inchangée) : {}", path.display());
                println!(
                    "jeton : {}  (utilisez `hyprpanel init --force` pour le régénérer)",
                    s.token
                );
            }
            0
        }
        Err(e) => {
            eprintln!("init impossible : {e}");
            1
        }
    }
}

fn cmd_token() -> i32 {
    match Settings::load() {
        Ok(s) => {
            println!("{}", s.token);
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn cmd_check() -> i32 {
    let mut failures = 0;

    // IPC Hyprland
    match ipc::instance_signature()
        .map_err(|e| e.to_string())
        .and_then(|_| ipc::query_json("version").map_err(|e| e.to_string()))
    {
        Ok(v) => println!(
            "✓ IPC Hyprland joignable (version {})",
            v["version"].as_str().unwrap_or("?")
        ),
        Err(e) => {
            println!("✗ IPC Hyprland : {e}");
            failures += 1;
        }
    }

    match Settings::load() {
        Ok(s) => {
            let files = hyprpanel_core::hypr_files::scan(&s.hypr_config_dir);
            match (s.hypr_config_dir.is_dir(), files.is_empty()) {
                (true, false) => println!(
                    "✓ config Hyprland : {} ({} fichiers)",
                    s.hypr_config_dir.display(),
                    files.len()
                ),
                (true, true) => {
                    println!("✗ config Hyprland vide : {}", s.hypr_config_dir.display());
                    failures += 1;
                }
                _ => {
                    println!(
                        "✗ répertoire de config introuvable : {}",
                        s.hypr_config_dir.display()
                    );
                    failures += 1;
                }
            }
            std::fs::create_dir_all(&s.backups_dir)
                .map(|_| println!("✓ backups : {}", s.backups_dir.display()))
                .map_err(|e| {
                    println!("✗ backups {e}");
                    failures += 1;
                })
                .ok();
            match std::net::TcpListener::bind(("127.0.0.1", s.port)) {
                Ok(_) => println!("✓ port {} disponible", s.port),
                Err(_) => {
                    println!("✗ port {} occupé (le serveur tourne déjà ?)", s.port);
                    failures += 1;
                }
            }
            let grim_ok = which("grim");
            let grim_note = if grim_ok {
                "présent"
            } else {
                "absent — hypr_screenshot sera indisponible"
            };
            println!(
                "{} grim (captures) : {grim_note}",
                if grim_ok { "✓" } else { "⚠" }
            );
            // clients externes enregistrés auprès du panneau
            let regs = hyprpanel_core::detect_clients();
            println!(
                "{} Claude Code : {}",
                if regs.claude_code { "✓" } else { "✗" },
                if regs.claude_code {
                    "enregistré"
                } else {
                    "non enregistré (commande affichée par install.sh)"
                }
            );
            println!(
                "{} ZCode : {}",
                if regs.zcode { "✓" } else { "✗" },
                if regs.zcode {
                    "enregistré"
                } else {
                    "non enregistré (~/.zcode/cli/config.json, mcp.servers)"
                }
            );
        }
        Err(e) => {
            println!("✗ configuration du démon : {e} (lancez `hyprpanel init`)");
            failures += 1;
        }
    }

    if failures == 0 {
        println!("tout est prêt : `hyprpanel serve`");
        0
    } else {
        1
    }
}

/// Équivalent minimal de `which` : parcours du PATH.
fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).exists()))
        .unwrap_or(false)
}

fn cmd_serve() -> i32 {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let settings = match Settings::load() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("runtime tokio : {e}");
            return 1;
        }
    };

    match runtime.block_on(serve(settings)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("serve : {e:#}");
            1
        }
    }
}

async fn serve(settings: Settings) -> anyhow::Result<()> {
    let port = settings.port;
    let token = Arc::new(settings.token.clone());
    let state = AppState::new(settings);

    // Watcher des événements Hyprland (reconnexion automatique).
    tokio::spawn(events::watch_forever(state.events.clone()));

    // Service MCP : une instance Panel par session, toutes clones bon marché.
    let factory_state = state.clone();
    let service: StreamableHttpService<Panel, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(Panel::new(factory_state.clone())),
        std::sync::Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default().with_json_response(true),
    );

    let mcp = axum::Router::new().nest_service("/mcp", service).layer(
        axum::middleware::from_fn_with_state(token, auth::middleware),
    );
    let app = mcp.route("/health", axum::routing::get(|| async { "ok" }));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!("hyprpanel à l'écoute sur http://127.0.0.1:{port}/mcp (auth par jeton)");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
