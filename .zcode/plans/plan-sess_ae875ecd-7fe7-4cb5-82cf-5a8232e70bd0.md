# LYRA — identité visuelle + envoi de fichiers dans hyprpanel-chat

## Contexte

L'utilisateur baptise l'assistante **LYRA** et fournit une référence visuelle : bannière cyberpunk « LYRA — Wayland Configuration Assistant » (fond `#0B0F1A`, surfaces `#141B2D`/`#1A2238`, néons cyan `#22D3EE` / magenta `#D946EF` / rose `#EC4899`, vert succès `#4ADE80`, badge pill, glows). Il veut aussi **pouvoir envoyer des fichiers** à l'interface (images + textes) pour enrichir les demandes au LLM.

Facts vérifiés : `zenity` présent (sélecteur de fichiers), Noto Sans + Noto Color Emoji installés, pas de police display exotique confirmée → wordmark en Noto Sans gras + `letter-spacing` + bicolore cyan/magenta.

## A. Restyle LYRA (`crates/hyprpanel-chat/ui/chat.slint`)

- **Identité** : titre de fenêtre « LYRA — configuration Wayland » ; en-tête = wordmark `LY` (cyan) + `RA` (magenta), gras, `letter-spacing: 4px`, + pill « WAYLAND CONFIGURATION ASSISTANT » (petites capitales espacées, bordure cyan, léger `drop-shadow` néon). Bulles assistant étiquetées « LYRA ».
- **Palette swap** complet : fond `#0B0F1A`, barre `#111827`, bulles assistant `#1A2238` (accent `#22D3EE`), bulles utilisateur en dégradé `@linear-gradient(135°, #8B5CF6 → #EC4899)` texte blanc, bulles outil `#0F1524` mono (accent cyan), erreur `#2D1220`/`#F43F5E`, statut « ● » vert `#4ADE80` quand connecté. Saisie sur `#111827`, bouton envoyer accent cyan.

## B. Envoi de fichiers

- **UI** : bouton « ＋ fichier » à gauche de la saisie ; ligne de **chips** de pièces jointes au-dessus (nom + taille, bouton « × » par chip). Nouvelles struct/callbacks Slint : `AttachmentRow {name, meta}`, `attach-clicked()`, `remove-attachment(int)`.
- **Sélecteur** (main.rs) : thread dédié → `zenity --file-selection --multiple --separator=\n` (env Wayland hérité), retour via `invoke_from_event_loop` ; miroir Rust `Arc<Mutex<Vec<PathBuf>>>` synchronisé avec le modèle (vidé au moment de l'envoi).
- **Canal** : `UserEvent::Text` devient `UserEvent::Input { text, files: Vec<PathBuf> }`.
- **Agent** (agent.rs, fonctions pures testées) : `build_user_content()` construit les blocs Anthropic du message utilisateur :
  - images `png/jpeg/webp/gif` ≤ 5 Mo → bloc `{"type":"image","source":{"type":"base64",…}}` (crate `base64` ajoutée)
  - fichiers texte (md/lua/conf/toml/json/rs/py/sh/yaml/html… ) ≤ 64 Ko → bloc texte délimité `— fichier <nom> —`
  - autres (pdf, binaires) → note dans le texte « pièce non transmise (type non pris en charge) »
  - la bulle utilisateur affiche les pièces (`📎 nom (taille)`)
- Les pièces restent dans l'historique de la conversation (renvoyées aux tours suivants — acceptable pour un usage personnel).

## C. Finitions

- README/CLAUDE.md : section LYRA (branding, pièces jointes, limites : PDF non supporté en v1, rejet propre si l'API refuse les images). CONTEXT.md + commit.

## Fichiers

`crates/hyprpanel-chat/ui/chat.slint` (restyle + chips), `src/main.rs` (zenity, miroir pièces), `src/agent.rs` (UserEvent, build_user_content + classification pure), `Cargo.toml` (+base64), docs. Réutilisation : `ui.rs` (append/set_row/invoke_from_event_loop), `truncate_str` d'agent.rs.

## Vérification

1. `cargo test` — nouveaux tests : classification des pièces, `build_user_content` avec fichiers temporaires (image factice encodée base64, fichier texte, type refusé), cas texte vide + pièce seule
2. Rebuild release + `install.sh` + relance de la fenêtre (arrêt de l'unit transitoire précédente)
3. **Porte visuelle** : capture via `hypr_screenshot` du daemon → analyse d'image pour confirmer wordmark LYRA, palette, chips ; corriger si écart
4. Test clic réel laissé à l'utilisateur (zenity + envoi d'une capture d'écran au LLM) — instructions fournies ; le pipeline est couvert par les tests unitaires

## Hors périmètre

PDF/documents binaires, glisser-déposer (DropArea non standard en Slint 1.18), historique persistant, streaming des résultats d'outils.