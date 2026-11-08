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
• Pièces jointes : bouton « Joindre » — images (elle les voit) et fichiers texte (elle les lit).
• Captures d'écran (grim) pour constater visuellement un changement, notifications à l'écran.

GARDE-FOUS
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
• Attachments: the “Attach” button — images (she sees them) and text files (she reads them).
• Screenshots (grim) to visually confirm a change, on-screen notifications.

SAFETY
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
        welcome_l3: "Vous pouvez joindre des images et des fichiers texte avec « Joindre ».".into(),
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
        settings_context: "Contexte SYSTEM".into(),
        settings_context_hint: "Instructions communes aux trois fournisseurs. Laissez vide pour le rôle LYRA par défaut ; votre texte le remplace. Le périmètre Hyprland/Wayland et les consignes des outils restent appliqués. 16 000 caractères maximum.".into(),
        settings_context_invalid: "Le contexte SYSTEM doit contenir au maximum 16 000 caractères.".into(),
        fs_title: "Autoriser l’accès aux fichiers ?".into(),
        fs_body: "Autorisez-vous l’assistant LYRA à consulter et modifier vos fichiers de configuration ? Leur contenu peut être transmis au fournisseur sélectionné pour traiter vos demandes.".into(),
        fs_note: "Les fichiers privés de LYRA et les configurations générées restent protégés. Sauvegardes et captures utilisent les dossiers de données de LYRA. Cette autorisation vaut pour la session et peut être retirée dans les réglages. Le retrait remet à zéro le contexte du modèle. Les fichiers joints explicitement restent utilisables si vous refusez.".into(),
        fs_allow: "Autoriser l’accès".into(),
        fs_refuse: "Refuser".into(),
        fs_granted: "FS : accès autorisé".into(),
        fs_denied: "FS : accès non autorisé".into(),
        fs_request: "Autoriser l’accès aux fichiers…".into(),
        fs_revoke: "Retirer l’accès aux fichiers".into(),

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
        status_tools: "Vérification du bureau…".into(),
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
        welcome_l3: "You can attach images and text files with “Attach”.".into(),
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
        settings_context: "SYSTEM context".into(),
        settings_context_hint: "Instructions shared by all three providers. Leave empty for LYRA’s default role; your text replaces it. Hyprland/Wayland scope and tool instructions still apply. Maximum 16,000 characters.".into(),
        settings_context_invalid: "The SYSTEM context must not exceed 16,000 characters.".into(),
        fs_title: "Allow filesystem access?".into(),
        fs_body: "Allow the LYRA assistant to read and edit your configuration files? Their contents may be sent to the selected provider to handle your requests.".into(),
        fs_note: "LYRA’s private files and generated configurations stay protected. Backups and screenshots use LYRA’s data directories. Access lasts for this session and can be revoked in Settings. Revocation resets the model’s conversation context. Explicitly attached files remain available if you decline.".into(),
        fs_allow: "Allow access".into(),
        fs_refuse: "Decline".into(),
        fs_granted: "FS: access allowed".into(),
        fs_denied: "FS: access not allowed".into(),
        fs_request: "Allow filesystem access…".into(),
        fs_revoke: "Revoke filesystem access".into(),

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
        status_tools: "Checking your desktop…".into(),
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
