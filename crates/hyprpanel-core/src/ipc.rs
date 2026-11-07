//! Client IPC direct pour Hyprland (socket Unix), sans sous-processus.
//!
//! Toutes les formes de requêtes ci-dessous ont été vérifiées empiriquement
//! sur Hyprland 0.57.1 (config Lua) :
//!
//! | Forme                 | Réponse    | Exemples                                    |
//! |-----------------------|------------|---------------------------------------------|
//! | `j/<requête>`         | JSON       | `monitors`, `workspaces`, `clients`, `devices`, `binds`, `version`, `getoption <chemin>`, `configerrors` |
//! | `<requête>` nue       | texte      | `reload`, `notify 3 2000 0 "msg"`, `configerrors` |
//! | `/eval <lua>`         | `ok`       | effets de bord seulement                    |
//! | `/repl <lua>`         | valeur     | retourne la valeur de l'expression (les tables affichent `table: 0x…`, à sérialiser en Lua) |
//! | `/dispatch <lua>`     | `ok`       | exécute `hl.dispatch(<lua>)`                |
//!
//! Pièges importants :
//! - l'ancien `hyprctl keyword` n'existe plus : le setter à chaud est
//!   `/dispatch hl.dsp.global("conf <option> <valeur>")`, **silencieusement
//!   ignoré** si l'option n'existe pas — toujours vérifier l'effet obtenu ;
//! - `getoption` / `hl.get_config` lisent l'état *chargé depuis les fichiers*,
//!   pas les overrides runtime : après un `conf` à chaud ils peuvent
//!   continuer d'afficher l'ancienne valeur (comportement observé, l'option
//!   est pourtant bien appliquée) ;
//! - les dispatchers typés sont préfixés par catégorie (`hl.dsp.window.close()`,
//!   `hl.dsp.workspace.toggle_special()`, `hl.dsp.reload_config()`, …) et
//!   s'exécutent via `/dispatch hl.dsp.…(args)`.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("aucune instance Hyprland trouvée dans {0} (pas de session active ?)")]
    NoInstance(PathBuf),
    #[error("impossible de lire $XDG_RUNTIME_DIR ni /run/user/$UID")]
    NoRuntimeDir,
    #[error("IPC Hyprland : {0}")]
    Io(#[from] std::io::Error),
}

/// Échappe une chaîne pour l'insérer dans un littéral Lua `"…"`.
pub fn lua_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

pub fn runtime_dir() -> Result<PathBuf, IpcError> {
    if let Some(x) = std::env::var_os("XDG_RUNTIME_DIR") {
        return Ok(PathBuf::from(x));
    }
    let uid = nix_uid();
    let p = PathBuf::from(format!("/run/user/{uid}"));
    if p.is_dir() {
        return Ok(p);
    }
    Err(IpcError::NoRuntimeDir)
}

fn nix_uid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1).map(|v| v.to_string()))
        })
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000)
}

/// Signature de l'instance vivante : `$HYPRLAND_INSTANCE_SIGNATURE` si
/// présente, sinon l'entrée la plus récente de `<runtime>/hypr/` qui possède
/// bien une `.socket.sock` (les services systemd n'héritent pas de l'env de
/// la session graphique).
pub fn instance_signature() -> Result<String, IpcError> {
    if let Ok(sig) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") {
        if !sig.is_empty() {
            return Ok(sig);
        }
    }
    let hypr_dir = runtime_dir()?.join("hypr");
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in std::fs::read_dir(&hypr_dir).map_err(|_| IpcError::NoInstance(hypr_dir.clone()))? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.join(".socket.sock").exists() {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(modified) = meta.modified() else { continue };
        if best.as_ref().is_none_or(|(t, _)| modified > *t) {
            best = Some((modified, entry.file_name().to_string_lossy().into_owned()));
        }
    }
    best.map(|(_, s)| s).ok_or(IpcError::NoInstance(hypr_dir))
}

pub fn instance_dir() -> Result<PathBuf, IpcError> {
    Ok(runtime_dir()?.join("hypr").join(instance_signature()?))
}

pub fn socket_path() -> Result<PathBuf, IpcError> {
    Ok(instance_dir()?.join(".socket.sock"))
}

pub fn event_socket_path() -> Result<PathBuf, IpcError> {
    Ok(instance_dir()?.join(".socket2.sock"))
}

/// Requête ponctuelle : ouvre la socket, écrit la commande, lit jusqu'à EOF.
pub fn request(cmd: &str) -> Result<String, IpcError> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(cmd.as_bytes())?;
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut buf = String::new();
    stream.read_to_string(&mut buf)?;
    Ok(buf.trim().to_string())
}

/// Requête JSON : préfixe `j/`, tolère une réponse non JSON (renvoyée telle
/// quelle dans une `Value::String`).
pub fn query_json(query: &str) -> Result<Value, IpcError> {
    let raw = request(&format!("j/{query}"))?;
    match serde_json::from_str(&raw) {
        Ok(v) => Ok(v),
        Err(_) => Ok(Value::String(raw)),
    }
}

/// Applique le préfixe `j/` en amont, pour les requêtes déjà formatées.
pub fn raw(cmd: &str) -> Result<String, IpcError> {
    request(cmd)
}

/// Évalue une expression Lua et RETOURNE sa valeur (`/repl`).
pub fn repl(expr: &str) -> Result<String, IpcError> {
    request(&format!("/repl {expr}"))
}

/// Exécute du Lua sans récupérer de valeur (`/eval`).
pub fn eval(code: &str) -> Result<String, IpcError> {
    request(&format!("/eval {code}"))
}

/// Construit la commande de setter d'option à chaud (équivalent de l'ancien
/// `hyprctl keyword`, disparu en 0.57).
pub fn build_set_option(path: &str, value: &str) -> String {
    format!("/dispatch hl.dsp.global(\"conf {} {}\")", lua_escape(path), lua_escape(value))
}

/// Applique une option à chaud. Répond `ok` même si l'option n'existe pas :
/// l'appelant doit vérifier l'effet (capture d'écran, comportement observé).
pub fn set_option_live(path: &str, value: &str) -> Result<String, IpcError> {
    request(&build_set_option(path, value))
}

/// Construit une commande `/dispatch` : cible typée (`hl.dsp.…(args)`) telle
/// quelle, ou dispatcher legacy `"nom args"` enveloppé dans `hl.dsp.global`.
pub fn build_dispatch(target: &str) -> String {
    let t = target.trim();
    if t.starts_with("hl.") {
        format!("/dispatch {t}")
    } else {
        format!("/dispatch hl.dsp.global(\"{}\")", lua_escape(t))
    }
}

pub fn dispatch(target: &str) -> Result<String, IpcError> {
    request(&build_dispatch(target))
}

/// Recharge la configuration depuis les fichiers. Les overrides runtime sont
/// perdus (l'état fichiers reprend la main).
pub fn reload() -> Result<String, IpcError> {
    request("reload")
}

/// Erreurs de configuration courantes (une par ligne, vide si rien à signaler).
pub fn config_errors() -> Result<String, IpcError> {
    request("configerrors")
}

/// Notification à l'écran (icône 0-4, timeout ms, couleur `0` = défaut).
pub fn notify(icon: u8, timeout_ms: u32, color: &str, message: &str) -> Result<String, IpcError> {
    request(&format!(
        "notify {} {} {} \"{}\"",
        icon.min(4),
        timeout_ms,
        lua_escape(color),
        lua_escape(message)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echappement_lua() {
        assert_eq!(lua_escape("simple"), "simple");
        assert_eq!(lua_escape(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(lua_escape("l1\nl2"), "l1\\nl2");
    }

    #[test]
    fn construction_commandes() {
        assert_eq!(
            build_set_option("general:gaps_in", "12"),
            "/dispatch hl.dsp.global(\"conf general:gaps_in 12\")"
        );
        // guillemet injecté dans la valeur : doit être échappé
        assert_eq!(
            build_set_option("a:b", "x\"y"),
            "/dispatch hl.dsp.global(\"conf a:b x\\\"y\")"
        );
        assert_eq!(build_dispatch("hl.dsp.window.close()"), "/dispatch hl.dsp.window.close()");
        assert_eq!(
            build_dispatch("movecursortocorner 0"),
            "/dispatch hl.dsp.global(\"movecursortocorner 0\")"
        );
        assert_eq!(
            build_dispatch(r#"exec "kitty""#),
            "/dispatch hl.dsp.global(\"exec \\\"kitty\\\"\")"
        );
    }
}
