# CLAUDE.md — mindlog.hyprland_tools (hyprpanel)

Panneau de configuration Hyprland piloté par chatbot : daemon Rust + serveur
MCP (HTTP streamable + jeton) **et** fenêtre cliente Slint **LYRA**
(`hyprpanel-chat`, boucle d'agent Anthropic Messages en streaming, pièces
jointes images/texte). Config Hyprland cible : **Lua** (`hl.*`), 0.57.x.

## Règles du projet

- `crates/hyprpanel-core` ne dépend d'aucune couche réseau : réutilisé par le
  serveur MCP (`hyprpanel-server`) et la fenêtre (`hyprpanel-chat`).
- **Jamais d'édition de `~/.config/hypr/monitors.lua` ni `inputs.lua`** :
  générés par un outil externe.
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
- LLM : le transport passe par **langchain-rust** (`Claude`, `Message`,
  `FunctionDefinition`) — pas de reqwest direct. La boucle d'exécution (streaming
  UI, pont MCP, timeouts, registre) reste locale et fine.
- i18n : toute chaîne UI passe par `src/i18n.rs` (struct `Tr` + test de parité
  FR/EN). Le system prompt suit `chat_config.language`.
- Pièces jointes : classification/blocs Anthropic dans `agent.rs`
  (`classify_attachment`, `build_user_content`) — purs et testés ; sélecteur
  zenity dans `main.rs` (thread dédié). Le miroir Rust des chips
  (`Arc<Mutex<Vec<PathBuf>>>`) fait foi, le modèle Slint n'est que l'affichage.
- Outil hôte local : pas de Docker, pas de déploiement (hors contrat INFRA.md).
- Sous-dépôt git indépendant, listé dans le `.gitignore` du monorepo
  mindlog.today (pattern « mindlog.* sous-dépôts ignorés »).
