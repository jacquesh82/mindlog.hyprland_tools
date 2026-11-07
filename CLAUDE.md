# CLAUDE.md — mindlog.hyprland_tools (hyprpanel)

Panneau de configuration Hyprland piloté par chatbot : daemon Rust + serveur
MCP (HTTP streamable + jeton). Config Hyprland cible : **Lua** (`hl.*`), 0.57.x.

## Règles du projet

- `crates/hyprpanel-core` ne dépend d'aucune couche réseau : réutilisable par
  la v2 Slint. Le serveur MCP vit dans `crates/hyprpanel-server`.
- **Jamais d'édition de `~/.config/hypr/monitors.lua` ni `inputs.lua`** :
  générés par hyprdmc (github.com/jacquesh82/hyprdmc).
- Le serveur ne réécrit pas la config : aperçu à chaud (`hl.dsp.global("conf …")`)
  + validation (`reload`, `configerrors`) + backups ; l'agent édite le Lua.
- `getoption`/`hl.get_config` = état fichiers, pas les overrides runtime.
- `hypr_set_option_live` répond `ok` même pour une option inconnue : l'effet
  doit être vérifié (screenshot/événements).

## Commandes

```bash
cargo build && cargo test        # workspace (core + server)
./scripts/install.sh             # release + ~/.local/bin + unit systemd user
hyprpanel {init|serve|check|token}
journalctl --user -u hyprpanel   # logs
```

## Conventions

- Code et messages d'outil en français, identifiants en anglais.
- Toute nouvelle commande IPC passe par `hyprpanel-core::ipc` avec un test de
  construction du message (voir `ipc::tests`).
- Outil hôte local : pas de Docker, pas de déploiement (hors contrat INFRA.md).
- Sous-dépôt git indépendant, listé dans le `.gitignore` du monorepo
  mindlog.today (pattern « mindlog.* sous-dépôts ignorés »).
