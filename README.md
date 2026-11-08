# hyprpanel — panneau de configuration Hyprland par chatbot

Daemon Rust local qui expose la configuration de [Hyprland](https://hypr.land/)
(config **Lua**, API `hl.*`, testé sur 0.57.1) à un agent conversationnel
(Claude Code, ZCode) via un serveur **MCP** en HTTP streamable, protégé par un
jeton. L'agent peut lire l'état du compositeur, tester des réglages à chaud,
éditer la config, valider, sauvegarder/restaurer et constater visuellement.

## Architecture

- `crates/hyprpanel-core` — cœur réutilisable : client IPC socket Unix
  (`j/…`, `/repl`, `/dispatch`, notify…), watcher d'événements `.socket2.sock`,
  inventaire des fichiers de config, backups horodatés, réglages.
- `crates/hyprpanel-server` — binaire `hyprpanel` : serveur MCP
  ([rmcp](https://crates.io/crates/rmcp) 3.x + Axum) sur `127.0.0.1:8765/mcp`,
  auth `Authorization: Bearer <jeton>` ou `?key=<jeton>` (comparaison en temps
  constant). Une v2 Slint s'appuierait sur `hyprpanel-core`.

Le serveur ne réécrit jamais la config : il fournit état + aperçu à chaud +
validation + backups ; l'agent édite `~/.config/hypr/hyprland.lua` avec ses
outils fichiers, puis `hypr_reload`. `monitors.lua`/`inputs.lua` (générés par
[hyprdmc](https://github.com/jacquesh82/hyprdmc)) sont signalés lecture seule.

## Outils MCP (20)

| Domaine | Outils |
|---|---|
| État | `hypr_status`, `hypr_monitors`, `hypr_workspaces`, `hypr_clients`, `hypr_devices`, `hypr_binds` |
| Live | `hypr_set_option_live` (ex-`hyprctl keyword`), `hypr_dispatch`, `hypr_notify` |
| Persistance assistée | `hypr_config_files`, `hypr_find_setting`, `hypr_backup`, `hypr_list_backups`, `hypr_restore_backup` |
| Validation | `hypr_reload`, `hypr_config_errors` |
| Observation | `hypr_get_option`, `hypr_eval` (Lua dans le compositeur), `hypr_recent_events`, `hypr_screenshot` (grim) |

## Installation et usage

```bash
./scripts/install.sh   # build release, ~/.local/bin, config + jeton, unit systemd user
hyprpanel check        # diagnostic IPC / config / port / grim
hyprpanel token        # jeton pour l'enregistrement
```

Enregistrement (affiché aussi par `install.sh`) :

```bash
# Claude Code
claude mcp add --scope user --transport http hyprpanel http://127.0.0.1:8765/mcp \
  --header "Authorization: Bearer <JETON>"
# ZCode — ~/.zcode/cli/config.json
"mcp": { "servers": { "hyprpanel": { "url": "http://127.0.0.1:8765/mcp?key=<JETON>" } } }
```

Puis, dans le chatbot : « passe les gaps intérieurs à 10 et rends-le
persistant ». Le workflow attendu (test à chaud → édition Lua → reload →
validation → capture) est transmis aux clients via les *server instructions*.

## Notes Hyprland 0.57 (vérifié empiriquement)

- `hyprctl keyword` n'existe plus : le setter à chaud est
  `/dispatch hl.dsp.global("conf <option> <valeur>")` — silencieusement ignoré
  si l'option est inconnue, d'où vérification visuelle conseillée.
- `getoption`/`hl.get_config` lisent l'état **fichier**, pas les overrides
  runtime.
- `/repl <expr Lua>` renvoie la valeur de l'expression : canal de lecture
  structuré (`hl.get_monitors()`, `hl.get_config(…)`, …).

## Fenêtre de chat native — **LYRA** (`hyprpanel-chat`)

Client chatbot graphique en **Slint** (crate `crates/hyprpanel-chat`, GPL-3.0
car Slint est lié sous licence communautaire) : **LYRA — Wayland Configuration
Assistant**, habillage cyberpunk (fond `#0B0F1A`, wordmark cyan/magenta,
badge pill), fenêtre où l'on discute directement avec le panneau.

- **Boucle d'agent intégrée** : protocole Anthropic Messages en **streaming**
  (SSE), endpoint par défaut `https://api.z.ai/api/anthropic` (GLM-5.3),
  configurable dans `~/.config/hyprpanel/chat.toml` (chmod 600 ; `init` reprend
  la clé de la config Zcode si présente).
- **Client MCP** du daemon (rmcp, jeton en query) : les 20 outils du panneau
  sont exposés au LLM ; les appels et résultats s'affichent en bulles « outil ».
- System prompt = règles de chat + instructions du serveur reçues au handshake.

### Interface complète (v2.2)

- **Accueil** avec explication et exemples cliquables ; **aide** (?), **à propos**,
  **réglages** (⚙ : langue FR/EN, modèle, endpoint, tokens, clé, statut des clients
  externes) — bulle d'information si Claude Code ou ZCode n'est pas enregistré
  (commandes prêtes à coller ; `hyprpanel check` affiche aussi ce statut).
- **i18n FR/EN** : interface, prompt système et exemples (`language` dans chat.toml).
- **Historique de configuration** (⟲) : une sauvegarde précède chaque message
  (`lyra-turn-NNN` via hypr_backup) ; « restaurer » remet la config Hyprland à
  l'état d'avant un prompt donné (hypr_restore_backup + reload). Registre persistant
  dans ~/.local/state/hyprpanel/lyra-history.json.
- **Agent sur LangChain Rust** (module claude) : transport SSE, tool use natif et
  messages multimodaux portés par le crate — la boucle d'exécution (streaming UI,
  pont MCP, timeouts) reste locale.

### Envoi de fichiers

Bouton **« ＋ fichier »** (sélecteur `zenity`, sélection multiple) ; les pièces
apparaissent en chips amovibles au-dessus de la saisie, envoyées avec le
message :

- images `png/jpeg/webp/gif` (≤ 5 Mo) → blocs `image` base64 pour le LLM
- fichiers texte (md, lua, conf, toml, json, rs, py, sh…) ≤ 64 Ko → contenu
  inliné délimité
- autres types → note explicative, non transmis (PDF non supporté en v1 ;
  si l'API refuse les images, l'erreur s'affiche en bulle)

```bash
hyprpanel-chat init   # crée la config (clé Z.ai reprise silencieusement)
hyprpanel-chat        # ouvre la fenêtre
# ex. raccourci dans hyprland.lua :
#   hl.bind(mainMod.."+Y", hl.dsp.exec_cmd("hyprpanel-chat"))
```

## Périmètre

Outil hôte local (socket Hyprland + fichiers de config) : ni conteneur ni
déploiement distant, hors contrat images INFRA.md. v1 = serveur MCP pour
Claude Code/ZCode ; v2 = fenêtre Slint cliente LYRA (`hyprpanel-chat`).
Historique de conversation non persistant, un seul serveur MCP, pièces
jointes sans glisser-déposer.
