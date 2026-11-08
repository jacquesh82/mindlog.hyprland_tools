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
outils fichiers, puis `hypr_reload`. `monitors.lua`/`inputs.lua` (générés par un outil externe) sont signalés lecture seule.

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
Assistant**, interface sombre graphite et accent menthe. Son avatar original
apparaît dans l'en-tête, l'accueil et les réponses. La conversation est limitée
à une colonne de 800 px pour rester lisible dans une fenêtre maximisée.

- **Boucle d'agent intégrée** : protocole Anthropic Messages en **streaming**
  (SSE), endpoint par défaut `https://api.z.ai/api/anthropic` (GLM-5.3),
  configurable dans `~/.config/hyprpanel/chat.toml` (chmod 600 ; `init` reprend
  la clé de la config Zcode si présente).
- **Client MCP** du daemon (rmcp, jeton en query) : les 20 outils du panneau
  sont exposés au LLM ; les appels et résultats s'affichent en bulles « outil ».
- System prompt = règles de chat + instructions du serveur reçues au handshake.

### Interface complète (v2.2)

- **Accueil** avec quatre suggestions en grille : un clic prépare le message,
  puis Entrée ou « Envoyer » le transmet ; **aide** (?), **à propos**,
  **réglages** (⚙ : langue FR/EN, modèle, endpoint, tokens, clé, statut des clients
  externes). L'enregistrement MCP de ces applications externes est facultatif
  pour le chat LYRA ; `hyprpanel check` affiche aussi ce statut.
- **i18n FR/EN** : interface, prompt système et exemples (`language` dans chat.toml).
- **Historique de configuration** (⟲) : une sauvegarde précède chaque message
  (`lyra-turn-NNN` via hypr_backup) ; « restaurer » remet la config Hyprland à
  l'état d'avant un prompt donné (hypr_restore_backup + reload). Registre persistant
  dans ~/.local/state/hyprpanel/lyra-history.json.
- **Zcode sur LangChain Rust** (module claude) : transport SSE, tool use natif et
  messages multimodaux portés par le crate — la boucle d'exécution (streaming UI,
  pont MCP, timeouts) reste locale.
- **Sélecteur d'abonnement** près d'« Envoyer » : Zcode (Coding Plan Z.ai),
  Claude Code et Codex. Le choix est mémorisé (`provider` dans `chat.toml`),
  la conversation et les outils restent disponibles après le changement.
  Claude Code utilise la session `claude auth login`, Codex celle de `codex login`
  (ChatGPT). Les clés API et passerelles héritées sont exclues pour ces deux modes,
  sans lire ni copier leurs jetons. Les modèles CLI ont leurs champs indépendants
  `claude_model` et `codex_model` ; vide = modèle par défaut du client officiel.
  Les CLI renvoient des réponses structurées validées ; LYRA exécute elle-même
  les outils et gère les choix cliquables. Les réponses CLI sont affichées après
  validation, avec l'indicateur de réflexion pendant l'attente.
  Interfaces utilisées : [Claude Code programmatique](https://code.claude.com/docs/en/headless)
  et [Codex non interactif](https://learn.chatgpt.com/docs/non-interactive-mode).
- **Suivi du texte** : le chat reste en bas pendant la génération ; remonter
  manuellement suspend ce suivi, revenir en bas le réactive. Les détails des
  outils sont regroupés derrière un lien repliable, fermé par défaut.
- **Activité visible** : un indicateur dans la conversation affiche « LYRA
  réfléchit… » avant sa réponse, « LYRA écrit… » pendant le streaming, puis
  l'activité des outils. Il disparaît à la fin du tour, y compris en cas d'erreur
  ou lorsque LYRA attend un choix.
- **Réponses mises en forme** : conversion du Markdown en HTML nettoyé,
  puis affichage natif des titres, listes imbriquées, emphases, citations,
  tableaux, liens et blocs de code. Le rendu suit le streaming ; les commandes
  conservent leur contenu exact. Les liens HTTP/HTTPS et mailto s'ouvrent au clic.
  Les images distantes restent des liens, sans téléchargement automatique.
- **Avatar animé** : respiration légère, inclinaisons de tête et clignements
  des yeux. Seuls les avatars d'accueil, d'en-tête et d'activité bougent ; ceux
  des anciens messages restent fixes. Les animations s'arrêtent quand la fenêtre
  est minimisée, pendant les panneaux modaux et pendant la frappe (reprise après
  250 ms). Une horloge partagée limite l'animation à 20 images/s et les textures
  sont préparées en 256 × 256 au build. Le rendu des fragments SSE est regroupé
  à 30 mises à jour/s maximum. L'option « Animer l’avatar de
  LYRA » dans les réglages est conservée dans `chat.toml` (`animate_avatar`).

### Choix cliquables dans le chat

LYRA dispose de deux outils locaux, en complément des outils MCP :

- `lyra_offer_choices` présente une question et **2 à 8 options** : textes,
  couleurs ou palettes, images locales PNG/JPEG/WebP/GIF, ou schémas SVG autonomes.
  Un clic renvoie le choix exact à l'agent et reprend la conversation. Les
  propositions précédentes sont désactivées après un choix ou une réponse écrite.
- `lyra_palette_catalog` fournit une première sélection de palettes issues de
  [Color Hunt](https://colorhunt.co/), avec leurs codes hexadécimaux et liens
  sources. C'est un catalogue local de départ, pas une recherche en temps réel.
  L'agent peut aussi proposer d'autres palettes et en citer la source.

Exemples : « Propose-moi trois ambiances de couleurs », « Fais-moi choisir un
style de fenêtres » ou « Montre-moi des schémas pour disposer mes écrans ».
Le choix reste une préférence : les règles de confirmation avant application
ou persistance continuent à s'appliquer.

### Envoi de fichiers

Bouton **« Joindre »** (sélecteur `zenity`, sélection multiple) ; les pièces
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

### Vérification de l'interface sans session graphique

```bash
cargo run -p hyprpanel-chat --example preview --offline
cargo run -p hyprpanel-chat --example typing_perf --offline
# Vérification locale des connexions (sans appel de modèle)
cargo run -p hyprpanel-chat -- check-providers
# Test réel des trois abonnements et des choix, sans modifier Hyprland
cargo run -p hyprpanel-chat -- check-providers --live
```

Cet exemple rend les vrais composants Slint, sans lire la configuration ni
appeler le LLM ou Hyprland. Il écrit les captures dans `target/lyra-previews/`
(accueil de 480 à 1366 px, conversation, pièces jointes et panneaux FR/EN)
et vérifie les interactions clavier et souris, le streaming long, les blocs
repliables, les valeurs des choix et leur désactivation. Le prompt de l'avatar est
conservé dans `crates/hyprpanel-chat/assets/README.md`.

## Périmètre

Outil hôte local (socket Hyprland + fichiers de config) : ni conteneur ni
déploiement distant, hors contrat images INFRA.md. v1 = serveur MCP pour
Claude Code/ZCode ; v2 = fenêtre Slint cliente LYRA (`hyprpanel-chat`).
Historique de conversation non persistant, un seul serveur MCP, pièces
jointes sans glisser-déposer.
