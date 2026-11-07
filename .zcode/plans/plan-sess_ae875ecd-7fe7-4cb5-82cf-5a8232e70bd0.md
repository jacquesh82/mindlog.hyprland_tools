# Panneau de configuration Hyprland — v1 « chatbot MCP »

## Contexte

Nouveau projet dans la ferme `mindlog.today` : un **panneau de configuration Hyprland piloté par chatbot**. L'utilisateur dialogue avec Claude Code ou ZCode ; l'agent agit sur le compositeur via un **serveur MCP** (daemon Rust local) qui expose l'état du système, applique des changements à chaud, valide et sauvegarde la config. Cible : Hyprland 0.57.1 avec **config Lua native** (`~/.config/hypr/hyprland.lua`, API `hl.*`), où `monitors.lua`/`inputs.lua` sont **générés par hyprdmc** (lecture seule pour nous).

Décisions actées avec l'utilisateur : daemon **HTTP + jeton Bearer** (auth minimaliste), stack **Rust + rmcp** (Slint réservé à une v2, architecture le permettant), modèle « **MCP état/live, agent édite** » (le serveur ne réécrit pas le Lua), interface **chatbot seul** (pas de page web).

## Architecture

Daemon `hyprpanel` (bin Rust, service systemd user, pattern `hyprdmc.service` existant) :

- **IPC Hyprland direct** : socket Unix `/run/user/$UID/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock` (protocole `j/…`, `keyword …`, `dispatch …`, `reload`, `configerrors`, `notify …`) — zéro sous-processus. Si la variable d'env est absente (cas des services systemd), scan de `/run/user/$UID/hypr/` pour l'instance la plus récente.
- **Événements** : connexion à `.socket2.sock` → tâche tokio qui alimente un ring buffer (~200 événements horodatés).
- **Transport MCP** : `rmcp` 3.x, `StreamableHttpService` (Tower) monté dans **Axum** sur `127.0.0.1:8765/mcp` (port configurable, vérifié libre à l'install).
- **Auth minimaliste** : jeton hex 256 bits dans `~/.config/hyprpanel/config.toml` (chmod 600), vérifié par middleware Axum — `Authorization: Bearer <token>` **ou** fallback `?key=<token>` en query (au cas où un client MCP ne saurait pas envoyer de headers). Comparaison en temps constant. Bind localhost uniquement. `/health` sans auth (ne divulgue rien).

## Structure du projet

```
mindlog.hyprland_tools/
├── Cargo.toml                    # workspace
├── .gitignore                    # target/
├── CLAUDE.md                     # notes projet (convention frères)
├── README.md
├── crates/
│   ├── hyprpanel-core/           # lib réutilisable par la v2 Slint
│   │   └── src/{lib,ipc,events,hypr_files,backup,settings}.rs
│   └── hyprpanel-server/         # bin `hyprpanel`
│       └── src/{main,tools,auth,state}.rs
├── assets/hyprpanel.service      # modèle systemd (Wants/PartOf=graphical-session.target)
└── scripts/install.sh
```

`hyprpanel-core` : client IPC socket, watcher socket2, gestion des fichiers de config (chemins, fichiers « appartenant à hyprdmc »), backup/restore horodatés, parsing du `config.toml`. `hyprpanel-server` : sous-commandes `init` (génère config + jeton), `serve` (rmcp + axum), `check` (diagnostic socket/config), `token` (affiche le jeton) ; les outils MCP (`#[tool]` rmcp) et le middleware auth. Les *server instructions* rmcp documentent la mise en page de la config (Lua `hl.*`, monitors/inputs = read-only, workflow live→édit→reload).

## Outils MCP exposés (v1)

- **État** : `hypr_status`, `hypr_monitors`, `hypr_workspaces`, `hypr_clients`, `hypr_devices`, `hypr_get_option` (getoption JSON), `hypr_binds` (tronqué si volumineux)
- **Live** : `hypr_set_option_live` (keyword — aperçu non persistant), `hypr_dispatch`, `hypr_notify` (retour visuel à l'écran)
- **Persistance assistée** : `hypr_config_files` (chemins + flags read-only), `hypr_find_setting` (où une option est définie dans les .lua/.conf, file:line), `hypr_backup` / `hypr_list_backups` / `hypr_restore_backup`
- **Validation** : `hypr_reload` (retourne les configerrors), `hypr_config_errors`
- **Observabilité** : `hypr_recent_events`, `hypr_screenshot` (via `grim` si présent → chemin PNG lisible par l'agent)

## Installation & enregistrement

`scripts/install.sh` : `cargo build --release` → binaire vers `~/.local/bin/hyprpanel` → `hyprpanel init` si pas de config → unit systemd user → `daemon-reload` + `enable --now` → affiche le jeton et les commandes d'enregistrement prêtes à coller :
- Claude Code : `claude mcp add --transport http hyprpanel http://127.0.0.1:8765/mcp --header "Authorization: Bearer <token>"` (scope user)
- ZCode : entrée `mcp.servers.hyprpanel = { url: "http://127.0.0.1:8765/mcp?key=<token>" }` dans `~/.zcode/cli/config.json`

L'enregistrement réel sera fait avec votre accord pendant la vérification.

## Conventions dépôt

- `git init` dans `mindlog.hyprland_tools/` + ajout de `mindlog.hyprland_tools/` au `.gitignore` du monorepo (pattern récent « rejoint les sous-dépôts ignorés ») + commit type `docs: mindlog.hyprland_tools rejoint les sous-dépôts ignorés`
- **Pas de conteneur** (outil desktop local : socket Hyprland + fichiers de config), hors contrat images INFRA.md — statut d'outil hôte, comme hyprdmc
- Fin de session : rédiger `CONTEXT.md` (< 20 lignes) selon la politique du monorepo

## Hors périmètre v1

Fenêtre Slint (v2, crate séparé réutilisant `hyprpanel-core`), page web de statut, édition du Lua par le serveur, conteneurisation, gestion de wayle/waybar/rofi (lecture seule au mieux via `hypr_config_files`).

## Vérification

1. `cargo test` — unitaires : protocole socket (messages bien formés), backup/restore (répertoires temporaires), parsing config.toml, middleware auth (401 sans jeton / mauvais jeton, 200 avec, query param)
2. `hyprpanel check` puis `hyprpanel serve` en avant-plan : `curl` sans jeton → 401 ; avec jeton → handshake MCP (initialize) OK
3. Service systemd : `systemctl --user status hyprpanel`, health OK après reboot de session (scan socket)
4. Enregistrement ZCode + Claude Code → les outils apparaissent
5. **E2E chat réel** : demander à l'agent « augmente les gaps intérieurs à 10, rends-le persistant » → il doit faire `hypr_set_option_live`, éditer `hyprland.lua`, `hypr_reload`, vérifier `hypr_config_errors` vide, constater via `hypr_screenshot` ; puis restauration du backup

## Références

- [StreamableHttpService (docs.rs rmcp)](https://docs.rs/rmcp/latest/rmcp/transport/streamable_http_server/tower/struct.StreamableHttpService.html)
- [Build an MCP Server in Rust with rmcp — ESEO Space](https://eseospace.com/blog/build-mcp-server-rust)
- [Streamable HTTP MCP Server in Rust — Shuttle](https://shuttle.dev/blog/build-mcp-server-rust)