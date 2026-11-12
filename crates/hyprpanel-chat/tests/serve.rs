//! Tests de bout en bout du mode `serve` : vrai binaire, faux client
//! `claude` dans un PATH isolé, HOME temporaire (aucun abonnement réel).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::{Value, json};

/// Faux Claude Code : connexion valide, réponses stream-json selon
/// LYRA_FAKE_MODE (text, tool, slow).
const FAKE_CLAUDE: &str = r#"#!/bin/sh
case "$*" in *"auth status"*) echo '{"loggedIn":true,"authMethod":"claude.ai"}'; exit 0;; esac
cat > /dev/null
reply() {
  printf '%s\n' "{\"type\":\"stream_event\",\"event\":{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"{\\\"text\\\":\\\"$1\\\",\\\"tool_calls\\\":[]}\"}}}"
  printf '%s\n' "{\"type\":\"result\",\"is_error\":false,\"structured_output\":{\"text\":\"$1\",\"tool_calls\":[]}}"
}
case "${LYRA_FAKE_MODE:-text}" in
  slow) exec sleep 30 ;;
  tool)
    if [ ! -e "$LYRA_FAKE_STATE" ]; then
      : > "$LYRA_FAKE_STATE"
      printf '%s\n' '{"type":"result","is_error":false,"structured_output":{"text":"","tool_calls":[{"name":"lyra_run_command","arguments":"{\"command\":\"echo lyra-test\",\"reason\":\"Vérifier le shell\"}"}]}}'
    else
      reply "Commande traitée."
    fi ;;
  *) reply "Bonjour, je suis LYRA." ;;
esac
"#;

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Value>,
    sandbox: std::sync::Arc<tempfile::TempDir>,
}

impl Session {
    fn start(mode: &str, args: &[&str]) -> Self {
        Self::start_in(
            std::sync::Arc::new(tempfile::tempdir().unwrap()),
            mode,
            args,
        )
    }

    /// Relance dans le même bac à sable (même HOME, mêmes décisions).
    fn start_in(sandbox: std::sync::Arc<tempfile::TempDir>, mode: &str, args: &[&str]) -> Self {
        let bin = sandbox.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        write_script(&bin.join("claude"), FAKE_CLAUDE);
        write_script(&bin.join("codex"), "#!/bin/sh\nexit 1\n");
        let home = sandbox.path().join("home");
        let mut child = Command::new(env!("CARGO_BIN_EXE_hyprpanel-chat"))
            .arg("serve")
            .args(["--provider", "claude_code"])
            .args(args)
            .env_clear()
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("LANG", "fr_FR.UTF-8")
            .env("LYRA_FAKE_MODE", mode)
            .env("LYRA_FAKE_STATE", sandbox.path().join("state"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.unwrap();
                // Rien d'autre que du JSON sur stdout.
                let value: Value = serde_json::from_str(&line)
                    .unwrap_or_else(|e| panic!("ligne non JSON sur stdout : {line} ({e})"));
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        let mut session = Self {
            child,
            stdin,
            lines,
            sandbox,
        };
        session.send(json!({"type":"hello","client":"test","protocol":1}));
        session
    }

    fn send(&mut self, message: Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    fn next(&self) -> Value {
        self.lines
            .recv_timeout(Duration::from_secs(60))
            .expect("message attendu dans les 60 s")
    }

    /// Lit jusqu'au premier message du type voulu ; les confirmations de
    /// l'inventaire de démarrage sont refusées au passage.
    fn until(&mut self, kind: &str, seen: &mut Vec<Value>) -> Value {
        loop {
            let message = self.next();
            seen.push(message.clone());
            if message["type"] == kind {
                return message;
            }
            if message["type"] == "confirm_request" {
                let id = message["id"].clone();
                self.send(json!({"type":"confirm","request":id,"accept":false}));
            }
        }
    }

    fn close(mut self) -> i32 {
        self.close_ref()
    }

    fn close_ref(&mut self) -> i32 {
        drop(self.stdin.take());
        for _ in 0..300 {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code().unwrap_or(-1);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        panic!("serve ne s'est pas arrêté après la fin de stdin");
    }
}

fn write_script(path: &Path, content: &str) {
    std::fs::write(path, content).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn types(messages: &[Value]) -> Vec<&str> {
    messages
        .iter()
        .map(|m| m["type"].as_str().unwrap())
        .collect()
}

#[test]
fn tour_complet_avec_fournisseur_factice() {
    let mut session = Session::start("text", &["--no-fs"]);
    session.send(json!({"type":"message","id":"m1","text":"bonjour","extra":1}));
    let mut seen = Vec::new();
    let end = session.until("turn_end", &mut seen);
    let kinds = types(&seen);
    assert_eq!(kinds[0], "ready", "{kinds:?}");
    assert_eq!(seen[0]["fs"], false);
    assert_eq!(seen[0]["provider"], "claude_code");
    let start = kinds.iter().position(|k| *k == "turn_start").unwrap();
    assert_eq!(seen[start]["id"], "m1");
    let streamed: String = seen
        .iter()
        .filter(|m| m["type"] == "delta")
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    assert_eq!(streamed, "Bonjour, je suis LYRA.");
    assert_eq!(end["id"], "m1");
    assert_eq!(end["text"], "Bonjour, je suis LYRA.");
    assert_eq!(session.close(), 0);
}

#[test]
fn message_invalide_non_fatal() {
    let mut session = Session::start("text", &["--no-fs"]);
    let stdin = session.stdin.as_mut().unwrap();
    writeln!(stdin, "pas du json").unwrap();
    let mut seen = Vec::new();
    let error = session.until("error", &mut seen);
    // Le service hyprpanel est absent du bac à sable : on cherche notre erreur.
    let error = if error["message"]
        .as_str()
        .unwrap()
        .starts_with("message invalide")
    {
        error
    } else {
        session.until("error", &mut seen)
    };
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .starts_with("message invalide")
    );
    assert_eq!(error["fatal"], false);
    // La session continue.
    session.send(json!({"type":"message","id":"m2","text":"encore"}));
    assert_eq!(session.until("turn_end", &mut seen)["id"], "m2");
    assert_eq!(session.close(), 0);
}

fn confirmation(accept: bool) -> Vec<Value> {
    let mut session = Session::start("tool", &["--fs"]);
    session.send(json!({"type":"message","id":"m1","text":"lance le test"}));
    let mut seen = Vec::new();
    let request = loop {
        let request = session.until("confirm_request", &mut seen);
        if request["command"] == "echo lyra-test" {
            break request;
        }
        // Inventaire de démarrage : refusé.
        session.send(json!({"type":"confirm","request":request["id"],"accept":false}));
    };
    assert_eq!(request["risk"], "low");
    // Pas de réponse : rien ne se passe.
    assert!(
        session
            .lines
            .recv_timeout(Duration::from_millis(800))
            .is_err(),
        "une confirmation n'est jamais acceptée par défaut"
    );
    session.send(json!({"type":"confirm","request":request["id"],"accept":accept}));
    session.until("turn_end", &mut seen);
    assert_eq!(session.close(), 0);
    seen
}

#[test]
fn confirmation_acceptee() {
    let seen = confirmation(true);
    let tool = seen
        .iter()
        .rfind(|m| m["type"] == "tool")
        .expect("événement outil");
    assert_eq!(tool["name"], "lyra_run_command");
    assert_eq!(tool["status"], "done");
    assert!(tool["result"].as_str().unwrap().contains("lyra-test"));
    assert_eq!(seen.last().unwrap()["text"], "Commande traitée.");
}

#[test]
fn confirmation_refusee() {
    let seen = confirmation(false);
    let tool = seen.iter().rfind(|m| m["type"] == "tool").unwrap();
    assert_eq!(tool["status"], "error");
    assert!(!tool["result"].as_str().unwrap().contains("lyra-test"));
}

#[test]
fn annulation_du_tour() {
    let mut session = Session::start("slow", &["--no-fs"]);
    session.send(json!({"type":"message","id":"m1","text":"long"}));
    let mut seen = Vec::new();
    session.until("turn_start", &mut seen);
    std::thread::sleep(Duration::from_millis(300));
    let started = std::time::Instant::now();
    session.send(json!({"type":"cancel"}));
    let end = session.until("turn_end", &mut seen);
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(end["id"], "m1");
    assert!(seen.iter().any(|m| m["type"] == "notice"));
    // La session reste utilisable après une annulation.
    session.send(json!({"type":"new_conversation"}));
    assert_eq!(session.close(), 0);
}

#[test]
fn fin_de_stdin_sans_message() {
    let mut session = Session::start("text", &["--no-fs"]);
    let mut seen = Vec::new();
    session.until("ready", &mut seen);
    assert_eq!(session.close(), 0);
}

#[test]
fn acces_fichiers_demande_par_le_protocole() {
    let mut session = Session::start("text", &[]);
    let request = session.next();
    assert_eq!(request["type"], "confirm_request");
    assert_eq!(request["id"], "fs");
    assert_eq!(request["risk"], "high");
    session.send(json!({"type":"confirm","request":"fs","accept":false}));
    let mut seen = Vec::new();
    let ready = session.until("ready", &mut seen);
    assert_eq!(ready["fs"], false);
    assert_eq!(session.close(), 0);
}

#[test]
fn decisions_memorisees_entre_deux_lancements() {
    // 1er lancement : accès fichiers demandé et accepté ; les éventuelles
    // requêtes d'inventaire (flatpak, pipx, cargo…) sont refusées.
    let mut first = Session::start("text", &[]);
    let request = first.next();
    assert_eq!(request["id"], "fs");
    first.send(json!({"type":"confirm","request":"fs","accept":true}));
    first.send(json!({"type":"message","id":"m1","text":"bonjour"}));
    let mut seen = Vec::new();
    assert_eq!(first.until("turn_end", &mut seen)["id"], "m1");
    assert_eq!(seen[0]["fs"], true);
    let sandbox = first.sandbox.clone();
    assert_eq!(first.close_ref(), 0);
    drop(first);

    // 2e lancement : plus aucune demande, accès fichiers déjà accordé.
    let mut second = Session::start_in(sandbox, "text", &[]);
    second.send(json!({"type":"message","id":"m2","text":"encore"}));
    let mut seen = Vec::new();
    loop {
        let message = second.next();
        assert_ne!(
            message["type"], "confirm_request",
            "rien ne doit être redemandé : {message}"
        );
        seen.push(message.clone());
        if message["type"] == "turn_end" {
            break;
        }
    }
    assert_eq!(seen[0]["type"], "ready");
    assert_eq!(seen[0]["fs"], true);
    assert_eq!(second.close_ref(), 0);
}
