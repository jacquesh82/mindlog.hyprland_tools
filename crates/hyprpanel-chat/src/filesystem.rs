//! Accès explicite de l'assistant aux configurations. Les fournisseurs utilisent
//! les mêmes outils locaux ; leurs shells et outils fichiers natifs restent fermés.

use anyhow::{Context, bail, ensure};
use langchain_rust::schemas::FunctionDefinition;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub const LIST_TOOL: &str = "lyra_list_directory";
pub const READ_TOOL: &str = "lyra_read_file";
pub const WRITE_TOOL: &str = "lyra_write_file";
const MAX_TEXT_BYTES: usize = 64 * 1024;

pub struct FilesystemAccess {
    root: PathBuf,
    backups: PathBuf,
    private_directories: Vec<PathBuf>,
    approved: AtomicBool,
}

impl FilesystemAccess {
    pub fn default_root() -> PathBuf {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/root".into())).join(".config")
    }

    pub fn new(root: PathBuf, backups: PathBuf) -> Self {
        let private_directories = vec![root.join("hyprpanel")];
        Self {
            root,
            backups,
            private_directories,
            approved: AtomicBool::new(false),
        }
    }

    pub fn protect_directory(mut self, path: PathBuf) -> Self {
        self.private_directories.push(path);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn approved(&self) -> bool {
        self.approved.load(Ordering::Acquire)
    }
    pub fn set_approved(&self, approved: bool) {
        self.approved.store(approved, Ordering::Release);
    }

    pub fn require_approval(&self) -> anyhow::Result<()> {
        ensure!(
            self.approved(),
            "Accès aux fichiers non autorisé. Demandez l'autorisation dans les réglages de LYRA."
        );
        Ok(())
    }

    // Les entrées Lua/dispatch arbitraires pourraient contourner les outils
    // fichiers bornés. Les outils Hyprland spécialisés restent disponibles.
    pub fn allows_mcp(&self, name: &str) -> bool {
        self.approved()
            && matches!(
                name,
                "hypr_status"
                    | "hypr_monitors"
                    | "hypr_workspaces"
                    | "hypr_clients"
                    | "hypr_devices"
                    | "hypr_binds"
                    | "hypr_set_option_live"
                    | "hypr_notify"
                    | "hypr_config_files"
                    | "hypr_find_setting"
                    | "hypr_backup"
                    | "hypr_list_backups"
                    | "hypr_restore_backup"
                    | "hypr_reload"
                    | "hypr_config_errors"
                    | "hypr_get_option"
                    | "hypr_recent_events"
                    | "hypr_screenshot"
            )
    }

    pub fn is_tool(name: &str) -> bool {
        matches!(name, LIST_TOOL | READ_TOOL | WRITE_TOOL)
    }

    fn resolve(&self, input: &str, new_file: bool) -> anyhow::Result<PathBuf> {
        self.require_approval()?;
        let root = self
            .root
            .canonicalize()
            .context("dossier de configuration inaccessible")?;
        let alias;
        let input = if input == "~/.config" {
            self.root.as_path()
        } else if let Some(relative) = input.strip_prefix("~/.config/") {
            alias = self.root.join(relative);
            alias.as_path()
        } else {
            Path::new(input)
        };
        ensure!(
            !input
                .components()
                .any(|c| matches!(c, Component::ParentDir)),
            "les chemins avec .. sont interdits"
        );
        let requested = if input.is_absolute() {
            input.to_path_buf()
        } else {
            self.root.join(input)
        };
        let target = if new_file && !requested.try_exists()? {
            let parent = requested
                .parent()
                .context("dossier parent attendu")?
                .canonicalize()
                .context("le dossier parent doit exister")?;
            parent.join(requested.file_name().context("nom de fichier attendu")?)
        } else {
            requested.canonicalize().context("chemin inaccessible")?
        };
        ensure!(
            target.starts_with(&root),
            "chemin en dehors du dossier autorisé"
        );
        for private in &self.private_directories {
            ensure!(
                !target.starts_with(private)
                    && !private
                        .canonicalize()
                        .ok()
                        .is_some_and(|p| target.starts_with(p)),
                "les fichiers privés de LYRA (clés, jetons et réglages) ne sont pas accessibles à l'assistant"
            );
        }
        Ok(target)
    }

    pub fn check_preview(&self, path: &str) -> anyhow::Result<()> {
        let target = self.resolve(path, false)?;
        ensure!(target.is_file(), "image locale attendue");
        Ok(())
    }

    /// L'inventaire partage les mêmes limites de chemins que les outils LLM.
    pub(crate) fn inventory_path(&self, path: &Path) -> anyhow::Result<PathBuf> {
        self.resolve(&path.to_string_lossy(), false)
    }

    pub(crate) fn inventory_text(&self, path: &Path) -> anyhow::Result<String> {
        Self::read_text(&self.inventory_path(path)?)
    }

    fn read_text(path: &Path) -> anyhow::Result<String> {
        let file = std::fs::File::open(path)?;
        ensure!(file.metadata()?.is_file(), "fichier ordinaire attendu");
        let mut data = Vec::new();
        file.take((MAX_TEXT_BYTES + 1) as u64)
            .read_to_end(&mut data)?;
        ensure!(
            data.len() <= MAX_TEXT_BYTES,
            "fichier texte limité à 64 Kio"
        );
        ensure!(!data.contains(&0), "fichier texte UTF-8 attendu");
        String::from_utf8(data).context("fichier texte UTF-8 attendu")
    }

    fn digest(text: &str) -> String {
        Sha256::digest(text.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub fn call(&self, name: &str, args: &Value) -> anyhow::Result<Value> {
        self.require_approval()?;
        let input = args["path"].as_str().context("path attendu")?;
        match name {
            LIST_TOOL => {
                let path = self.resolve(input, false)?;
                ensure!(path.is_dir(), "dossier attendu");
                let mut entries = Vec::new();
                for entry in std::fs::read_dir(path)? {
                    let entry = entry?;
                    // Ne pas explorer les liens ou les fichiers privés pendant
                    // l'inventaire. Chaque lecture revalide le chemin complet.
                    let Ok(target) = self.resolve(&entry.path().to_string_lossy(), false) else {
                        continue;
                    };
                    let kind = if target.is_dir() {
                        "directory"
                    } else if target.is_file() {
                        "file"
                    } else {
                        continue;
                    };
                    entries.push(json!({"name":entry.file_name().to_string_lossy(),"kind":kind}));
                    if entries.len() == 200 {
                        break;
                    }
                }
                entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
                Ok(json!({"path":input,"entries":entries,"limit":200}))
            }
            READ_TOOL => {
                let path = self.resolve(input, false)?;
                let content = Self::read_text(&path)?;
                Ok(json!({"path":input,"sha256":Self::digest(&content),"content":content}))
            }
            WRITE_TOOL => {
                let path = self.resolve(input, true)?;
                ensure!(
                    !matches!(
                        path.file_name().and_then(|s| s.to_str()),
                        Some("monitors.lua" | "inputs.lua")
                    ),
                    "configuration générée : lecture seule"
                );
                let content = args["content"].as_str().context("content attendu")?;
                ensure!(
                    content.len() <= MAX_TEXT_BYTES && !content.contains('\0'),
                    "texte UTF-8 limité à 64 Kio"
                );
                let expected = args["expected_sha256"]
                    .as_str()
                    .context("expected_sha256 attendu (ou new pour créer)")?;
                let previous = if path.try_exists()? {
                    let old = Self::read_text(&path)?;
                    ensure!(
                        expected == Self::digest(&old),
                        "le fichier a changé ; relisez-le avant de modifier"
                    );
                    Some(old)
                } else {
                    ensure!(
                        expected == "new",
                        "le fichier n'existe pas ; utilisez expected_sha256=new pour le créer"
                    );
                    None
                };
                let backup = if let Some(old) = &previous {
                    std::fs::create_dir_all(&self.backups)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(
                            &self.backups,
                            std::fs::Permissions::from_mode(0o700),
                        )?;
                    }
                    let mut backup = tempfile::Builder::new()
                        .prefix("config-")
                        .suffix(".bak")
                        .tempfile_in(&self.backups)?;
                    backup.write_all(old.as_bytes())?;
                    backup.as_file().sync_all()?;
                    Some(backup.keep().map_err(|e| e.error)?.1)
                } else {
                    None
                };
                let mut pending = tempfile::NamedTempFile::new_in(
                    path.parent().context("dossier parent attendu")?,
                )?;
                if previous.is_some() {
                    pending
                        .as_file()
                        .set_permissions(std::fs::metadata(&path)?.permissions())?;
                }
                pending.write_all(content.as_bytes())?;
                pending.as_file().sync_all()?;
                self.require_approval()?;
                if previous.is_some() {
                    pending.persist(&path).map_err(|e| e.error)?;
                } else {
                    pending.persist_noclobber(&path).map_err(|e| e.error)?;
                }
                Ok(
                    json!({"path":input,"sha256":Self::digest(content),"backup":backup,"status":"written"}),
                )
            }
            _ => bail!("outil fichiers inconnu"),
        }
    }

    pub fn tool_definitions(&self) -> Vec<FunctionDefinition> {
        if !self.approved() {
            return Vec::new();
        }
        vec![
            FunctionDefinition::new(
                LIST_TOOL,
                "Liste un dossier dans ~/.config (200 entrées maximum). Aucun fichier privé de LYRA n'est exposé.",
                json!({"type":"object","required":["path"],"properties":{"path":{"type":"string","description":"Chemin relatif à ~/.config, ou chemin absolu dans ce dossier. Utilisez . pour sa racine."}}}),
            ),
            FunctionDefinition::new(
                READ_TOOL,
                "Lit un fichier de configuration UTF-8 dans ~/.config, 64 Kio maximum, et renvoie son sha256 pour une modification ultérieure.",
                json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}),
            ),
            FunctionDefinition::new(
                WRITE_TOOL,
                "Écrit un fichier UTF-8 dans ~/.config après demande explicite de l'utilisateur. Relisez le fichier et fournissez son sha256 actuel ; utilisez new pour un nouveau fichier dans un dossier existant. Une sauvegarde précède tout remplacement. monitors.lua/inputs.lua et les fichiers privés de LYRA sont protégés.",
                json!({"type":"object","required":["path","content","expected_sha256"],"properties":{"path":{"type":"string"},"content":{"type":"string"},"expected_sha256":{"type":"string"}}}),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, FilesystemAccess) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("config");
        std::fs::create_dir(&root).unwrap();
        let access = FilesystemAccess::new(root, dir.path().join("backups"));
        (dir, access)
    }

    #[test]
    fn consent_is_required_and_revocation_blocks_all_tools() {
        let (_dir, access) = fixture();
        std::fs::write(access.root.join("waybar.json"), "{}").unwrap();
        assert!(access.tool_definitions().is_empty());
        assert!(!access.allows_mcp("hypr_backup"));
        assert!(
            access
                .call(READ_TOOL, &json!({"path":"waybar.json"}))
                .is_err()
        );
        assert!(
            access
                .call(
                    WRITE_TOOL,
                    &json!({"path":"new.conf","content":"x","expected_sha256":"new"})
                )
                .is_err()
        );
        assert!(!access.root.join("new.conf").exists());
        access.set_approved(true);
        assert_eq!(access.tool_definitions().len(), 3);
        assert_eq!(
            access
                .call(READ_TOOL, &json!({"path":"~/.config/waybar.json"}))
                .unwrap()["content"],
            "{}"
        );
        assert!(access.allows_mcp("hypr_backup"));
        assert!(!access.allows_mcp("hypr_eval"));
        assert!(!access.allows_mcp("hypr_dispatch"));
        access.set_approved(false);
        assert!(access.call(LIST_TOOL, &json!({"path":"."})).is_err());
        assert!(!access.allows_mcp("hypr_restore_backup"));
    }

    #[test]
    fn paths_outside_scope_and_private_settings_are_not_read_or_written() {
        let (dir, access) = fixture();
        access.set_approved(true);
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, "outside").unwrap();
        std::fs::create_dir(access.root.join("hyprpanel")).unwrap();
        std::fs::write(
            access.root.join("hyprpanel/chat.toml"),
            "private test settings",
        )
        .unwrap();
        for path in [
            outside.to_string_lossy().to_string(),
            "../outside.txt".into(),
            "hyprpanel/chat.toml".into(),
        ] {
            assert!(access.call(READ_TOOL, &json!({"path":path})).is_err());
            assert!(
                access
                    .call(
                        WRITE_TOOL,
                        &json!({"path":path,"content":"changed","expected_sha256":"new"})
                    )
                    .is_err()
            );
        }
        let listing = access.call(LIST_TOOL, &json!({"path":"."})).unwrap();
        assert!(
            !listing["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["name"] == "hyprpanel")
        );
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "outside");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape_or_alias_private_credentials() {
        use std::os::unix::fs::symlink;
        let (dir, access) = fixture();
        access.set_approved(true);
        std::fs::create_dir(dir.path().join("outside")).unwrap();
        std::fs::write(dir.path().join("outside/key"), "outside").unwrap();
        symlink(dir.path().join("outside"), access.root.join("link")).unwrap();
        assert!(access.call(READ_TOOL, &json!({"path":"link/key"})).is_err());
        assert!(
            access
                .call(
                    WRITE_TOOL,
                    &json!({"path":"link/new","content":"x","expected_sha256":"new"})
                )
                .is_err()
        );
        std::fs::create_dir(access.root.join("private-real")).unwrap();
        std::fs::write(access.root.join("private-real/chat.toml"), "private").unwrap();
        symlink(
            access.root.join("private-real"),
            access.root.join("hyprpanel"),
        )
        .unwrap();
        assert!(
            access
                .call(READ_TOOL, &json!({"path":"private-real/chat.toml"}))
                .is_err()
        );
        assert!(!dir.path().join("outside/new").exists());
    }

    #[test]
    fn writes_require_current_hash_and_keep_a_backup() {
        let (_dir, access) = fixture();
        access.set_approved(true);
        let path = access.root.join("theme.conf");
        std::fs::write(&path, "old theme").unwrap();
        assert!(
            access
                .call(
                    WRITE_TOOL,
                    &json!({"path":"theme.conf","content":"new theme","expected_sha256":"stale"})
                )
                .is_err()
        );
        assert!(!access.backups.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old theme");
        let read = access
            .call(READ_TOOL, &json!({"path":"theme.conf"}))
            .unwrap();
        let result = access.call(WRITE_TOOL, &json!({"path":"theme.conf","content":"new theme","expected_sha256":read["sha256"]})).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new theme");
        assert_eq!(
            std::fs::read_to_string(result["backup"].as_str().unwrap()).unwrap(),
            "old theme"
        );
        access
            .call(
                WRITE_TOOL,
                &json!({"path":"new.conf","content":"created","expected_sha256":"new"}),
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(access.root.join("new.conf")).unwrap(),
            "created"
        );
    }

    #[test]
    fn generated_files_and_oversized_or_binary_text_stay_protected() {
        let (_dir, access) = fixture();
        access.set_approved(true);
        std::fs::write(access.root.join("monitors.lua"), "generated").unwrap();
        assert!(access.call(WRITE_TOOL, &json!({"path":"monitors.lua","content":"changed","expected_sha256":FilesystemAccess::digest("generated")})).is_err());
        assert_eq!(
            std::fs::read_to_string(access.root.join("monitors.lua")).unwrap(),
            "generated"
        );
        std::fs::write(
            access.root.join("large.conf"),
            vec![b'x'; MAX_TEXT_BYTES + 1],
        )
        .unwrap();
        assert!(
            access
                .call(READ_TOOL, &json!({"path":"large.conf"}))
                .is_err()
        );
        std::fs::write(access.root.join("binary"), [0, 1, 2]).unwrap();
        assert!(access.call(READ_TOOL, &json!({"path":"binary"})).is_err());
    }
}
