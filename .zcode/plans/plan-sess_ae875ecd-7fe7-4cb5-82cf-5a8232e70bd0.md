# hyprpanel-chat — fenêtre Slint cliente chatbot du panneau

## Contexte

Suite de hyprpanel v0.1.0 : l'utilisateur veut maintenant la v2 annoncée — une **UI native Slint en Rust qui est elle-même le client chatbot**. On discute dans la fenêtre ; une boucle d'agent intégrée parle à un LLM (protocole **Anthropic Messages**, streaming token par token), qui appelle les 20 outils du panneau via un **client MCP** (rmcp) connecté au daemon existant `http://127.0.0.1:8765/mcp`. Décisions actées : streaming dès la v1, liaison par MCP (pas in-process), clé Z.ai copiée vers la config du chat (GLM-5.3 via `https://api.z.ai/api/anthropic`).

## Nouveau crate : `crates/hyprpanel-chat` (bin `hyprpanel-chat`)

**License `GPL-3.0-only`** (licence communautaire de Slint — les crates existants restent MIT).

```
crates/hyprpanel-chat/
├── Cargo.toml        # slint 1.18, slint-build, rmcp {client, transport-streamable-http-client-reqwest},
│                     # reqwest (json+stream+rustls), tokio, serde/serde_json/toml, anyhow, hyprpanel-core
├── build.rs          # slint_build::compile("ui/chat.slint")
├── ui/chat.slint     # fenêtre sombre (palette Catppuccin mocha, assortie aux thèmes hypr)
└── src/
    ├── main.rs       # config, runtime tokio en thread dédié, boucle d'événements Slint
    ├── agent.rs      # boucle LLM + parsing SSE + types de messages conversation
    └── mcp.rs        # wrapper client rmcp (connexion, list_all_tools, call_tool)
```

## Design

**UI (chat.slint)** — fenêtre ~520×680 : liste de messages (bulles `user` / `assistant` / `outil` / `erreur`), streaming du texte assistant (mise à jour de la dernière bulle à chaque delta), champ de saisie multi-ligne + bouton Envoyer, barre d'état (modèle, « MCP : N outils » ou erreur). `MessageRow { kind, text }` via `VecModel`, mises à jour depuis les tâches tokio par `slint::invoke_from_event_loop` + `Weak`.

**Client MCP (mcp.rs)** — squelette validé sur les sources rmcp 3.9 : `StreamableHttpClientTransportConfig::with_uri("http://127.0.0.1:8765/mcp?key=<token>")` (la query passe telle quelle à reqwest → compatible avec notre auth `?key=`), `ClientConfig::default().serve(transport).await` (handshake auto, `peer_info` fournit les instructions serveur), `list_all_tools()`, `call_tool(CallToolRequestParams::new(...).with_arguments(...))`. Token/port lus via **`hyprpanel-core::Settings::load()`** (réutilisation du cœur). Reconnexion au lancement ; erreurs affichées en bulle d'état.

**Boucle d'agent (agent.rs)** — protocole Anthropic Messages :
- `POST {base_url}/v1/messages` (`x-api-key`, `anthropic-version: 2023-06-01`, `stream: true`)
- Outils convertis depuis les `Tool` MCP (`name`, `description`, `input_schema` → format Anthropic)
- System prompt = instructions du serveur (récupérées au handshake) + règles de chat en français
- Parsing SSE maison (lignes `data:` sur `bytes_stream()`) : `content_block_delta` (`text_delta` → delta UI ; `input_json_delta` → accumulation JSON), `tool_use` complet → bulle « outil X… » → `call_tool` MCP → bulle résultat (tronquée ~400 caractères) → message `tool_result` → **boucle** jusqu'à `stop_reason: end_turn` (garde-fou : 12 itérations max)
- Timeouts : 120 s LLM, 30 s outil ; erreurs réseau/API en bulles d'erreur

**Config** — `~/.config/hyprpanel/chat.toml` (chmod 600) : `base_url` (défaut `https://api.z.ai/api/anthropic`), `api_key`, `model` (défaut `GLM-5.3`), `max_tokens` (défaut 8192), `mcp_url` (optionnel — sinon composé depuis `Settings::load()`). Créée par `hyprpanel-chat init` ; **la clé Z.ai est copiée depuis `~/.zcode/cli/config.json`** (jq, jamais affichée en clair dans les sorties).

**Threading** — thread principal : boucle Slint ; thread dédié : runtime tokio multi-thread (session MCP + boucle agent) ; communication UI→agent par `mpsc` dans un `OnceLock`, agent→UI par `invoke_from_event_loop`.

## Non-inclus

Autostart/keybind (ligne `hl.bind` fournie dans le README à coller dans `hyprland.lua`), historique persistant (la conversation vit le temps de la fenêtre), serveurs MCP additionnels, streaming des résultats d'outils.

## Vérification

1. `cargo build && cargo test` (workspace complet — tests : parsing SSE sur fixtures, conversion outils MCP→Anthropic, config chat round-trip)
2. `hyprpanel-chat init` : chat.toml créé en 600 avec la clé, sans fuite dans la sortie
3. Lancer `hyprpanel-chat` (WAYLAND_DISPLAY hérité) → **capture d'écran via l'outil `hypr_screenshot` du daemon** pour confirmer le rendu de la fenêtre + barre d'état « MCP : 20 outils »
4. Conversation réelle : « montre l'état de mes écrans » → vérifier : streaming visible, bulles d'outils (`hypr_status`/`hypr_monitors`), réponse correcte ; puis une commande à effet réversible (gaps 12→5) avec notification
5. Commit du sous-dépôt + mise à jour README/CLAUDE.md/CONTEXT.md

## Références

- [StreamableHttpService (docs.rs rmcp)](https://docs.rs/rmcp/latest/rmcp/transport/streamable_http_server/tower/struct.StreamableHttpService.html) et sources rmcp 3.9 locales (client : `src/transport/streamable_http_client.rs`, tests `test_streamable_http_connection_reuse.rs`)
- [slint 1.18.1 — crates.io](https://crates.io/crates/slint), [slint-build](https://crates.io/crates/slint-build)
- [ESEO Space — MCP server en Rust avec rmcp](https://eseospace.com/blog/build-mcp-server-rust)