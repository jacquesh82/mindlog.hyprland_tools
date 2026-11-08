# LYRA v2.2 — LangChain Rust, onboarding, réglages, à propos, i18n, historique restaurable, statut clients

## Contexte

LYRA (fenêtre Slint `hyprpanel-chat`) est fonctionnelle mais minimale. Cette itération la complète : **agent réimplémenté sur LangChain Rust**, écran d'accueil avec exemples cliquables, panneau d'aide, réglages, à propos, **i18n FR/EN**, **historique de configuration restaurable par prompt**, et **détection de l'enregistrement MCP de Claude Code/Zcode** avec messages et commandes prêtes à coller.

Étude préalable (docs.rs, sources v8.0.0) : le module `claude` de langchain-rust 8 (MIT, publié le 3 nov. 2026) fait tout ce que faisait notre code maison — `with_api_key` / `with_base_url` (endpoint final `{base_url}/v1/messages` → compatible `https://api.z.ai/api/anthropic`) / `with_model` (ID arbitraire → GLM-5.3) ; tool use natif Anthropic (fonctions → `input_schema`, `tool_choice`, historique tool_use/tool_result converti automatiquement) ; streaming `stream()` avec accumulateur `input_json_delta` ; `Message` avec `images: Vec<ImageContent>` (base64 brut, media type détecté par magic bytes) et `thinking` round-trip. Features cargo : aucune requise pour claude.

## A. Agent sur LangChain Rust (`agent.rs` refondu)

- **Boucle LLM** : `Claude::new().with_model(..).with_api_key(..).with_base_url(..)` (reconstruit à chaque `ConfigUpdated`) ; conversation = `Vec<langchain Message>` ; outils MCP convertis en `FunctionDefinition::new(name, description, input_schema)` passés via `CallOptions::with_functions`.
- **Streaming UI** : les événements `LLMEvent::Text` mettent à jour la bulle LYRA token par token ; `Done(GenerateResult)` livre `tool_calls` → bulles outil → `McpSession::call` → `Message::new_ai_message_with_tool_calls` + `Message::new_tool_message` → boucle (≤ 12 tours, timeouts 180 s/LLM, 30 s/outil).
- **Pièces jointes** : `build_user_content` simplifié → `Message::new_human_message(texte+fichiers texte inlinés).with_images(vec![ImageContent::from(base64)])`.
- **Supprimé** : `request_stream`, la machine SSE (`SseState`, `apply_event`, `next_data_line`) et le dep `reqwest` direct (HTTP LLM désormais porté par langchain) ; les tests SSE cèdent la place aux tests pièces jointes (`Message` avec images/base64).
- La boucle d'exécution reste fine et maison (streaming vers l'UI, pont MCP, timeouts, registre de tours) : les `AgentExecutor` standards n'exposent ni streaming d'UI ni pont MCP — c'est bien LangChain qui porte client LLM, messages, outils et SSE.

## B. i18n (`src/i18n.rs` nouveau)

- `Lang { Fr, En }` + dictionnaire `Tr` (~60 chaînes : accueil, aide, exemples, réglages, à propos, historique, clients, boutons) exposé comme struct Slint `TrTexts` (propriété `tr`, swap immédiat).
- `chat.toml` + `language = "fr"` ; le system prompt suit la langue.
- Test : parité des clés FR/EN, aucune chaîne vide.

## C. UI : accueil, aide, à propos, réglages, header

- Header : boutons à glyphes avec `Tooltip` natif — `?` aide, `⚙` réglages, `⟲` historique (+ « ↺ nouvelle »).
- **Accueil** (état vide) : wordmark + 3 lignes + **4 exemples cliquables** qui envoient le prompt.
- **Aide** : `TextEdit` read-only (workflow, 20 outils, pièces jointes, restauration, limites). **À propos** : version, GPL-3.0 (Slint communauté), stack, dépôt.
- **Réglages** : Langue (ComboBox), Modèle, Endpoint, Max tokens, Clé API (`InputType.password`), section clients (voir E), [enregistrer] → `chat.toml` + `UserEvent::ConfigUpdated` + swap `tr`.

## D. Historique de configuration restaurable

- Avant chaque tour : `hypr_backup { label: "lyra-turn-NNN" }` (nom parsé de « backup créé : X ») ; `TurnRecord { turn, timestamp, preview, backup }` persisté dans `~/.local/state/hyprpanel/lyra-history.json` (rechargé au démarrage).
- Panneau historique : entrées antéchronologiques + [restaurer] → `UserEvent::Restore` → `hypr_restore_backup` + `hypr_reload` via MCP + bulles de confirmation. « Nouvelle conversation » vide le chat, conserve le registre.

## E. Statut clients Claude Code / ZCode (`hyprpanel-core::clients`, nouveau)

- `detect_clients()` : Claude Code = `hyprpanel` dans `mcpServers` global ou d'un projet de `~/.claude.json` ; ZCode = `mcp.servers.hyprpanel` dans `~/.zcode/cli/config.json`. Parsing pur sur fixtures (tests).
- `registration_snippets(token, port)` : commandes prêtes à coller (token via `Settings::load()`).
- Dans LYRA : bulle d'information au démarrage si un client manque + section « clients externes » dans les réglages (statut ✔/✗ + commandes, token inclus — usage local). Dans `hyprpanel check` : lignes ✓/✗ par client.

## Fichiers

`crates/hyprpanel-chat/{ui/chat.slint, src/{i18n.rs (nouveau), agent.rs, main.rs, ui.rs, chat_config.rs}}`, `crates/hyprpanel-core/src/clients.rs` (nouveau), `crates/hyprpanel-server/src/main.rs` (check), Cargo.toml (+langchain-rust 8, −reqwest), README/CLAUDE.md/CONTEXT + commit.

## Vérification

1. `cargo test` — parité i18n, round-trip registre/config (language), pièces jointes → `Message` (images+texte), parsing « backup créé : », détection clients (fixtures présent/absent/malformé)
2. **Spike LangChain d'abord** : avant le refact complet, un mini-test (ignoré, réseau) `Claude` + base_url Z.ai + `with_model("GLM-5.3")` + un appel streaming sans outil — valide l'endpoint ; si la clé coding-plan refusait l'endpoint Anthropic natif de langchain (en-têtes/version), fallback documenté : conserver l'implémentation SSE actuelle pour le transport
3. Release + install + relance ; **porte visuelle** `hypr_screenshot` + analyse : accueil FR, boutons header, statuts clients (✔/✔ sur cette machine — pas de bulle attendue)
4. Conversation réelle (état écrans, gaps réversibles) → streaming visibles, bulles outils ; historique → [restaurer] → `hypr_get_option`/`hypr_config_errors` confirment le retour arrière
5. Bascule EN dans les réglages → capture de contrôle ; commit

## Hors périmètre

Drag & drop, PDF, historique de conversation persistant, AgentExecutor standard (voir A), enregistrement automatique des clients (commandes affichées, jamais écrites sans accord).

## Références

- [langchain-rust — crates.io](https://crates.io/crates/langchain-rust) · [module claude (docs.rs)](https://docs.rs/langchain-rust/latest/langchain_rust/llm/claude/index.html) · [dépôt GitHub](https://github.com/Abraxas-365/langchain-rust)
- [Slint 1.18.1](https://crates.io/crates/slint) (Tooltip natif, InputType.password, TextEdit read-only — vérifiés dans les sources)