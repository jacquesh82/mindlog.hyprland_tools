//! Outils MCP de hyprdesk (le bureau pour Hyprland) : chacun appelle la ligne
//! de commande `hyprdesk`, qui fait la même chose que ses Réglages. Le démon
//! hyprdesk relit sa configuration aussitôt (settings.lua surveillé).
//!
//! Couverture : état et modes, Réglages (ouvrir une page), tous les réglages
//! (`hyprdesk config list|get|set|unset|reset`), vérification de la
//! configuration, raccourcis et conflits, dépendances, contexte, démarrage
//! automatique, ménage de hyprland.lua, places des fenêtres, icônes du
//! bureau, thème des applications, couleurs des écrans, clavier, écran de
//! connexion.

use std::path::PathBuf;
use std::process::Command;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ErrorData};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use crate::tools::Panel;

/// Binaire de hyprdesk : `HYPRDESK_BIN`, sinon le PATH, sinon les
/// emplacements d'installation (le service systemd n'a pas toujours
/// ~/.local/bin dans son PATH).
fn hyprdesk_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("HYPRDESK_BIN").map(PathBuf::from)
        && p.is_file()
    {
        return Some(p);
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .chain([
            home.join(".local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/usr/local/bin"),
        ])
        .map(|d| d.join("hyprdesk"))
        .find(|p| p.is_file())
}

/// Lance `hyprdesk <args>` ; sa sortie (ou son erreur) devient la réponse.
fn run(args: &[&str]) -> Result<CallToolResult, ErrorData> {
    let bin = hyprdesk_bin().ok_or_else(|| {
        ErrorData::internal_error(
            "hyprdesk introuvable (ni HYPRDESK_BIN, ni PATH, ni ~/.local/bin) : est-il installé ?",
            None,
        )
    })?;
    let out = Command::new(&bin)
        .args(args)
        .output()
        .map_err(|e| ErrorData::internal_error(format!("{} : {e}", bin.display()), None))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim_end().to_string();
    let body = match (stdout.is_empty(), stderr.is_empty()) {
        (false, true) => stdout,
        (true, false) => stderr,
        (true, true) => "ok".to_string(),
        (false, false) => format!("{stdout}\n{stderr}"),
    };
    let body = if body.len() > 20000 {
        let mut cut = 20000;
        while !body.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}\n…[tronqué]", &body[..cut])
    } else {
        body
    };
    if out.status.success() {
        Ok(CallToolResult::success(vec![ContentBlock::text(body)]))
    } else {
        Ok(CallToolResult::error(vec![ContentBlock::text(format!(
            "hyprdesk {} : échec ({})\n{body}",
            args.join(" "),
            out.status
        ))]))
    }
}

fn bad(msg: &str) -> ErrorData {
    ErrorData::invalid_params(msg.to_string(), None)
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ModeArgs {
    /// `no-desktop` (bureau masqué), `launcher-only` (fenêtres et lanceur) ou
    /// `full-desktop` (bureau complet).
    pub mode: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SettingsArgs {
    /// Page des Réglages (`desktop`, `icons`, `widgets`, `wallpaper`, `theme`,
    /// `apptheme`, `bar`, `launcher`, `screens`, `colors`, `workspaces`, `layouts`,
    /// `autostart`, `hyprconfig`, `assistant`, `about`…), éventuellement
    /// `page/onglet` (ex. `desktop/raccourcis`), ou `?mots` pour ouvrir la
    /// recherche des Réglages (ex. `?gamma`, réglages correspondants surlignés).
    /// Vide : Bureau.
    pub page: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ConfigListArgs {
    /// Paquet dont lister les réglages (`desktop`, `bar`, `launcher`…) ; vide : tous.
    pub package: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ConfigKeyArgs {
    /// Clé de réglage (voir hyprdesk_config_list), ex. `show_on`, `bar.position`,
    /// `widget.clock`, `assign.firefox`, `theme.accent`.
    pub key: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ConfigSetArgs {
    /// Clé de réglage (voir hyprdesk_config_list).
    pub key: String,
    /// Valeur (ex. `focused`, `on`, `#ff79c6`, `3 silent`).
    pub value: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ConfirmArgs {
    /// Doit valoir `true` : l'utilisateur a confirmé.
    pub confirm: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct AutostartArgs {
    /// `list`, `add`, `remove`, `on` ou `off`.
    pub action: String,
    /// Commande visée (sauf pour `list`), telle qu'affichée par `list`.
    pub command: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct HyprlandFixArgs {
    /// Toutes les corrections sûres (`Tout ranger`).
    pub safe: Option<bool>,
    /// Lignes de hyprland.lua à corriger (numéros donnés par hyprdesk_hyprland_check).
    pub lines: Option<Vec<usize>>,
    /// Supprimer les vieilles copies de sauvegarde.
    pub backups: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ForgetArgs {
    /// `true` : oublier les places (sinon : les lister).
    pub forget: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct IconsArgs {
    /// `list`, `add` ou `remove`.
    pub action: String,
    /// Pour `add` : `app` (identifiant .desktop), `path` (fichier ou dossier) ou `exec` (commande).
    pub kind: Option<String>,
    /// Pour `add` : l'identifiant, le chemin ou la commande ; pour `remove` : l'id de l'icône (voir `list`).
    pub value: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct AppThemeArgs {
    /// `status` (applications et fichiers) ou `apply` (réappliquer le thème).
    pub action: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct KeyboardArgs {
    /// Disposition xkb (ex. `fr`, `us`, `fr,us`) ; vide : affiche le clavier en place.
    pub layout: Option<String>,
    /// Variante (ex. `azerty`, `oss`, `bepo`).
    pub variant: Option<String>,
    /// Options xkb (ex. `grp:alt_shift_toggle`).
    pub options: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ScreenColorsArgs {
    /// `list`, `set`, `reset` ou `calibration`.
    pub action: String,
    /// Écran (connecteur, ex. `DP-1`) ; requis sauf pour `list`.
    pub monitor: Option<String>,
    /// Pour `set` : `brightness` (−0.5…0.5), `contrast` (0.5…1.5), `gamma`
    /// (0.4…2.5), `red` / `green` / `blue` (gains 0.5…1.5), `gamma_red` /
    /// `gamma_green` / `gamma_blue` (0.4…2.5).
    pub key: Option<String>,
    /// Pour `set` : la valeur (neutre : 0 pour brightness, 1 sinon).
    pub value: Option<f64>,
    /// Pour `calibration` : fichier .icc, .icm ou .cal (copié dans la
    /// configuration), ou `none` pour le retirer.
    pub file: Option<String>,
}

// ---------------------------------------------------------------------------
// Outils
// ---------------------------------------------------------------------------

#[tool_router(router = hyprdesk_router, vis = "pub(crate)")]
impl Panel {
    /// hyprdesk : version, mode du bureau en cours (no-desktop, launcher-only,
    /// full-desktop), écrans et workspaces, configuration valide ou non.
    /// Point de départ avant d'agir sur hyprdesk.
    #[tool]
    pub async fn hyprdesk_status(&self) -> Result<CallToolResult, ErrorData> {
        run(&["status"])
    }

    /// Change le mode du bureau hyprdesk : `no-desktop`, `launcher-only` ou `full-desktop`.
    #[tool]
    pub async fn hyprdesk_set_mode(
        &self,
        args: Parameters<ModeArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let cmd = match args.0.mode.as_str() {
            "no-desktop" | "hide" => "hide",
            "launcher-only" => "launcher-only",
            "full-desktop" | "show" => "show",
            _ => return Err(bad("mode : no-desktop, launcher-only ou full-desktop")),
        };
        run(&[cmd])
    }

    /// Ouvre les Réglages de hyprdesk sur une page (pour montrer un réglage à
    /// l'utilisateur), sans changer le mode du bureau.
    #[tool]
    pub async fn hyprdesk_open_settings(
        &self,
        args: Parameters<SettingsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match args.0.page.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) => run(&["settings", p]),
            None => run(&["settings"]),
        }
    }

    /// Liste les réglages de hyprdesk et leur valeur (par paquet : desktop,
    /// icons, launcher, bar, lock, theme, wallpaper, workspaces, assistant…),
    /// avec leurs valeurs possibles. À lire avant hyprdesk_config_set.
    #[tool]
    pub async fn hyprdesk_config_list(
        &self,
        args: Parameters<ConfigListArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match args.0.package.as_deref().filter(|p| !p.is_empty()) {
            Some(p) => run(&["config", "list", p]),
            None => run(&["config", "list"]),
        }
    }

    /// Valeur d'un réglage de hyprdesk.
    #[tool]
    pub async fn hyprdesk_config_get(
        &self,
        args: Parameters<ConfigKeyArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run(&["config", "get", &args.0.key])
    }

    /// Change un réglage de hyprdesk (même effet que ses Réglages ; appliqué
    /// aussitôt). Ex. `show_on focused`, `icon_scope workspace`,
    /// `widget.clock on`, `assign.firefox 2`, `hyprbars on`, `running_apps workspace`.
    #[tool]
    pub async fn hyprdesk_config_set(
        &self,
        args: Parameters<ConfigSetArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run(&["config", "set", &args.0.key, &args.0.value])
    }

    /// Rend un réglage à sa valeur par défaut (ou à celle de hyprdesk.lua).
    #[tool]
    pub async fn hyprdesk_config_unset(
        &self,
        args: Parameters<ConfigKeyArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        run(&["config", "unset", &args.0.key])
    }

    /// Remet TOUTE la configuration de hyprdesk par défaut (hyprdesk.lua
    /// d'exemple, réglages effacés ; copie gardée). Seulement après accord
    /// explicite de l'utilisateur : `confirm = true`.
    #[tool]
    pub async fn hyprdesk_config_reset(
        &self,
        args: Parameters<ConfirmArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if !args.0.confirm {
            return Err(bad("confirm = true requis, après accord de l'utilisateur"));
        }
        run(&["config", "reset", "--yes"])
    }

    /// Vérifie la configuration de hyprdesk (hyprdesk.lua et settings.lua) sans
    /// toucher au démon : erreurs avec fichier:ligne, ou le résumé.
    #[tool]
    pub async fn hyprdesk_check_config(&self) -> Result<CallToolResult, ErrorData> {
        run(&["check"])
    }

    /// Raccourcis de hyprdesk, et leurs conflits avec hyprland.lua (ligne en cause).
    #[tool]
    pub async fn hyprdesk_keys(&self) -> Result<CallToolResult, ErrorData> {
        run(&["keys"])
    }

    /// Dépendances externes de hyprdesk : présentes, absentes (avec le paquet),
    /// et la fonction qui en dépend.
    #[tool]
    pub async fn hyprdesk_doctor(&self) -> Result<CallToolResult, ErrorData> {
        run(&["doctor"])
    }

    /// Description complète de l'environnement hyprdesk (Markdown) : modes,
    /// paquets, applications, écrans, workspaces, widgets, fichiers, commandes.
    #[tool]
    pub async fn hyprdesk_context(&self) -> Result<CallToolResult, ErrorData> {
        run(&["context"])
    }

    /// Démarrage automatique de la session : programmes lancés par hyprdesk
    /// (autostart.lua), lignes du bloc de démarrage de hyprland.lua, applications
    /// XDG. Actions : `list`, `add`, `remove`, `on`, `off` (commande telle que listée).
    #[tool]
    pub async fn hyprdesk_autostart(
        &self,
        args: Parameters<AutostartArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let a = &args.0;
        match a.action.as_str() {
            "list" => run(&["autostart", "list"]),
            v @ ("add" | "remove" | "on" | "off") => {
                let cmd = a
                    .command
                    .as_deref()
                    .filter(|c| !c.trim().is_empty())
                    .ok_or_else(|| bad("command requis"))?;
                run(&["autostart", v, cmd])
            }
            _ => Err(bad("action : list, add, remove, on ou off")),
        }
    }

    /// Ménage de hyprland.lua (JSON) : erreurs, règles de workspace en
    /// conflit ou vers un écran absent, raccourcis et démarrages en double,
    /// programmes absents ou faisant doublon avec hyprdesk, require absents,
    /// lignes déjà coupées, vieilles copies ; chaque point : ligne, correction, sûr ou non.
    #[tool]
    pub async fn hyprdesk_hyprland_check(&self) -> Result<CallToolResult, ErrorData> {
        run(&["hyprland-check", "--json"])
    }

    /// Applique des corrections du ménage de hyprland.lua (copie d'abord) :
    /// `safe` (toutes les sûres), `lines` (numéros de hyprdesk_hyprland_check),
    /// `backups` (vieilles copies). Demander l'accord de l'utilisateur avant.
    #[tool]
    pub async fn hyprdesk_hyprland_fix(
        &self,
        args: Parameters<HyprlandFixArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let a = &args.0;
        let mut v: Vec<String> = Vec::new();
        if a.safe == Some(true) {
            v.push("--safe".into());
        }
        if a.backups == Some(true) {
            v.push("backups".into());
        }
        for l in a.lines.iter().flatten() {
            v.push(l.to_string());
        }
        if v.is_empty() {
            return Err(bad("safe, lines ou backups requis"));
        }
        let mut args = vec!["hyprland-fix"];
        args.extend(v.iter().map(String::as_str));
        run(&args)
    }

    /// Places des fenêtres choisies par l'utilisateur en plein bureau (par
    /// workspace et application) ; `forget = true` : les oublier.
    #[tool]
    pub async fn hyprdesk_window_places(
        &self,
        args: Parameters<ForgetArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if args.0.forget == Some(true) {
            run(&["window-places", "forget"])
        } else {
            run(&["window-places"])
        }
    }

    /// Icônes du bureau : `list` (portée, icônes et leur origine), `add`
    /// (kind `app` | `path` | `exec`, value), `remove` (value = id de l'icône).
    #[tool]
    pub async fn hyprdesk_desktop_icons(
        &self,
        args: Parameters<IconsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let a = &args.0;
        let value = a.value.as_deref().unwrap_or("").trim();
        match a.action.as_str() {
            "list" => run(&["icons", "list"]),
            "add" => {
                let kind = a.kind.as_deref().unwrap_or("");
                if !matches!(kind, "app" | "path" | "exec") || value.is_empty() {
                    return Err(bad("add : kind app|path|exec et value requis"));
                }
                run(&["icons", "add", kind, value])
            }
            "remove" => {
                if value.is_empty() {
                    return Err(bad("remove : value = id de l'icône"));
                }
                run(&["icons", "remove", value])
            }
            _ => Err(bad("action : list, add ou remove")),
        }
    }

    /// Thème de hyprdesk appliqué à d'autres applications (kitty, Thunar) :
    /// `status`, ou `apply` pour le réappliquer. Les applications cochées se
    /// règlent avec hyprdesk_config_set (clé `app_themes`).
    #[tool]
    pub async fn hyprdesk_app_theme(
        &self,
        args: Parameters<AppThemeArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match args.0.action.as_deref().unwrap_or("status") {
            "status" => run(&["app-theme", "status"]),
            "apply" => run(&["app-theme", "apply"]),
            _ => Err(bad("action : status ou apply")),
        }
    }

    /// Couleurs de chaque écran (appliquées aussitôt par une table gamma, et
    /// gardées) : `list`, `set` (monitor, key, value), `reset` (monitor),
    /// `calibration` (monitor, file .icc/.icm/.cal ou `none`).
    #[tool]
    pub async fn hyprdesk_screen_colors(
        &self,
        args: Parameters<ScreenColorsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let a = &args.0;
        let mon = a.monitor.as_deref().unwrap_or("").trim();
        let need_mon = || {
            if mon.is_empty() { Err(bad("monitor requis")) } else { Ok(()) }
        };
        match a.action.as_str() {
            "list" => run(&["colors", "list"]),
            "set" => {
                need_mon()?;
                let key = a.key.as_deref().ok_or_else(|| bad("key requis"))?;
                let v = a.value.ok_or_else(|| bad("value requis"))?;
                run(&["colors", "set", mon, key, &v.to_string()])
            }
            "reset" => {
                need_mon()?;
                run(&["colors", "reset", mon])
            }
            "calibration" => {
                need_mon()?;
                let f = a.file.as_deref().filter(|f| !f.trim().is_empty()).ok_or_else(|| bad("file requis (ou none)"))?;
                run(&["colors", "calibration", mon, f])
            }
            _ => Err(bad("action : list, set, reset ou calibration")),
        }
    }

    /// Ouvre (ou ferme) la mire plein écran des couleurs sur tous les écrans,
    /// avec les boutons de réglage de chaque écran pour comparer leur rendu.
    #[tool]
    pub async fn hyprdesk_color_test(&self) -> Result<CallToolResult, ErrorData> {
        run(&["color-test"])
    }

    /// Clavier de la session (inputs.lua, tenu par hyprdesk) : sans `layout`,
    /// affiche celui en place ; sinon l'applique à chaud et l'enregistre.
    #[tool]
    pub async fn hyprdesk_keyboard(
        &self,
        args: Parameters<KeyboardArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let a = &args.0;
        match a.layout.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
            None => run(&["keyboard"]),
            Some(l) => run(&[
                "keyboard",
                l,
                a.variant.as_deref().unwrap_or(""),
                a.options.as_deref().unwrap_or(""),
            ]),
        }
    }

    /// Écran de connexion de hyprdesk (greetd) : état de l'installation.
    #[tool]
    pub async fn hyprdesk_greeter_status(&self) -> Result<CallToolResult, ErrorData> {
        run(&["greeter", "setup", "status"])
    }

    /// Lignes conseillées pour hyprland.lua (raccourcis à garder sur le bureau, flou de la barre).
    #[tool]
    pub async fn hyprdesk_snippet(&self) -> Result<CallToolResult, ErrorData> {
        run(&["snippet"])
    }

    /// Fait relire sa configuration au démon hyprdesk.
    #[tool]
    pub async fn hyprdesk_reload(&self) -> Result<CallToolResult, ErrorData> {
        run(&["reload"])
    }
}
