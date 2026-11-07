# CLAUDE.md — mindlog.hyprland_tools (hyprpanel)

Panneau de configuration Hyprland piloté par chatbot : daemon Rust + serveur
MCP (HTTP streamable + jeton) **et** fenêtre cliente Slint (`hyprpanel-chat`,
boucle d'agent Anthropic Messages en streaming). Config Hyprland cible :
**Lua** (`hl.*`), 0.57.x.

## Règles du projet

- `crates/hyprpanel-core` ne dépend d'aucune couche réseau : réutilisé par le
  serveur MCP (`hyprpanel-server`) et la fenêtre (`hyprpanel-chat`).
- **Jamais d'édition de `~/.config/hypr/monitors.lua` ni `inputs.lua`** :
  générés par hyprdmc (github.com/jacquesh82/hyprdmc).
- Le serveur ne réécrit pas la config : aperçu à chaud (`hl.dsp.global("conf …")`)
  + validation (`reload`, `configerrors`) + backups ; l'agent édite le Lua.
- `getoption`/`hl.get_config` = état fichiers, pas les overrides runtime.
- `hypr_set_option_live` répond `ok` même pour une option inconnue : l'effet
  doit être vérifié (screenshot/événements).
- `hyprpanel-chat` est **GPL-3.0-only** (Slint communauté) ; core et server
  restent MIT. Ne pas déplacer du code du chat vers un crate MIT.
- La clé API du chat vit dans `~/.config/hyprpanel/chat.toml` (600) : ne
  jamais l'afficher, la logger ou la committer.

## Commandes

```bash
cargo build && cargo test        # workspace (core + server + chat)
./scripts/install.sh             # release + ~/.local/bin + unit systemd user
hyprpanel {init|serve|check|token}
hyprpanel-chat {init|}           # fenêtre de chat (config via init)
journalctl --user -u hyprpanel   # logs du daemon
```

## Conventions

- Code et messages d'outil en français, identifiants en anglais.
- Toute nouvelle commande IPC passe par `hyprpanel-core::ipc` avec un test de
  construction du message (voir `ipc::tests`).
- SSE/LLM : la machine à états (`agent::apply_event`) reste pure et testée ;
  l'accès UI passe par `ui.rs` (invoke_from_event_loop uniquement).
- Outil hôte local : pas de Docker, pas de déploiement (hors contrat INFRA.md).
- Sous-dépôt git indépendant, listé dans le `.gitignore` du monorepo
  mindlog.today (pattern « mindlog.* sous-dépôts ignorés »).
