//! Internationalisation FR/EN de LYRA.
//!
//! Le dictionnaire interne `Tr` est sérialisable (test de parité des clés) ;
//! `Tr::to_slint()` produit la struct `TrTexts` consommée par l'UI.

use serde::Serialize;

use crate::TrTexts;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Fr,
    En,
}

impl Lang {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "en" | "us" => Lang::En,
            _ => Lang::Fr,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
        }
    }
}

/// Dictionnaire complet. Toute nouvelle chaîne DOIT être ajoutée aux deux
/// langues (le test de parité l'exige).
#[derive(Debug, Clone, Serialize)]
pub struct Tr {
    // générique
    pub send: String,
    pub cancel: String,
    pub save: String,
    pub close: String,
    pub new_chat: String,
    pub status_starting: String,
    pub status_thinking: String,
    pub status_writing: String,
    pub placeholder: String,
    pub attach: String,
    pub paste_image: String,
    pub paste_label: String,
    pub paste_reading: String,
    pub paste_added: String,
    pub paste_empty: String,
    pub paste_error: String,
    // header
    pub tip_help: String,
    pub tip_settings: String,
    pub tip_history: String,
    pub header_subtitle: String,
    pub composer_hint: String,
    pub card_displays: String,
    pub card_appearance: String,
    pub card_shortcuts: String,
    pub card_capture: String,
    // accueil
    pub welcome_title: String,
    pub welcome_l1: String,
    pub welcome_l2: String,
    pub welcome_l3: String,
    pub welcome_examples: String,
    pub ex1: String,
    pub ex2: String,
    pub ex3: String,
    pub ex4: String,
    // aide
    pub help_title: String,
    pub help_body: String,
    // à propos
    pub about_title: String,
    pub about_body: String,
    // réglages
    pub settings_title: String,
    pub settings_language: String,
    pub settings_model: String,
    pub settings_endpoint: String,
    pub settings_max_tokens: String,
    pub settings_api_key: String,
    pub settings_clients: String,
    pub settings_clients_ok: String,
    pub settings_clients_missing: String,
    pub settings_saved: String,
    pub settings_invalid: String,
    pub settings_motion: String,
    pub settings_scope: String,
    pub settings_scope_hint: String,
    pub memory_title: String,
    pub memory_hint: String,
    pub memory_clear: String,
    pub memory_confirm: String,
    pub memory_cleared: String,
    pub settings_context: String,
    pub settings_context_hint: String,
    pub settings_context_invalid: String,
    pub fs_title: String,
    pub fs_body: String,
    pub fs_note: String,
    pub fs_allow: String,
    pub fs_refuse: String,
    pub fs_granted: String,
    pub fs_denied: String,
    pub fs_request: String,
    pub fs_revoke: String,
    pub command_title: String,
    pub command_note: String,
    pub command_run: String,
    pub command_decline: String,
    pub command_waiting: String,
    pub command_declined: String,
    pub command_restore: String,
    pub command_desktop: String,
    pub environment_title: String,
    pub environment_note: String,
    pub environment_refresh: String,
    pub environment_scanning: String,
    pub environment_query: String,
    pub environment_refreshed: String,
    pub inventory_packages: String,
    pub inventory_configurations: String,
    pub inventory_launchers: String,
    pub inventory_queries: String,
    pub inventory_saving: String,

    pub provider: String,
    pub subscription_hint: String,
    // historique
    pub history_title: String,
    pub history_hint: String,
    pub history_empty: String,
    pub restore: String,
    pub restore_started: String,
    pub restore_done: String,
    pub restore_failed: String,
    pub no_backup: String,
    // clients externes
    pub clients_notice: String,
    // bulles
    pub label_you: String,
    pub label_tool: String,
    pub show_tool_details: String,
    pub hide_tool_details: String,
    pub status_tools: String,
    pub choice_done: String,
    pub choice_skipped: String,
    pub choice_selected: String,
    pub preview_missing: String,
    pub label_error: String,
    pub new_conversation: String,
    pub mcp_unavailable: String,
    pub mcp_tools: String,
}

pub fn tr(lang: Lang) -> Tr {
    match lang {
        Lang::Fr => french(),
        Lang::En => english(),
    }
}

const HELP_FR: &str = "\
LYRA configure votre interface Hyprland en discutant.

WORKFLOW — elle procède toujours ainsi :
1. Test à chaud (hypr_set_option_live) : le réglage s'applique immédiatement, sans toucher aux fichiers — parfait pour essayer.
2. Si vous validez : elle édite ~/.config/hypr/hyprland.lua et recharge (hypr_reload), puis vérifie les erreurs (hypr_config_errors).
3. Après autorisation d’accès aux fichiers, une sauvegarde de la config est demandée avant chaque message : le panneau ⟲ permet de restaurer les sauvegardes disponibles.

ACCÈS AUX FICHIERS — LYRA demande votre accord pour consulter et modifier ~/.config. Refuser ou fermer la demande laisse l’accès désactivé. L’accord vaut pour la session et peut être retiré dans les réglages.

CE QU'ELLE SAIT FAIRE
• Lire l'état : écrans, espaces de travail, fenêtres, périphériques, raccourcis, options.
• Régler : gaps, bordures, animations, opacité, règles de fenêtres, raccourcis clavier…
• Pièces jointes : bouton « Joindre » — images (elle les voit) et fichiers texte (elle les lit). Ctrl+V ou « Coller une image » ajoute une image du presse-papiers ; elle est transmise uniquement à l’envoi du message.
• Captures d'écran (grim) pour constater visuellement un changement, notifications à l'écran.

GARDE-FOUS
• En cas d’incertitude, elle vérifie les sources officielles sur Internet et cite les pages consultées. Un échec de vérification est signalé.
• Chaque commande, même de lecture, demande votre confirmation dans une boîte affichant son contenu exact. Refuser, fermer ou Échap n’exécute rien.
• Après votre accord d’accès, un profil local de l’environnement est mémorisé sans lancer de commande ; il est consultable et actualisable dans les réglages.
• monitors.lua et inputs.lua sont générés par un outil externe : elle ne les édite jamais.
• Un réglage à chaud répond « ok » même si l'option n'existe pas : elle vérifie l'effet réel.
• Demandez-lui d'annoncer les changements visibles importants (hypr_notify).

LIMITES — pas de drag & drop, pas de PDF ; la conversation vit le temps de la fenêtre (l'historique de config, lui, persiste).";

const HELP_EN: &str = "\
LYRA configures your Hyprland desktop through conversation.

WORKFLOW — she always proceeds like this:
1. Hot test (hypr_set_option_live): the setting applies immediately, no files touched — perfect for trying things out.
2. Once you approve: she edits ~/.config/hypr/hyprland.lua and reloads (hypr_reload), then checks for errors (hypr_config_errors).
3. After filesystem approval, a config snapshot is requested before each message: the ⟲ panel restores available snapshots.

FILESYSTEM ACCESS — LYRA asks permission to read and edit ~/.config. Declining or closing the request leaves access disabled. Approval lasts for the session and can be revoked in Settings.

WHAT SHE CAN DO
• Read state: monitors, workspaces, windows, devices, keybindings, options.
• Tune: gaps, borders, animations, opacity, window rules, keybindings…
• Attachments: the “Attach” button — images (she sees them) and text files (she reads them). Ctrl+V or “Paste image” adds an image from the clipboard; it is shared only when you send the message.
• Screenshots (grim) to visually confirm a change, on-screen notifications.

SAFETY
• When uncertain, she checks official online sources and cites the pages read. Failed verification is reported.
• Every command, including read-only commands, requires confirmation in a dialog showing its exact content. Declining, closing or Escape runs nothing.
• After filesystem approval, a local environment profile is remembered without running commands; view and refresh it in Settings.
• monitors.lua and inputs.lua are generated by an external tool: she never edits them.
• A hot setting answers “ok” even for unknown options: she verifies the actual effect.
• Ask her to announce important visible changes (hypr_notify).

LIMITS — no drag & drop, no PDF; the conversation lives as long as the window (the config history, however, persists).";

fn about_body(version: &str) -> (String, String) {
    let fr = format!(
        "LYRA — Wayland Configuration Assistant\n\
         Version {version} · Licence GPL-3.0 (Slint sous licence communauté)\n\
         Fenêtre native Slint 1.18 · GLM Coding Plan, Claude Code et Codex\n\
         Outils du panneau hyprpanel via MCP (rmcp 3.9)\n\
         Dépôt : github.com/jacquesh82/mindlog.hyprland_tools"
    );
    let en = format!(
        "LYRA — Wayland Configuration Assistant\n\
         Version {version} · GPL-3.0 license (Slint community license)\n\
         Native Slint 1.18 window · GLM Coding Plan, Claude Code and Codex\n\
         hyprpanel panel tools over MCP (rmcp 3.9)\n\
         Repository: github.com/jacquesh82/mindlog.hyprland_tools"
    );
    (fr, en)
}

fn french() -> Tr {
    let (about_fr, _) = about_body(env!("CARGO_PKG_VERSION"));
    Tr {
        send: "Envoyer".into(),
        cancel: "Annuler".into(),
        save: "Enregistrer".into(),
        close: "Fermer".into(),
        new_chat: "Nouvelle conversation".into(),
        status_starting: "démarrage…".into(),
        status_thinking: "LYRA réfléchit…".into(),
        status_writing: "LYRA écrit…".into(),
        placeholder: "Que souhaitez-vous changer sur votre bureau ?".into(),
        attach: "Joindre".into(),
        paste_image: "Coller une image".into(),
        paste_label: "Image collée".into(),
        paste_reading: "Lecture du presse-papiers…".into(),
        paste_added: "Image ajoutée. Envoyez le message pour la partager avec LYRA.".into(),
        paste_empty: "Aucune image compatible dans le presse-papiers.".into(),
        paste_error: "Impossible de coller l’image".into(),
        tip_help: "Aide".into(),
        tip_settings: "Réglages".into(),
        tip_history: "Historique de configuration".into(),
        header_subtitle: "Votre assistante Hyprland".into(),
        composer_hint: "Entrée pour envoyer · Images et fichiers texte".into(),
        card_displays: "Vos écrans".into(),
        card_appearance: "L’apparence".into(),
        card_shortcuts: "Vos raccourcis".into(),
        card_capture: "Votre bureau".into(),
        welcome_title: "Bonjour, je suis LYRA.".into(),
        welcome_l1: "Un bureau qui vous ressemble. Décrivez vos envies, je m’occupe des réglages.".into(),
        welcome_l2: "Des essais réversibles. L’accès à vos fichiers reste votre choix.".into(),
        welcome_l3: "Joignez des fichiers avec « Joindre » ou collez une image avec Ctrl+V.".into(),
        welcome_examples: "Un point de départ pour votre prochaine idée".into(),
        ex1: "Montre l'état de mes écrans".into(),
        ex2: "Essaie des gaps de 10, puis remets comme avant".into(),
        ex3: "Liste mes raccourcis contenant SUPER".into(),
        ex4: "Prends une capture et analyse mon bureau".into(),
        help_title: "Aide — comment ça marche".into(),
        help_body: HELP_FR.into(),
        about_title: "À propos".into(),
        about_body: about_fr,
        settings_title: "Réglages".into(),
        settings_language: "Langue / Language".into(),
        settings_model: "Modèle".into(),
        settings_endpoint: "Service du GLM Coding Plan".into(),
        settings_max_tokens: "Tokens max".into(),
        settings_api_key: "Clé du GLM Coding Plan".into(),
        settings_clients: "Clients externes (Claude Code, ZCode)".into(),
        settings_clients_ok: "✔ enregistré".into(),
        settings_clients_missing: "✗ non enregistré — commande :".into(),
        settings_saved: "réglages enregistrés".into(),
        settings_invalid: "Vérifiez le modèle et la limite de tokens (256 à 200 000).".into(),
        settings_motion: "Animer l’avatar de LYRA".into(),
        settings_scope: "Limiter les actions au périmètre Wayland".into(),
        settings_scope_hint: "Activé : bureau, applications, navigateurs et thèmes compris. Désactivé : demandes générales autorisées. Les permissions des fichiers et confirmations de commandes restent obligatoires.".into(),
        memory_title: "Utiliser la mémoire locale de conversation".into(),
        memory_hint: "Après autorisation FS, conserve jusqu’à 192 extraits de vos demandes et choix. Recherche locale à chaque envoi, sans service d’embeddings. Désactiver arrête la lecture et l’enregistrement ; effacer supprime ces extraits. Les rappels pertinents sont envoyés au fournisseur choisi avec votre message.".into(),
        memory_clear: "Effacer la mémoire de conversation".into(),
        memory_confirm: "Supprimer les extraits conservés et démarrer une nouvelle conversation ? Le profil système et l’historique des sauvegardes seront conservés.".into(),
        memory_cleared: "Mémoire effacée — nouvelle conversation.".into(),
        settings_context: "Contexte SYSTEM".into(),
        settings_context_hint: "Instructions communes aux trois fournisseurs. Laissez vide pour le rôle LYRA par défaut ; votre texte le remplace. Le périmètre choisi, les vérifications et confirmations restent appliqués. 16 000 caractères maximum.".into(),
        settings_context_invalid: "Le contexte SYSTEM doit contenir au maximum 16 000 caractères.".into(),
        fs_title: "Autoriser l’accès aux fichiers ?".into(),
        fs_body: "Autorisez-vous LYRA à accéder à vos configurations et à mémoriser l’environnement (OS, version, utilisateur, outils) ? Si la mémoire de conversation est activée, elle conserve aussi des extraits de vos demandes et choix. Les informations pertinentes sont transmises au fournisseur sélectionné.".into(),
        fs_note: "Profil local : OS, paquets installés, lanceurs et configurations autorisées. Chaque commande de métadonnées demande votre confirmation. Fichiers privés et générés protégés. Accord valable pour la session, révocable dans les réglages (contexte réinitialisé). Les pièces jointes restent utilisables en cas de refus.".into(),
        fs_allow: "Autoriser l’accès".into(),
        fs_refuse: "Refuser".into(),
        fs_granted: "FS : accès autorisé".into(),
        fs_denied: "FS : accès non autorisé".into(),
        fs_request: "Autoriser l’accès aux fichiers…".into(),
        fs_revoke: "Retirer l’accès aux fichiers".into(),
        command_title: "Confirmer l’exécution".into(),
        command_note: "Cette action s’exécute sur votre machine. Une commande shell dispose des droits de votre compte, y compris hors de ~/.config. L’accord porte uniquement sur le contenu ci-dessus ; Refuser ou fermer n’exécute rien.".into(),
        command_run: "Exécuter".into(),
        command_decline: "Refuser".into(),
        command_waiting: "LYRA attend votre confirmation…".into(),
        command_declined: "Action refusée : rien n’a été exécuté.".into(),
        command_restore: "Restaurer la sauvegarde sélectionnée, puis recharger Hyprland.".into(),
        command_desktop: "Interroger ou agir sur Hyprland avec l’outil et les paramètres affichés.".into(),
        environment_title: "Environnement mémorisé localement".into(),
        environment_note: "Inventaire des paquets installés, lanceurs et configurations autorisées. Versions issues des paquets ; références sans paquet et configurations actives non confirmées restent signalées. Chaque commande de métadonnées demande votre accord. Profil local partagé avec le fournisseur pour contextualiser vos demandes.".into(),
        environment_refresh: "Actualiser l’environnement".into(),
        environment_scanning: "Inventaire des paquets et configurations…".into(),
        environment_query: "Lire l’inventaire installé de {} : noms, versions et chemins déclarés. Aucune installation, mise à jour ni exécution des commandes trouvées dans vos configurations.".into(),
        environment_refreshed: "Profil actualisé — consultez les sources et limites de l’inventaire.".into(),
        inventory_packages: "Paquets installés".into(),
        inventory_configurations: "Configurations parcourues".into(),
        inventory_launchers: "Lanceurs parcourus".into(),
        inventory_queries: "Sources de métadonnées".into(),
        inventory_saving: "Recoupement et enregistrement du profil".into(),

        provider: "Abonnement utilisé".into(),
        subscription_hint: "Utilise la session connectée du client officiel. Modèle vide = modèle par défaut de l’abonnement. Connexion : claude auth login / codex login.".into(),
        history_title: "Historique de configuration".into(),
        history_hint: "« Restaurer » remet la configuration Hyprland à l'état d'AVANT ce message. La conversation est conservée.".into(),
        history_empty: "aucun tour enregistré pour l'instant".into(),
        restore: "Restaurer".into(),
        restore_started: "restauration de la configuration…".into(),
        restore_done: "configuration restaurée et rechargée".into(),
        restore_failed: "restauration échouée".into(),
        no_backup: "sans backup".into(),
        clients_notice: "Client externe non connecté au panneau".into(),
        label_you: "vous".into(),
        label_tool: "outil".into(),
        show_tool_details: "Détails des outils".into(),
        hide_tool_details: "Masquer les détails".into(),
        status_tools: "LYRA utilise ses outils…".into(),
        choice_done: "Choix envoyé à LYRA".into(),
        choice_skipped: "Propositions précédentes".into(),
        choice_selected: "Je choisis".into(),
        preview_missing: "Aperçu indisponible".into(),
        label_error: "erreur".into(),
        new_conversation: "nouvelle conversation".into(),
        mcp_unavailable: "Serveur MCP hyprpanel injoignable".into(),
        mcp_tools: "MCP : {} outils".into(),
    }
}

fn english() -> Tr {
    let (_, about_en) = about_body(env!("CARGO_PKG_VERSION"));
    Tr {
        send: "Send".into(),
        cancel: "Cancel".into(),
        save: "Save".into(),
        close: "Close".into(),
        new_chat: "New conversation".into(),
        status_starting: "starting…".into(),
        status_thinking: "LYRA is thinking…".into(),
        status_writing: "LYRA is writing…".into(),
        placeholder: "What would you like to change on your desktop?".into(),
        attach: "Attach".into(),
        paste_image: "Paste image".into(),
        paste_label: "Pasted image".into(),
        paste_reading: "Reading clipboard…".into(),
        paste_added: "Image attached. Send the message to share it with LYRA.".into(),
        paste_empty: "No supported image in the clipboard.".into(),
        paste_error: "Could not paste the image".into(),
        tip_help: "Help".into(),
        tip_settings: "Settings".into(),
        tip_history: "Configuration history".into(),
        header_subtitle: "Your Hyprland assistant".into(),
        composer_hint: "Enter to send · Images and text files".into(),
        card_displays: "Your displays".into(),
        card_appearance: "Make it yours".into(),
        card_shortcuts: "Your shortcuts".into(),
        card_capture: "Your desktop".into(),
        welcome_title: "Hi, I'm LYRA.".into(),
        welcome_l1: "A desktop that feels like you. Tell me what you have in mind, I’ll handle the settings.".into(),
        welcome_l2: "Reversible changes. File access stays your choice.".into(),
        welcome_l3: "Attach files with “Attach”, or paste an image with Ctrl+V.".into(),
        welcome_examples: "A starting point for your next idea".into(),
        ex1: "Show my monitors".into(),
        ex2: "Set gaps to 10 just to try, then revert".into(),
        ex3: "List my keybindings containing SUPER".into(),
        ex4: "Take a screenshot and comment the layout".into(),
        help_title: "Help — how it works".into(),
        help_body: HELP_EN.into(),
        about_title: "About".into(),
        about_body: about_en,
        settings_title: "Settings".into(),
        settings_language: "Langue / Language".into(),
        settings_model: "Model".into(),
        settings_endpoint: "GLM Coding Plan service".into(),
        settings_max_tokens: "Max tokens".into(),
        settings_api_key: "GLM Coding Plan key".into(),
        settings_clients: "External clients (Claude Code, ZCode)".into(),
        settings_clients_ok: "✔ registered".into(),
        settings_clients_missing: "✗ not registered — command:".into(),
        settings_saved: "settings saved".into(),
        settings_invalid: "Check the model and token limit (256 to 200,000).".into(),
        settings_motion: "Animate LYRA’s avatar".into(),
        settings_scope: "Limit actions to the Wayland desktop".into(),
        settings_scope_hint: "On: desktop, applications, browsers and themes are included. Off: general requests are allowed. Filesystem permissions and command confirmations still apply.".into(),
        memory_title: "Use local conversation memory".into(),
        memory_hint: "After filesystem consent, keeps up to 192 excerpts from your requests and choices. Searches locally on send, with no embedding service. Disabling stops reading and recording; clearing deletes the excerpts. Relevant recalls are sent to your selected provider with your message.".into(),
        memory_clear: "Clear conversation memory".into(),
        memory_confirm: "Delete saved excerpts and start a new conversation? The system profile and backup history will be kept.".into(),
        memory_cleared: "Memory cleared — new conversation.".into(),
        settings_context: "SYSTEM context".into(),
        settings_context_hint: "Instructions shared by all three providers. Leave empty for LYRA’s default role; your text replaces it. The selected scope, verification and confirmation rules still apply. Maximum 16,000 characters.".into(),
        settings_context_invalid: "The SYSTEM context must not exceed 16,000 characters.".into(),
        fs_title: "Allow filesystem access?".into(),
        fs_body: "Allow LYRA to access your configuration files and remember the environment (OS, version, username, tools)? If conversation memory is enabled, it also keeps excerpts from your requests and choices. Relevant information is sent to the selected provider.".into(),
        fs_note: "Local profile: OS, installed packages, launchers and approved configs. Every metadata command requires confirmation. Private and generated files stay protected. Session access can be revoked in Settings (model context resets). Attachments remain available if you decline.".into(),
        fs_allow: "Allow access".into(),
        fs_refuse: "Decline".into(),
        fs_granted: "FS: access allowed".into(),
        fs_denied: "FS: access not allowed".into(),
        fs_request: "Allow filesystem access…".into(),
        fs_revoke: "Revoke filesystem access".into(),
        command_title: "Confirm execution".into(),
        command_note: "This action runs on your machine. A shell command has your account’s permissions, including outside ~/.config. Approval covers only the content above; declining or closing runs nothing.".into(),
        command_run: "Run".into(),
        command_decline: "Decline".into(),
        command_waiting: "LYRA is waiting for your confirmation…".into(),
        command_declined: "Action declined: nothing was executed.".into(),
        command_restore: "Restore the selected backup, then reload Hyprland.".into(),
        command_desktop: "Query or act on Hyprland using the displayed tool and parameters.".into(),
        environment_title: "Locally remembered environment".into(),
        environment_note: "Installed packages, launchers and approved configurations. Versions come from packages; unpackaged references and unconfirmed active configs remain identified. Every metadata command needs confirmation. The local profile is shared with the provider to contextualize requests.".into(),
        environment_refresh: "Refresh environment".into(),
        environment_scanning: "Discovering packages and configurations…".into(),
        environment_query: "Read the installed {} inventory: names, versions and declared paths. No installation, upgrade or execution of commands found in your configurations.".into(),
        environment_refreshed: "Profile refreshed — see inventory sources and limits.".into(),
        inventory_packages: "Installed packages".into(),
        inventory_configurations: "Configuration entries scanned".into(),
        inventory_launchers: "Launchers scanned".into(),
        inventory_queries: "Metadata sources".into(),
        inventory_saving: "Matching and saving the profile".into(),

        provider: "Active subscription".into(),
        subscription_hint: "Uses the signed-in session of the official client. Empty model = subscription default. Sign in: claude auth login / codex login.".into(),
        history_title: "Configuration history".into(),
        history_hint: "“Restore” puts the Hyprland configuration back to the state BEFORE that message. The conversation is kept.".into(),
        history_empty: "no turn recorded yet".into(),
        restore: "Restore".into(),
        restore_started: "restoring configuration…".into(),
        restore_done: "configuration restored and reloaded".into(),
        restore_failed: "restore failed".into(),
        no_backup: "no backup".into(),
        clients_notice: "External client not connected to the panel".into(),
        label_you: "you".into(),
        label_tool: "tool".into(),
        show_tool_details: "Tool details".into(),
        hide_tool_details: "Hide details".into(),
        status_tools: "LYRA is using tools…".into(),
        choice_done: "Choice sent to LYRA".into(),
        choice_skipped: "Previous suggestions".into(),
        choice_selected: "I choose".into(),
        preview_missing: "Preview unavailable".into(),
        label_error: "error".into(),
        new_conversation: "new conversation".into(),
        mcp_unavailable: "hyprpanel MCP server unreachable".into(),
        mcp_tools: "MCP: {} tools".into(),
    }
}

impl Tr {
    /// Convertit vers la struct Slint (générée par include_modules!).
    pub fn to_slint(&self) -> TrTexts {
        TrTexts {
            send: self.send.clone().into(),
            cancel: self.cancel.clone().into(),
            save: self.save.clone().into(),
            close: self.close.clone().into(),
            new_chat: self.new_chat.clone().into(),
            status_starting: self.status_starting.clone().into(),
            status_thinking: self.status_thinking.clone().into(),
            status_writing: self.status_writing.clone().into(),
            placeholder: self.placeholder.clone().into(),
            attach: self.attach.clone().into(),
            paste_image: self.paste_image.clone().into(),
            paste_label: self.paste_label.clone().into(),
            paste_reading: self.paste_reading.clone().into(),
            paste_added: self.paste_added.clone().into(),
            paste_empty: self.paste_empty.clone().into(),
            paste_error: self.paste_error.clone().into(),
            tip_help: self.tip_help.clone().into(),
            tip_settings: self.tip_settings.clone().into(),
            tip_history: self.tip_history.clone().into(),
            header_subtitle: self.header_subtitle.clone().into(),
            composer_hint: self.composer_hint.clone().into(),
            card_displays: self.card_displays.clone().into(),
            card_appearance: self.card_appearance.clone().into(),
            card_shortcuts: self.card_shortcuts.clone().into(),
            card_capture: self.card_capture.clone().into(),
            welcome_title: self.welcome_title.clone().into(),
            welcome_l1: self.welcome_l1.clone().into(),
            welcome_l2: self.welcome_l2.clone().into(),
            welcome_l3: self.welcome_l3.clone().into(),
            welcome_examples: self.welcome_examples.clone().into(),
            ex1: self.ex1.clone().into(),
            ex2: self.ex2.clone().into(),
            ex3: self.ex3.clone().into(),
            ex4: self.ex4.clone().into(),
            help_title: self.help_title.clone().into(),
            help_body: self.help_body.clone().into(),
            about_title: self.about_title.clone().into(),
            about_body: self.about_body.clone().into(),
            settings_title: self.settings_title.clone().into(),
            settings_language: self.settings_language.clone().into(),
            settings_model: self.settings_model.clone().into(),
            settings_endpoint: self.settings_endpoint.clone().into(),
            settings_max_tokens: self.settings_max_tokens.clone().into(),
            settings_api_key: self.settings_api_key.clone().into(),
            settings_clients: self.settings_clients.clone().into(),
            settings_clients_ok: self.settings_clients_ok.clone().into(),
            settings_clients_missing: self.settings_clients_missing.clone().into(),
            settings_saved: self.settings_saved.clone().into(),
            settings_invalid: self.settings_invalid.clone().into(),
            settings_motion: self.settings_motion.clone().into(),
            settings_scope: self.settings_scope.clone().into(),
            settings_scope_hint: self.settings_scope_hint.clone().into(),
            memory_title: self.memory_title.clone().into(),
            memory_hint: self.memory_hint.clone().into(),
            memory_clear: self.memory_clear.clone().into(),
            memory_confirm: self.memory_confirm.clone().into(),
            memory_cleared: self.memory_cleared.clone().into(),
            settings_context: self.settings_context.clone().into(),
            settings_context_hint: self.settings_context_hint.clone().into(),
            settings_context_invalid: self.settings_context_invalid.clone().into(),
            fs_title: self.fs_title.clone().into(),
            fs_body: self.fs_body.clone().into(),
            fs_note: self.fs_note.clone().into(),
            fs_allow: self.fs_allow.clone().into(),
            fs_refuse: self.fs_refuse.clone().into(),
            fs_granted: self.fs_granted.clone().into(),
            fs_denied: self.fs_denied.clone().into(),
            fs_request: self.fs_request.clone().into(),
            fs_revoke: self.fs_revoke.clone().into(),
            command_title: self.command_title.clone().into(),
            command_note: self.command_note.clone().into(),
            command_run: self.command_run.clone().into(),
            command_decline: self.command_decline.clone().into(),
            command_waiting: self.command_waiting.clone().into(),
            command_declined: self.command_declined.clone().into(),
            environment_title: self.environment_title.clone().into(),
            environment_note: self.environment_note.clone().into(),
            environment_refresh: self.environment_refresh.clone().into(),

            provider: self.provider.clone().into(),
            subscription_hint: self.subscription_hint.clone().into(),
            history_title: self.history_title.clone().into(),
            history_hint: self.history_hint.clone().into(),
            history_empty: self.history_empty.clone().into(),
            restore: self.restore.clone().into(),
            restore_started: self.restore_started.clone().into(),
            restore_done: self.restore_done.clone().into(),
            restore_failed: self.restore_failed.clone().into(),
            no_backup: self.no_backup.clone().into(),
            clients_notice: self.clients_notice.clone().into(),
            label_you: self.label_you.clone().into(),
            label_tool: self.label_tool.clone().into(),
            show_tool_details: self.show_tool_details.clone().into(),
            hide_tool_details: self.hide_tool_details.clone().into(),
            status_tools: self.status_tools.clone().into(),
            choice_done: self.choice_done.clone().into(),
            choice_skipped: self.choice_skipped.clone().into(),
            choice_selected: self.choice_selected.clone().into(),
            preview_missing: self.preview_missing.clone().into(),
            label_error: self.label_error.clone().into(),
            new_conversation: self.new_conversation.clone().into(),
            mcp_unavailable: self.mcp_unavailable.clone().into(),
            mcp_tools: self.mcp_tools.clone().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parité : mêmes clés, aucune chaîne vide, dans les deux langues.
    #[test]
    fn parite_fr_en() {
        let fr = serde_json::to_value(&tr(Lang::Fr)).unwrap();
        let en = serde_json::to_value(&tr(Lang::En)).unwrap();
        let fr_keys: Vec<&str> = fr.as_object().unwrap().keys().map(String::as_str).collect();
        let en_keys: Vec<&str> = en.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(fr_keys, en_keys, "les dictionnaires divergent");
        for (k, v) in fr.as_object().unwrap() {
            assert!(!v.as_str().unwrap().trim().is_empty(), "FR : {k} vide");
        }
        for (k, v) in en.as_object().unwrap() {
            assert!(!v.as_str().unwrap().trim().is_empty(), "EN : {k} vide");
        }
        // marqueurs {} cohérents entre langues
        assert_eq!(
            fr["mcp_tools"].as_str().unwrap().matches("{}").count(),
            en["mcp_tools"].as_str().unwrap().matches("{}").count()
        );
    }

    #[test]
    fn parsing_langue() {
        assert_eq!(Lang::parse("fr"), Lang::Fr);
        assert_eq!(Lang::parse("EN"), Lang::En);
        assert_eq!(Lang::parse("us"), Lang::En);
        assert_eq!(Lang::parse("n'importe"), Lang::Fr);
    }
}
