//! Outils MCP du panneau de configuration Hyprland.
//!
//! Philosophie : le serveur expose l'état, applique des changements à chaud,
//! valide et sauvegarde — l'agent chatbot édite lui-même `hyprland.lua` avec
//! ses outils fichiers natifs. Voir les instructions serveur pour le
//! workflow complet.
//!
//! Convention rmcp 3.x : les arguments arrivent via l'extracteur
//! `Parameters<T>` (T : Deserialize + JsonSchema).

use hyprpanel_core::{backup, hypr_files, ipc, timefmt};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ErrorData};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

pub struct Panel {
    pub state: AppState,
}

impl Panel {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

// ---------------------------------------------------------------------------
// Structures d'arguments
// ---------------------------------------------------------------------------

#[derive(Deserialize, schemars::JsonSchema)]
pub struct BindsArgs {
    /// Taille max de la sortie en caractères (défaut 6000).
    pub max_chars: Option<usize>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct OptionArgs {
    /// Chemin de l'option Hyprland, ex. `general:gaps_in`.
    pub option: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SetOptionArgs {
    /// Chemin de l'option Hyprland, ex. `general:gaps_in`.
    pub option: String,
    /// Valeur à appliquer, ex. `12` ou `5 5 10 10`.
    pub value: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DispatchArgs {
    /// Dispatcher typé (`hl.dsp.window.close()`) ou legacy (`movecursortocorner 0`).
    pub target: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct EvalArgs {
    /// Expression Lua évaluée dans l'environnement de config de Hyprland.
    pub lua: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct NotifyArgs {
    /// Texte affiché à l'écran.
    pub message: String,
    /// Durée d'affichage en ms (défaut 3000).
    pub timeout_ms: Option<u32>,
    /// Icône 0-4 (0 avertissement, 1 info, 2 indice, 3 ok, 4 erreur).
    pub icon: Option<u8>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct FindSettingArgs {
    /// Texte recherché dans les fichiers de config.
    pub needle: String,
    /// Nombre max d'occurrences renvoyées (défaut 50).
    pub max_results: Option<usize>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct BackupArgs {
    /// Étiquette optionnelle du backup (alphanumérique).
    pub label: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct RestoreBackupArgs {
    /// Nom du backup (voir hypr_list_backups).
    pub name: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct RecentEventsArgs {
    /// Nombre d'événements à renvoyer (défaut 20).
    pub count: Option<usize>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ScreenshotArgs {
    /// Moniteur à capturer (nom ex. DP-1) ; sinon tous les écrans.
    pub output: Option<String>,
}

// ---------------------------------------------------------------------------
// Aides
// ---------------------------------------------------------------------------

fn text(s: impl Into<String>) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![ContentBlock::text(s)]))
}

fn json_out(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn ipc_err(e: ipc::IpcError) -> ErrorData {
    ErrorData::internal_error(format!("IPC Hyprland indisponible : {e}"), None)
}

fn bad_param(msg: String) -> ErrorData {
    ErrorData::invalid_params(msg, None)
}

/// Tronque à une frontière de caractère UTF-8, avec mention de la coupe.
fn truncate(s: String, limit: usize) -> String {
    if s.len() <= limit {
        return s;
    }
    let mut cut = limit;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n…[tronqué à {cut} caractères sur {}]", &s[..cut], s.len())
}

/// Note rappelée partout où elle compte : getoption lit l'état fichiers.
const RUNTIME_OVERRIDE_NOTE: &str = "NB : getoption/hl.get_config lisent l'état chargé des FICHIERS, pas les overrides runtime — une option changée à chaud peut y afficher l'ancienne valeur alors qu'elle est bien appliquée.";

// ---------------------------------------------------------------------------
// Outils
// ---------------------------------------------------------------------------

#[tool_router]
impl Panel {
    /// Vue d'ensemble : version Hyprland, instance IPC, répertoire de config,
    /// nombre de fichiers, événements en mémoire. Point de départ d'une session.
    #[tool]
    pub async fn hypr_status(&self) -> Result<CallToolResult, ErrorData> {
        let version = ipc::query_json("version").map_err(ipc_err)?;
        let sig = ipc::instance_signature().map_err(ipc_err);
        let files = hypr_files::scan(&self.state.settings.hypr_config_dir);
        let out = json!({
            "hyprland": version,
            "instance": sig.ok(),
            "socket": ipc::socket_path().ok().map(|p| p.to_string_lossy().into_owned()),
            "config_dir": self.state.settings.hypr_config_dir,
            "config_files": files.len(),
            "events_buffered": self.state.events.len(),
            "serveur_demarre": timefmt::iso(self.state.started),
            "notes": "config Lua (API hl.*). monitors.lua/inputs.lua sont générés par un outil externe : lecture seule.",
        });
        text(json_out(&out))
    }

    /// Moniteurs actifs (nom, résolution, position, espace de travail courant) en JSON.
    #[tool]
    pub async fn hypr_monitors(&self) -> Result<CallToolResult, ErrorData> {
        let v = ipc::query_json("monitors").map_err(ipc_err)?;
        text(json_out(&v))
    }

    /// Espaces de travail existants (id, nom, moniteur, fenêtres) en JSON.
    #[tool]
    pub async fn hypr_workspaces(&self) -> Result<CallToolResult, ErrorData> {
        let v = ipc::query_json("workspaces").map_err(ipc_err)?;
        text(json_out(&v))
    }

    /// Fenêtres ouvertes (classe, titre, espace de travail, taille) en JSON.
    #[tool]
    pub async fn hypr_clients(&self) -> Result<CallToolResult, ErrorData> {
        let v = ipc::query_json("clients").map_err(ipc_err)?;
        text(json_out(&v))
    }

    /// Périphériques d'entrée (claviers, souris, écrans tactiles) en JSON.
    #[tool]
    pub async fn hypr_devices(&self) -> Result<CallToolResult, ErrorData> {
        let v = ipc::query_json("devices").map_err(ipc_err)?;
        text(json_out(&v))
    }

    /// Raccourcis clavier enregistrés. Sortie potentiellement énorme : elle
    /// est tronquée à `max_chars` (6000 par défaut) ; utilisez
    /// `hypr_find_setting` ou `hypr_eval` pour cibler un raccourci précis.
    #[tool]
    pub async fn hypr_binds(&self, args: Parameters<BindsArgs>) -> Result<CallToolResult, ErrorData> {
        let v = ipc::query_json("binds").map_err(ipc_err)?;
        let count = v.as_array().map(|a| a.len()).unwrap_or(0);
        let body = truncate(json_out(&v), args.0.max_chars.unwrap_or(6000));
        text(format!("({count} raccourcis)\n{body}"))
    }

    /// Lit une option de configuration (ex. `general:gaps_in`,
    /// `decoration:rounding`). Répond l'état chargé des fichiers — voir la
    /// note sur les overrides runtime.
    #[tool]
    pub async fn hypr_get_option(&self, args: Parameters<OptionArgs>) -> Result<CallToolResult, ErrorData> {
        let option = args.0.option.trim().to_string();
        if option.is_empty() || option.split_whitespace().count() > 1 {
            return Err(bad_param("option : chemin d'option Hyprland attendu, ex. general:gaps_in".into()));
        }
        let v = ipc::query_json(&format!("getoption {option}")).map_err(ipc_err)?;
        text(format!("{}\n{RUNTIME_OVERRIDE_NOTE}", json_out(&v)))
    }

    /// Applique une option À CHAUD (aperçu immédiat, NON persistant : perdu au
    /// prochain reload). Équivalent de l'ancien `hyprctl keyword`. Le
    /// compositeur répond `ok` même si l'option n'existe pas : vérifiez
    /// l'effet (comportement observé, `hypr_screenshot`). Pour persister,
    /// éditez `hyprland.lua` puis `hypr_reload`.
    #[tool]
    pub async fn hypr_set_option_live(
        &self,
        args: Parameters<SetOptionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let option = args.0.option.trim().to_string();
        if option.is_empty() {
            return Err(bad_param("option : chemin d'option Hyprland attendu, ex. general:gaps_in".into()));
        }
        let r = ipc::set_option_live(&option, args.0.value.trim()).map_err(ipc_err)?;
        text(format!(
            "réponse du compositeur : {r}\n\nAperçu non persistant appliqué. {RUNTIME_OVERRIDE_NOTE}\nPour persister : éditer hyprland.lua (voir hypr_config_files / hypr_find_setting) puis hypr_reload."
        ))
    }

    /// Invoque un dispatcher Hyprland. `target` accepte soit une expression
    /// typée (`hl.dsp.window.close()`, `hl.dsp.workspace.toggle_special()`,
    /// `hl.dsp.reload_config()`), soit la forme legacy `"nom args"`
    /// (`movecursortocorner 0`, `exec kitty`) enveloppée automatiquement.
    #[tool]
    pub async fn hypr_dispatch(&self, args: Parameters<DispatchArgs>) -> Result<CallToolResult, ErrorData> {
        let target = args.0.target.trim().to_string();
        if target.is_empty() {
            return Err(bad_param("target : expression hl.dsp.…(args) ou « nom args » legacy".into()));
        }
        let r = ipc::dispatch(&target).map_err(ipc_err)?;
        if r.starts_with("error") {
            text(format!("le compositeur a répondu : {r}"))
        } else {
            text(format!("ok : {r}"))
        }
    }

    /// Évalue une expression Lua DANS l'environnement de config de Hyprland et
    /// renvoie sa valeur (`hl.get_config("general:layout")`,
    /// `hl.get_monitors()`…). Les tables s'affichent `table: 0x…` :
    /// sérialisez-les en Lua (`local t={} for k,v in pairs(r) do … end`).
    /// Puissant mais sans filet : réservez-le aux lectures ciblées.
    #[tool]
    pub async fn hypr_eval(&self, args: Parameters<EvalArgs>) -> Result<CallToolResult, ErrorData> {
        let lua = args.0.lua.trim().to_string();
        if lua.is_empty() {
            return Err(bad_param("lua : expression Lua attendue".into()));
        }
        let r = ipc::repl(&lua).map_err(ipc_err)?;
        text(r)
    }

    /// Recharge la configuration depuis les fichiers et renvoie les éventuelles
    /// erreurs de parsing. À appeler après chaque édition de `hyprland.lua`.
    /// Attention : les overrides appliqués à chaud sont perdus.
    #[tool]
    pub async fn hypr_reload(&self) -> Result<CallToolResult, ErrorData> {
        let r = ipc::reload().map_err(ipc_err)?;
        let errors = ipc::config_errors().unwrap_or_default();
        let errors = if errors.trim().is_empty() {
            "aucune erreur de configuration".to_string()
        } else {
            format!("ERREURS :\n{errors}")
        };
        text(format!("reload : {r}\n{errors}\n\n(les overrides runtime ont été réinitialisés à l'état des fichiers)"))
    }

    /// Erreurs de configuration courantes (parsing, options inconnues), une
    /// par ligne. Vide = tout va bien.
    #[tool]
    pub async fn hypr_config_errors(&self) -> Result<CallToolResult, ErrorData> {
        let errors = ipc::config_errors().map_err(ipc_err)?;
        text(if errors.trim().is_empty() { "aucune erreur de configuration".into() } else { errors })
    }

    /// Affiche une notification à l'écran — retour visuel immédiat pour
    /// l'utilisateur pendant une série de changements.
    #[tool]
    pub async fn hypr_notify(&self, args: Parameters<NotifyArgs>) -> Result<CallToolResult, ErrorData> {
        let r = ipc::notify(args.0.icon.unwrap_or(1), args.0.timeout_ms.unwrap_or(3000), "0", &args.0.message)
            .map_err(ipc_err)?;
        text(format!("notification envoyée : {r}"))
    }

    /// Inventorie les fichiers de configuration Hyprland (`hyprland.lua`
    /// principal, modules Lua, .conf hyprpaper/hypridle/hyprlock, thèmes) et
    /// signale ceux générés par un outil externe (lecture seule). C'est la carte du
    /// terrain avant toute édition persistante.
    #[tool]
    pub async fn hypr_config_files(&self) -> Result<CallToolResult, ErrorData> {
        let files = hypr_files::scan(&self.state.settings.hypr_config_dir);
        let out = json!({
            "repertoire": self.state.settings.hypr_config_dir,
            "fichiers": files,
            "regles": [
                "éditer hyprland.lua (config principale, API hl.*) avec les outils fichiers natifs, puis hypr_reload",
                "monitors.lua et inputs.lua sont générés par un outil externe : NE PAS les éditer (écrasés au redémarrage du démon)",
                "hypr_backup avant une série de modifications, hypr_restore_backup pour revenir en arrière",
            ],
        });
        text(json_out(&out))
    }

    /// Localise où une option/réglage est défini dans les fichiers de config :
    /// renvoie les lignes correspondantes au format `fichier:ligne`. Ex. :
    /// aiguille `gaps_in`, `rounding`, `bind(`, `layerrule`.
    #[tool]
    pub async fn hypr_find_setting(
        &self,
        args: Parameters<FindSettingArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let needle = args.0.needle.trim().to_string();
        if needle.is_empty() {
            return Err(bad_param("needle : texte recherché attendu".into()));
        }
        let max = args.0.max_results.unwrap_or(50).clamp(1, 200);
        let matches = hypr_files::find_in_configs(&self.state.settings.hypr_config_dir, &needle, max);
        if matches.is_empty() {
            return text(format!("aucune occurrence de « {needle} » dans {}", self.state.settings.hypr_config_dir.display()));
        }
        let mut out = String::new();
        for m in &matches {
            out.push_str(&format!("{}:{}: {}\n", m.file, m.line, m.text));
        }
        out.push_str(&format!("\n({} occurrence(s), limite {})", matches.len(), max));
        text(out)
    }

    /// Crée une sauvegarde horodatée de tous les fichiers de config Hyprland.
    /// `label` optionnel (alphanumérique). À faire AVANT une série
    /// d'éditions persistantes.
    #[tool]
    pub async fn hypr_backup(&self, args: Parameters<BackupArgs>) -> Result<CallToolResult, ErrorData> {
        let name = backup::create_backup(
            &self.state.settings.hypr_config_dir,
            &self.state.settings.backups_dir,
            args.0.label.as_deref(),
        )
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        text(format!("backup créé : {name}"))
    }

    /// Liste les sauvegardes disponibles (du plus récent au plus ancien).
    #[tool]
    pub async fn hypr_list_backups(&self) -> Result<CallToolResult, ErrorData> {
        let list = backup::list_backups(&self.state.settings.backups_dir);
        if list.is_empty() {
            return text(format!("aucun backup dans {}", self.state.settings.backups_dir.display()));
        }
        text(json_out(&json!({ "backups": list })))
    }

    /// Restaure une sauvegarde dans la config active (une sauvegarde
    /// « pre-restore » est créée au préalable). Appeler `hypr_reload` ensuite.
    #[tool]
    pub async fn hypr_restore_backup(&self, args: Parameters<RestoreBackupArgs>) -> Result<CallToolResult, ErrorData> {
        let count = backup::restore_backup(
            &self.state.settings.hypr_config_dir,
            &self.state.settings.backups_dir,
            args.0.name.trim(),
        )
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        text(format!("{count} fichier(s) restaurés (une sauvegarde pre-restore a été créée). Appelez hypr_reload pour appliquer."))
    }

    /// Derniers événements Hyprland (changements d'espace de travail,
    /// fenêtres, couches…) captés depuis le socket d'événements :
    /// utile pour constater l'effet d'une action.
    #[tool]
    pub async fn hypr_recent_events(&self, args: Parameters<RecentEventsArgs>) -> Result<CallToolResult, ErrorData> {
        let entries = self.state.events.recent(args.0.count.unwrap_or(20).clamp(1, 200));
        if entries.is_empty() {
            return text("aucun événement en mémoire (buffer vide ou Hyprland silencieux)");
        }
        let mut out = String::new();
        for e in &entries {
            out.push_str(&format!("{} {}\n", e.timestamp, e.event));
        }
        text(out)
    }

    /// Capture d'écran PNG via `grim` — confirmation visuelle d'un changement
    /// d'apparence (gaps, bordures, disposition). `output` optionnel pour
    /// viser un moniteur précis (sinon tous). Renvoie le chemin du PNG :
    /// lisez le fichier pour l'observer.
    #[tool]
    pub async fn hypr_screenshot(&self, args: Parameters<ScreenshotArgs>) -> Result<CallToolResult, ErrorData> {
        let dir = &self.state.settings.screenshots_dir;
        std::fs::create_dir_all(dir)
            .map_err(|e| ErrorData::internal_error(format!("impossible de créer {} : {e}", dir.display()), None))?;
        let path = dir.join(format!("screenshot-{}.png", timefmt::compact(std::time::SystemTime::now())));
        let mut cmd = std::process::Command::new("grim");
        // Sous systemd user, WAYLAND_DISPLAY peut manquer : repli sur le nom
        // le plus courant (grim en a besoin, pas nous pour l'IPC).
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            cmd.env("WAYLAND_DISPLAY", "wayland-1");
        }
        if let Some(o) = args.0.output.as_deref().map(str::trim).filter(|o| !o.is_empty()) {
            cmd.arg("-o").arg(o);
        }
        cmd.arg(&path);
        let status = cmd.status().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ErrorData::internal_error("grim est absent du PATH — installez-le (pacman -S grim)", None)
            } else {
                ErrorData::internal_error(format!("grim : {e}"), None)
            }
        })?;
        if !status.success() {
            return Err(ErrorData::internal_error(format!("grim a échoué (code {status})"), None));
        }
        text(format!("capture écrite : {}\nLisez ce fichier image pour observer l'état de l'interface.", path.display()))
    }
}

/// Instructions transmises aux clients MCP (Claude Code, ZCode…) au handshake.
/// (Littéral requis par le macro `tool_handler`.)
// (version.workspace = 0.1.0 — le macro n'accepte que des littéraux)
#[tool_handler(name = "hyprpanel", version = "0.1.0", instructions = "Panneau de configuration Hyprland (configuration Lua, API hl.*).\n\nWORKFLOW RECOMMANDÉ\n1. hypr_status puis hypr_config_files pour situer la configuration.\n2. Tester un réglage À CHAUD avec hypr_set_option_live (aperçu non persistant) et confirmer visuellement (hypr_screenshot, hypr_recent_events).\n3. Pour persister : éditer ~/.config/hypr/hyprland.lua avec tes outils fichiers natifs (hypr_find_setting localise la ligne), puis hypr_reload et vérifier hypr_config_errors.\n4. hypr_backup avant une série d'éditions ; hypr_restore_backup + hypr_reload pour annuler.\n\nRÈGLES\n- monitors.lua et inputs.lua sont générés par un outil externe : NE JAMAIS les éditer.\n- getoption/hl.get_config lisent l'état des fichiers : un réglage appliqué à chaud peut y afficher l'ancienne valeur alors qu'il est actif.\n- hypr_set_option_live répond ok même pour une option inconnue : toujours vérifier l'effet réel.\n- Prévenir l'utilisateur avec hypr_notify avant un changement visible important.")]
impl ServerHandler for Panel {}
