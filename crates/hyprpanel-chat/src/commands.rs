//! Exécution selon la politique choisie ; chaque confirmation porte sur le contenu exact.
use crate::{ChatWindow, filesystem::FilesystemAccess};
use anyhow::{Context, ensure};
use langchain_rust::schemas::FunctionDefinition;
use serde::Deserialize;
use serde_json::{Value, json};
use slint::Weak;
use std::{
    future::Future,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI32, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncReadExt, sync::oneshot};

pub const TOOL: &str = "lyra_run_command";
const OUTPUT_LIMIT: u64 = 32 * 1024;

struct Pending {
    id: i32,
    answer: oneshot::Sender<bool>,
}
#[derive(Clone, Default)]
pub struct Approvals {
    pending: Arc<Mutex<Option<Pending>>>,
    next: Arc<AtomicI32>,
}

impl Approvals {
    fn begin(&self) -> anyhow::Result<(i32, oneshot::Receiver<bool>)> {
        let mut pending = self.pending.lock().expect("confirmation empoisonnée");
        ensure!(pending.is_none(), "une confirmation est déjà en attente");
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (answer, receiver) = oneshot::channel();
        *pending = Some(Pending { id, answer });
        Ok((id, receiver))
    }

    pub fn answer(&self, id: i32, approved: bool) {
        let mut pending = self.pending.lock().expect("confirmation empoisonnée");
        if pending.as_ref().is_some_and(|p| p.id == id) {
            if let Some(pending) = pending.take() {
                let _ = pending.answer.send(approved);
            }
        }
    }

    pub async fn request(&self, weak: &Weak<ChatWindow>, details: String, reason: String) -> bool {
        let Ok((id, receiver)) = self.begin() else {
            return false;
        };
        let weak = weak.clone();
        let pending = self.clone();
        if slint::invoke_from_event_loop(move || {
            let Some(app) = weak.upgrade() else {
                pending.answer(id, false);
                return;
            };
            // Ne jamais remplacer une autre demande d'autorisation.
            if app.get_overlay() != "none" && app.get_overlay() != "settings" {
                pending.answer(id, false);
                return;
            }
            app.set_command_id(id);
            app.set_command_return_overlay(app.get_overlay());
            app.set_command_text(details.into());
            app.set_command_reason(reason.into());
            app.set_activity("approval".into());
            app.set_status(app.get_tr().command_waiting);
            app.set_overlay("command".into());
        })
        .is_err()
        {
            self.answer(id, false);
        }
        receiver.await.unwrap_or(false)
    }
}

pub fn definition() -> FunctionDefinition {
    FunctionDefinition::new(
        TOOL,
        "Propose une commande shell et sa raison. La politique d'exécution des réglages décide : tout confirmer, FULL access, confirmer sudo/élévations et commandes indirectes, ou commandes à copier sans exécution. Si une confirmation est requise, seul le dialogue natif l'autorise. Ne pas retenter après refus. Pas de saisie interactive, timeout 60 s, sortie bornée.",
        json!({"type":"object","required":["command","reason"],"properties":{"command":{"type":"string","description":"Script /bin/sh exact, sans placeholder ni secret. Une opération cohérente par demande."},"reason":{"type":"string","description":"But concret et effets attendus, risques ou privilèges nécessaires, dans la langue de l'utilisateur."}}}),
    )
}

#[derive(Debug, Deserialize)]
pub struct Request {
    pub command: String,
    pub reason: String,
}
impl Request {
    pub fn parse(args: &Value) -> anyhow::Result<Self> {
        let request: Self = serde_json::from_value(args.clone())?;
        ensure!(
            !request.command.trim().is_empty()
                && request.command.len() <= 8192
                && !request.command.contains('\0'),
            "commande vide ou trop longue"
        );
        ensure!(
            !request.reason.trim().is_empty() && request.reason.len() <= 2000,
            "raison attendue (2000 octets maximum)"
        );
        Ok(request)
    }
}

#[cfg(unix)]
struct ProcessGroup(u32);
#[cfg(unix)]
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        // Le groupe est créé par notre enfant. Nettoie aussi les petits-enfants
        // en cas de timeout ou de tâches lancées en arrière-plan.
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}

pub async fn run<F: Future<Output = bool>>(
    request: Request,
    filesystem: &FilesystemAccess,
    confirmation: F,
) -> anyhow::Result<Value> {
    run_with_limit(request, filesystem, confirmation, OUTPUT_LIMIT).await
}

pub async fn run_inventory<F: Future<Output = bool>>(
    request: Request,
    filesystem: &FilesystemAccess,
    confirmation: F,
) -> anyhow::Result<Value> {
    run_with_limit(request, filesystem, confirmation, 32 * 1024 * 1024).await
}

async fn run_with_limit<F: Future<Output = bool>>(
    request: Request,
    filesystem: &FilesystemAccess,
    confirmation: F,
    output_limit: u64,
) -> anyhow::Result<Value> {
    let mode = filesystem.operation_mode();
    if !mode.can_operate() {
        return Ok(
            json!({"status":"manual_only", "command":request.command, "reason":request.reason, "executed":false, "instruction":"Command not executed. The host displays a copyable shell block. Explain it if useful without repeating it, and never claim success. Do not bypass copy-only mode with any other tool."}),
        );
    }
    filesystem.require_approval()?;
    if mode.confirm_command(&request.command) && !confirmation.await {
        return Ok(
            json!({"status":"declined","instruction":"The user declined. Do not execute, retry, split or rephrase this action to bypass refusal."}),
        );
    }
    filesystem.require_approval()?;
    ensure!(
        filesystem.operation_mode() == mode,
        "Le mode d'exécution a changé ; proposez de nouveau l'action"
    );
    let mut command = tokio::process::Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(&request.command)
        .current_dir(filesystem.root())
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Pas de démarrage de shell interactif, ni d'exposition des clés héritées.
    command.env_remove("ENV").env_remove("BASH_ENV");
    for (name, _) in std::env::vars_os() {
        let upper = name.to_string_lossy().to_ascii_uppercase();
        if ["TOKEN", "SECRET", "PASSWORD", "API_KEY", "APIKEY", "AUTH"]
            .iter()
            .any(|key| upper.contains(key))
        {
            command.env_remove(name);
        }
    }
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .context("démarrage de la commande confirmée")?;
    #[cfg(unix)]
    let _group = ProcessGroup(child.id().context("pid enfant absent")?);
    async fn capture<R: tokio::io::AsyncRead + Unpin>(
        reader: R,
        limit: u64,
    ) -> anyhow::Result<(String, bool)> {
        // Continuer à vider le pipe après la limite, sinon le processus se bloque.
        let mut reader = reader;
        let mut saved = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut truncated = false;
        loop {
            let count = reader.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            let keep = count.min((limit as usize).saturating_sub(saved.len()));
            saved.extend_from_slice(&buffer[..keep]);
            truncated |= keep < count;
        }
        Ok((String::from_utf8_lossy(&saved).into_owned(), truncated))
    }
    let stdout = child.stdout.take().context("stdout absent")?;
    let stderr = child.stderr.take().context("stderr absent")?;
    let outcome = tokio::time::timeout(Duration::from_secs(60), async {
        tokio::try_join!(
            async { Ok::<_, anyhow::Error>(child.wait().await?) },
            capture(stdout, output_limit),
            capture(stderr, OUTPUT_LIMIT)
        )
    })
    .await;
    match outcome {
        Ok(result) => {
            let (status, (stdout, out_cut), (stderr, err_cut)) = result?;
            Ok(
                json!({"status":"completed","success":status.success(),"exit_code":status.code(),"stdout":stdout,"stderr":stderr,"truncated":out_cut || err_cut}),
            )
        }
        Err(_) => {
            let _ = child.kill().await;
            Ok(
                json!({"status":"timeout","instruction":"Command exceeded 60 s and its process group was stopped. Partial changes may have occurred; do not claim success or retry without fresh approval."}),
            )
        }
    }
}

// Les opérations de fichiers gérées ne lancent pas de commande. Les requêtes
// et actions IPC Hyprland (ainsi que grim) passent par la confirmation native.
pub fn needs_mcp_approval(name: &str) -> bool {
    !matches!(
        name,
        "hypr_config_files"
            | "hypr_find_setting"
            | "hypr_backup"
            | "hypr_list_backups"
            | "hypr_recent_events"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn approvals_are_single_use_and_stale_clicks_are_ignored() {
        let approvals = Approvals::default();
        let (id, reply) = approvals.begin().unwrap();
        assert!(approvals.begin().is_err());
        approvals.answer(id + 1, true);
        approvals.answer(id, false);
        assert!(!reply.await.unwrap());
        let (next, reply) = approvals.begin().unwrap();
        approvals.answer(id, true);
        approvals.answer(next, true);
        assert!(reply.await.unwrap());
    }
    #[tokio::test]
    async fn refusal_never_starts_a_command_and_each_execution_needs_approval() {
        let dir = tempfile::tempdir().unwrap();
        let filesystem = FilesystemAccess::new(dir.path().into(), dir.path().join("backup"));
        let args = json!({"command":"printf tested > proof.txt; printf ok", "reason":"test"});
        assert!(
            run(Request::parse(&args).unwrap(), &filesystem, async { true })
                .await
                .is_err()
        );
        filesystem.set_approved(true);
        let result = run(Request::parse(&args).unwrap(), &filesystem, async { false })
            .await
            .unwrap();
        assert_eq!(result["status"], "declined");
        let result = run_inventory(Request::parse(&args).unwrap(), &filesystem, async { false })
            .await
            .unwrap();
        assert_eq!(result["status"], "declined");
        assert!(!dir.path().join("proof.txt").exists());
        let result = run(Request::parse(&args).unwrap(), &filesystem, async { true })
            .await
            .unwrap();
        assert_eq!(result["stdout"], "ok");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("proof.txt")).unwrap(),
            "tested"
        );
        assert!(Request::parse(&json!({"command":"x","reason":""})).is_err());
    }
    #[tokio::test]
    async fn full_and_ordinary_sudo_mode_skip_confirmation_while_copy_mode_never_runs() {
        use crate::operations::Mode;
        let dir = tempfile::tempdir().unwrap();
        let fs = FilesystemAccess::new(dir.path().into(), dir.path().join("backup"));
        let args = json!({"command":"printf done > proof.txt", "reason":"test policy"});
        fs.set_operation_mode(Mode::CopyOnly);
        for inventory in [false, true] {
            let request = Request::parse(&args).unwrap();
            let output = if inventory {
                run_inventory(request, &fs, async { panic!("copy mode must not ask") }).await
            } else {
                run(request, &fs, async { panic!("copy mode must not ask") }).await
            }
            .unwrap();
            assert_eq!(output["status"], "manual_only");
            assert_eq!(output["executed"], false);
            assert_eq!(output["command"], args["command"]);
            assert!(!dir.path().join("proof.txt").exists());
        }
        fs.set_approved(true);
        for mode in [Mode::FullAccess, Mode::SudoOnly] {
            fs.set_operation_mode(mode);
            let out = run(Request::parse(&args).unwrap(), &fs, async {
                panic!("ordinary command must not ask")
            })
            .await
            .unwrap();
            assert_eq!(out["success"], true);
        }
        fs.set_operation_mode(Mode::SudoOnly);
        let args = json!({"command":"sudo touch privileged-proof", "reason":"test refusal"});
        let out = run(Request::parse(&args).unwrap(), &fs, async { false })
            .await
            .unwrap();
        assert_eq!(out["status"], "declined");
        assert!(!dir.path().join("privileged-proof").exists());
    }
    #[tokio::test]
    async fn a_policy_change_during_confirmation_prevents_execution() {
        let dir = tempfile::tempdir().unwrap();
        let fs = FilesystemAccess::new(dir.path().into(), dir.path().join("backup"));
        fs.set_approved(true);
        let request = Request::parse(&json!({"command":"touch proof", "reason":"test"})).unwrap();
        assert!(
            run(request, &fs, async {
                fs.set_operation_mode(crate::operations::Mode::CopyOnly);
                true
            })
            .await
            .is_err()
        );
        assert!(!dir.path().join("proof").exists());
    }
}
