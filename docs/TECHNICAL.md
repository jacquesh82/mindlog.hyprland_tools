# Guide technique

[← Présentation et installation](../README.md)

## Composants du projet

LYRA s’appuie sur un service local, nommé `hyprpanel`, pour communiquer avec
Hyprland. Ce service expose 20 outils via MCP. Le chat y ajoute les propositions
visuelles et le catalogue de palettes.

| Dossier | Rôle | Licence |
| --- | --- | --- |
| `crates/hyprpanel-chat` | Application LYRA, interface Slint et conversation avec les modèles | GPL-3.0-only |
| `crates/hyprpanel-core` | Communication avec Hyprland, inventaire de configuration et sauvegardes | MIT |
| `crates/hyprpanel-server` | Service MCP local, binaire `hyprpanel` | MIT |

Le serveur écoute par défaut sur `127.0.0.1:8765/mcp`. Il authentifie les requêtes
avec un jeton transmis par l’en-tête `Authorization: Bearer <JETON>` ou par
le paramètre `?key=<JETON>`. LYRA configure automatiquement sa connexion au
service à partir de la configuration locale.

## Configuration et données

Chemins par défaut, adaptés si les variables XDG correspondantes sont définies :

| Emplacement | Contenu |
| --- | --- |
| `~/.config/hyprpanel/chat.toml` | Fournisseur, modèles, langue, animation et clé Z.ai |
| `~/.config/hyprpanel/config.toml` | Port, jeton et chemins utilisés par le service |
| `~/.local/state/hyprpanel/backups/` | Sauvegardes de configuration |
| `~/.local/state/hyprpanel/lyra-history.json` | Registre des sauvegardes associées aux échanges |
| `~/.cache/hyprpanel/screenshots/` | Captures d’écran |

Les fichiers de configuration sont créés avec les permissions `600`.
Pour les initialiser sans passer par le script d’installation :

```bash
hyprpanel init
hyprpanel-chat init
```

### Fournisseurs et modèles

Le champ `provider` accepte `zcode`, `claude_code` ou `codex`.

- **Zcode** utilise LangChain Rust et le protocole Anthropic compatible Z.ai,
  avec l’endpoint par défaut `https://api.z.ai/api/anthropic`. Les champs
  `model`, `base_url`, `api_key` et `max_tokens` règlent ce mode.
- **Claude Code** utilise le client officiel connecté à votre abonnement.
  Le champ `claude_model` est facultatif.
- **Codex** utilise le client officiel connecté avec ChatGPT.
  Le champ `codex_model` est facultatif.

Pour les deux clients officiels, un modèle vide conserve le choix par défaut
du client. LYRA exclut les clés API et passerelles héritées de l’environnement
pour ces modes, sans copier les jetons de connexion. Elle valide leurs réponses
structurées avant de les afficher ou d’exécuter les outils demandés.

Les outils, sauvegardes et choix cliquables sont exécutés par LYRA pour les trois
fournisseurs. Leur utilisation dans le chat ne nécessite pas d’enregistrer
manuellement le serveur MCP dans chaque client.

Références : [Claude Code programmatique](https://code.claude.com/docs/en/headless),
[Codex non interactif](https://learn.chatgpt.com/docs/non-interactive-mode).

## Fichiers joints et choix visuels

Le sélecteur de fichiers utilise `zenity` et accepte plusieurs pièces jointes :

| Type | Limite par fichier |
| --- | --- |
| Images PNG, JPEG, WebP et GIF | 5 Mio |
| Texte, configuration et code source | 64 Kio |

Les images sont transmises au modèle comme des images ; le contenu des fichiers
texte est inclus dans le message. Les PDF et le glisser-déposer ne sont pas pris
en charge.

`lyra_offer_choices` affiche de 2 à 8 options cliquables : texte, couleurs ou
palettes, images locales ou schémas SVG autonomes. Le clic transmet le choix
exact à l’agent et reprend la conversation. Une préférence ne vaut pas
confirmation générale pour modifier le bureau.

`lyra_palette_catalog` propose une sélection locale de palettes issues de
[Color Hunt](https://colorhunt.co/), avec leurs codes et liens sources. Ce
catalogue constitue un point de départ et n’est pas actualisé en temps réel.

## Dépannage

### Le service local ne répond pas

```bash
systemctl --user status hyprpanel
journalctl --user -u hyprpanel -n 50 --no-pager
systemctl --user restart hyprpanel
```

Exécutez ces commandes depuis votre session Hyprland. Le script d’installation
configure le service avec l’affichage Wayland détecté au moment de l’installation.

`hyprpanel check` fournit aussi un diagnostic IPC, configuration, sauvegardes,
port et présence de `grim`. Son test de disponibilité du port peut signaler
« occupé » lorsque le service est déjà démarré.

### Vérifier les fournisseurs

```bash
hyprpanel-chat check-providers
hyprpanel-chat check-providers --live
```

La première commande vérifie la présence de la clé Zcode et la connexion locale
des clients Claude Code et Codex. La seconde fait de vrais appels via les trois
fournisseurs et vérifie les propositions de choix, sans modifier Hyprland.

## Utiliser MCP depuis un autre client

Cette intégration est facultative et permet d’accéder aux outils du service
directement depuis une autre application.

```bash
hyprpanel token
```

Utilisez le jeton affiché à la place de `<JETON>` dans les exemples ci-dessous.
Adaptez le port si vous l’avez changé.

### Claude Code

```bash
claude mcp add --scope user --transport http hyprpanel http://127.0.0.1:8765/mcp \
  --header "Authorization: Bearer <JETON>"
```

### Zcode

Dans `~/.zcode/cli/config.json`, ajoutez le serveur à votre configuration existante :

```json
{
  "mcp": {
    "servers": {
      "hyprpanel": {
        "url": "http://127.0.0.1:8765/mcp?key=<JETON>"
      }
    }
  }
}
```

### Outils exposés

| Domaine | Outils |
| --- | --- |
| État | `hypr_status`, `hypr_monitors`, `hypr_workspaces`, `hypr_clients`, `hypr_devices`, `hypr_binds` |
| Réglages à chaud | `hypr_set_option_live`, `hypr_dispatch`, `hypr_notify` |
| Configuration et sauvegardes | `hypr_config_files`, `hypr_find_setting`, `hypr_backup`, `hypr_list_backups`, `hypr_restore_backup` |
| Validation | `hypr_reload`, `hypr_config_errors` |
| Observation | `hypr_get_option`, `hypr_eval`, `hypr_recent_events`, `hypr_screenshot` |

Le serveur fournit l’état, les réglages à chaud, les sauvegardes et la validation.
La persistance dans `hyprland.lua` se fait avec les outils d’édition de fichiers
du client externe. Les fichiers `monitors.lua` et `inputs.lua`, générés par un
outil externe, sont signalés en lecture seule.

## Compatibilité Hyprland

Le projet a été testé sur Hyprland 0.57.1 avec la configuration Lua (`hl.*`).

- Les réglages à chaud passent par `/dispatch hl.dsp.global("conf …")`.
  Une option inconnue peut être ignorée malgré une réponse « ok » : son effet
  doit être vérifié.
- `getoption` et `hl.get_config` renvoient l’état des fichiers, qui peut différer
  d’un réglage appliqué à chaud.
- `/repl` permet de lire les expressions Lua du compositeur.

## Développer et vérifier l’interface

```bash
cargo build --workspace --locked
cargo test --workspace --locked
cargo run -p hyprpanel-chat --example preview --locked
cargo run -p hyprpanel-chat --example typing_perf --locked
```

Les deux exemples fonctionnent sans session graphique, serveur MCP ou appel de
modèle. `preview` rend les composants Slint réels, vérifie les interactions et
écrit les captures dans `target/lyra-previews/`. `typing_perf` mesure le traitement
des frappes et le rendu, et vérifie la pause de l’animation pendant la saisie.
Ajoutez `--offline` après téléchargement des dépendances pour travailler sans
réseau.

Les réponses Markdown sont converties en HTML nettoyé puis affichées par les
composants natifs. Les détails d’outils sont repliés et le défilement suit la
réponse tant que l’utilisateur reste en bas du chat.

L’avatar utilise des textures préparées en 256 × 256 au build et une horloge
partagée limitée à 20 images par seconde. Les animations s’arrêtent pendant la
saisie, les panneaux modaux et la minimisation. Elles sont désactivables dans
les réglages (`animate_avatar`). Les prompts de création des images figurent
dans [la documentation des assets](../crates/hyprpanel-chat/assets/README.md).
