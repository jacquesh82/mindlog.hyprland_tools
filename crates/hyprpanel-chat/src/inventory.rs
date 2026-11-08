//! Inventaire observé : paquets installés + lanceurs + références de configuration.
//! Les chaînes Exec/command lues ici ne sont JAMAIS exécutées.
use crate::filesystem::FilesystemAccess;
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
};

const MAX_PACKAGES: usize = 20_000;
const MAX_TOOLS: usize = 256;
const MAX_ENTRIES: usize = 6_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tool {
    pub name: String,
    pub manager: Option<String>,
    pub package: Option<String>,
    pub version: Option<String>,
    pub version_source: Option<String>,
    pub executables: Vec<String>,
    pub config_files: Vec<String>,
    #[serde(default)]
    pub referenced_by: Vec<String>,
    /// Chemins déclarés par le paquet, pas une preuve de la configuration active.
    pub package_config_files: Vec<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Source {
    pub manager: String,
    pub status: String,
    pub packages: usize,
    pub note: Option<String>,
}

#[derive(Default, Debug, Clone)]
pub struct Package {
    pub manager: String,
    pub name: String,
    pub architecture: Option<String>,
    pub version: Option<String>,
    pub executables: BTreeSet<String>,
    pub configs: BTreeSet<String>,
    pub desktop_files: BTreeSet<String>,
    pub description: String,
    pub application: bool,
}

impl Package {
    fn new(manager: &str, name: &str, version: &str) -> Self {
        Self {
            manager: manager.into(),
            name: name.into(),
            version: (!version.trim().is_empty())
                .then(|| version.trim().chars().take(200).collect()),
            ..Default::default()
        }
    }
    fn file(&mut self, path: &str) {
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        if path.len() > 4096 || path.ends_with('/') {
            return;
        }
        if [
            "/usr/bin/",
            "/usr/sbin/",
            "/bin/",
            "/sbin/",
            "/usr/local/bin/",
            "/usr/libexec/",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix))
        {
            self.executables.insert(path.clone());
        }
        if path.starts_with("/etc/") && config_name(Path::new(&path)) {
            self.configs.insert(path.clone());
        }
        if path.ends_with(".desktop") && path.contains("/applications/") {
            self.desktop_files.insert(path);
        }
    }
}

#[derive(Debug, Clone)]
pub struct Query {
    pub manager: &'static str,
    pub command: &'static str,
}

// Commandes constantes, aucune interpolation de nom de paquet, de configuration
// ou de référence Exec. Les requêtes passent toutes par la confirmation native.
pub fn queries(managers: &[String], native: &BTreeSet<String>) -> Vec<Query> {
    let candidates = [
        Query {
            manager: "pacman",
            command: "LC_ALL=C pacman -Q && LC_ALL=C pacman -Ql",
        },
        Query {
            manager: "dpkg",
            command: "LC_ALL=C dpkg-query -W -f='${binary:Package}\t${Version}\t${db:Status-Status}\\n'",
        },
        Query {
            manager: "rpm",
            command: "LC_ALL=C rpm -qa --qf 'P\\t%{NAME}\\t%{VERSION}-%{RELEASE}\\n[F\\t%{FILENAMES}\\n]'",
        },
        Query {
            manager: "apk",
            command: "LC_ALL=C apk list --installed",
        },
        Query {
            manager: "flatpak",
            command: "LC_ALL=C flatpak list --app --columns=application,version,installation",
        },
        Query {
            manager: "snap",
            command: "LC_ALL=C snap list",
        },
        Query {
            manager: "nix",
            command: "nix profile list --json || nix-env --query --json",
        },
        Query {
            manager: "homebrew",
            command: "HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_ANALYTICS=1 brew info --json=v2 --installed",
        },
        Query {
            manager: "xbps",
            command: "LC_ALL=C xbps-query -l",
        },
        Query {
            manager: "pipx",
            command: "pipx list --json",
        },
        Query {
            manager: "cargo",
            command: "LC_ALL=C cargo install --list",
        },
    ];
    candidates
        .into_iter()
        .filter(|q| managers.iter().any(|m| m == q.manager) && !native.contains(q.manager))
        .collect()
}

fn bounded_text(path: &Path, limit: usize) -> anyhow::Result<String> {
    let file = std::fs::File::open(path)?;
    ensure!(file.metadata()?.is_file(), "fichier ordinaire attendu");
    let mut data = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut data)?;
    ensure!(data.len() <= limit, "métadonnées trop volumineuses");
    String::from_utf8(data).context("métadonnées UTF-8 attendues")
}

fn section<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let (_, rest) = text.split_once(&format!("%{name}%\n"))?;
    Some(rest.split("\n\n").next().unwrap_or(rest).trim())
}

fn pacman_package(desc: &str, files: &str) -> Option<Package> {
    let mut p = Package::new("pacman", section(desc, "NAME")?, section(desc, "VERSION")?);
    p.description = section(desc, "DESC").unwrap_or("").into();
    for path in section(files, "FILES").unwrap_or("").lines() {
        p.file(path);
    }
    Some(p)
}

fn paragraphs(text: &str) -> impl Iterator<Item = BTreeMap<&str, String>> {
    text.split("\n\n").map(|paragraph| {
        let mut fields = BTreeMap::new();
        let mut current = "";
        for line in paragraph.lines() {
            if line.starts_with(' ') {
                if let Some(value) = fields.get_mut(current) {
                    let value: &mut String = value;
                    value.push('\n');
                    value.push_str(line.trim());
                }
            } else if let Some((key, value)) = line.split_once(':') {
                current = key;
                fields.insert(key, value.trim().to_string());
            }
        }
        fields
    })
}

fn dpkg_packages(status: &str) -> Vec<Package> {
    paragraphs(status)
        .filter_map(|fields| {
            let status = fields.get("Status")?;
            if status.split_whitespace().last() != Some("installed")
                || status.split_whitespace().nth(1) != Some("ok")
            {
                return None;
            }
            let mut p = Package::new("dpkg", fields.get("Package")?, fields.get("Version")?);
            p.architecture = fields.get("Architecture").cloned();
            p.description = fields.get("Description").cloned().unwrap_or_default();
            for path in fields
                .get("Conffiles")
                .into_iter()
                .flat_map(|s| s.lines())
                .filter_map(|s| s.split_whitespace().next())
            {
                p.file(path);
            }
            Some(p)
        })
        .collect()
}

fn apk_packages(text: &str) -> Vec<Package> {
    text.split("\n\n")
        .filter_map(|block| {
            let mut p = Package::default();
            p.manager = "apk".into();
            let mut directory = "";
            for line in block.lines() {
                let (key, value) = line.split_once(':')?;
                match key {
                    "P" => p.name = value.into(),
                    "V" => p.version = Some(value.into()),
                    "T" => p.description = value.into(),
                    "F" => directory = value,
                    "R" => p.file(&format!("{directory}/{value}")),
                    _ => {}
                }
            }
            (!p.name.is_empty()).then_some(p)
        })
        .collect()
}

fn name_version(value: &str) -> (String, Option<String>) {
    let split = value.char_indices().find(|(i, c)| {
        *c == '-'
            && value[*i + 1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
    });
    split
        .map(|(i, _)| (value[..i].into(), Some(value[i + 1..].into())))
        .unwrap_or_else(|| (value.into(), None))
}

pub fn parse_query(manager: &str, text: &str) -> anyhow::Result<Vec<Package>> {
    let mut out = Vec::new();
    match manager {
        "rpm" => {
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("P\t") {
                    let (name, version) = rest.split_once('\t').context("en-tête RPM invalide")?;
                    out.push(Package::new(manager, name, version));
                } else if let Some(path) = line.strip_prefix("F\t") {
                    if let Some(p) = out.last_mut() {
                        p.file(path);
                    }
                }
            }
        }
        "pacman" => {
            let mut by_name = BTreeMap::new();
            for line in text.lines() {
                let Some((name, value)) = line.split_once(' ') else {
                    continue;
                };
                if value.starts_with('/') {
                    if let Some(p) = by_name.get_mut(name) {
                        let p: &mut Package = p;
                        p.file(value);
                    }
                } else {
                    by_name.insert(name.to_string(), Package::new(manager, name, value));
                }
            }
            out.extend(by_name.into_values());
        }
        "dpkg" => {
            for line in text.lines() {
                let fields: Vec<_> = line.split('\t').collect();
                if fields.len() == 3 && fields[2] == "installed" {
                    out.push(Package::new(manager, fields[0], fields[1]));
                }
            }
        }
        "flatpak" => {
            for line in text.lines() {
                let fields: Vec<_> = line.split('\t').collect();
                if fields.len() >= 2 {
                    let mut p = Package::new(manager, fields[0], fields[1]);
                    p.application = true;
                    out.push(p);
                }
            }
        }
        "snap" => {
            for line in text.lines().skip(1) {
                let fields: Vec<_> = line.split_whitespace().collect();
                if fields.len() >= 2 {
                    let mut p = Package::new(manager, fields[0], fields[1]);
                    p.application = true;
                    out.push(p);
                }
            }
        }
        "apk" | "xbps" => {
            for line in text.lines() {
                let mut words = line.split_whitespace();
                let value = if manager == "xbps" {
                    if words.next() != Some("ii") {
                        continue;
                    }
                    words.next()
                } else {
                    words.next()
                };
                if let Some(value) = value {
                    let (name, version) = name_version(value);
                    out.push(Package::new(
                        manager,
                        &name,
                        version.as_deref().unwrap_or(""),
                    ));
                }
            }
        }
        "cargo" => {
            for line in text.lines() {
                if !line.starts_with(' ') && line.ends_with(':') {
                    if let Some((name, version)) = line.trim_end_matches(':').split_once(" v") {
                        let mut p = Package::new(
                            manager,
                            name,
                            version.split_whitespace().next().unwrap_or(""),
                        );
                        p.application = true;
                        out.push(p);
                    }
                } else if let Some(p) = out.last_mut() {
                    let bin = line.trim();
                    if valid_program(bin) {
                        p.executables.insert(bin.into());
                    }
                }
            }
        }
        "homebrew" => {
            let value: Value = serde_json::from_str(text)?;
            ensure!(
                value["formulae"].is_array() && value["casks"].is_array(),
                "inventaire Homebrew JSON inconnu"
            );
            for formula in value["formulae"].as_array().into_iter().flatten() {
                for installed in formula["installed"].as_array().into_iter().flatten() {
                    if let Some(name) = formula["name"].as_str() {
                        let mut p = Package::new(
                            manager,
                            name,
                            installed["version"].as_str().unwrap_or(""),
                        );
                        p.application =
                            installed["installed_on_request"].as_bool().unwrap_or(false);
                        out.push(p);
                    }
                }
            }
            for cask in value["casks"].as_array().into_iter().flatten() {
                if let Some(name) = cask["token"].as_str() {
                    let installed = cask["installed"]
                        .as_str()
                        .or_else(|| cask["installed"].as_array()?.first()?.as_str());
                    if let Some(version) = installed {
                        let mut p = Package::new(manager, name, version);
                        p.application = true;
                        out.push(p);
                    }
                }
            }
        }
        "pipx" => {
            let value: Value = serde_json::from_str(text)?;
            ensure!(value["venvs"].is_object(), "inventaire pipx JSON inconnu");
            for venv in value["venvs"]
                .as_object()
                .into_iter()
                .flat_map(|o| o.values())
            {
                let main = &venv["metadata"]["main_package"];
                if let Some(name) = main["package"].as_str() {
                    let mut p = Package::new(
                        manager,
                        name,
                        main["package_version"].as_str().unwrap_or(""),
                    );
                    p.application = true;
                    for bin in main["apps"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        p.executables.insert(bin.into());
                    }
                    out.push(p);
                }
            }
        }
        "nix" => {
            let value: Value = serde_json::from_str(text)?;
            let elements = value.get("elements").unwrap_or(&value);
            let rows: Vec<_> = if let Some(map) = elements.as_object() {
                map.values().collect()
            } else if let Some(array) = elements.as_array() {
                array.iter().collect()
            } else {
                anyhow::bail!("profil Nix JSON inconnu")
            };
            for row in rows {
                let paths: Vec<_> = row["storePaths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .chain(
                        row["outputs"]
                            .as_object()
                            .into_iter()
                            .flat_map(|m| m.values())
                            .filter_map(Value::as_str),
                    )
                    .collect();
                let raw = row["name"].as_str().or_else(|| {
                    paths.first().and_then(|p| {
                        Path::new(p)
                            .file_name()?
                            .to_str()?
                            .split_once('-')
                            .map(|(_, v)| v)
                    })
                });
                if let Some(raw) = raw {
                    let (name, version) = name_version(raw);
                    let mut p = Package::new(manager, &name, version.as_deref().unwrap_or(""));
                    p.application = true;
                    for path in paths {
                        if path.starts_with("/nix/store/") {
                            if let Ok(entries) = std::fs::read_dir(Path::new(path).join("bin")) {
                                for entry in entries.flatten().take(128) {
                                    p.executables.insert(entry.path().display().to_string());
                                }
                            }
                        }
                    }
                    out.push(p);
                }
            }
        }
        _ => anyhow::bail!("source non prise en charge : {manager}"),
    }
    ensure!(out.len() <= MAX_PACKAGES, "trop de paquets");
    ensure!(
        text.trim().is_empty()
            || !out.is_empty()
            || matches!(manager, "homebrew" | "pipx" | "nix" | "snap"),
        "sortie de gestionnaire non reconnue"
    );
    out.retain(|p| !p.name.is_empty() && p.name.len() <= 256);
    Ok(out)
}

#[derive(Debug)]
struct Config {
    path: String,
    group: String,
    stem: String,
    programs: BTreeSet<String>,
}

fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
fn sensitive(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "token",
        "password",
        "credential",
        "secret",
        "keyring",
        "login data",
        "cookies",
    ]
    .iter()
    .any(|word| n.contains(word))
}
fn config_name(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    !sensitive(name)
        && (matches!(
            path.extension().and_then(|s| s.to_str()),
            Some(
                "conf"
                    | "ini"
                    | "toml"
                    | "json"
                    | "jsonc"
                    | "yaml"
                    | "yml"
                    | "lua"
                    | "css"
                    | "desktop"
                    | "cfg"
            )
        ) || matches!(name, "config" | "Preferences" | "Local State" | "settings"))
}
fn valid_program(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))
}

fn program_name(value: &str) -> &str {
    Path::new(value)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(value)
}
fn executable_path(value: &str) -> Option<String> {
    fn executable(path: &Path) -> bool {
        std::fs::metadata(path).ok().is_some_and(|m| {
            use std::os::unix::fs::PermissionsExt;
            m.is_file() && m.permissions().mode() & 0o111 != 0
        })
    }
    if value.starts_with('/') {
        return executable(Path::new(value)).then(|| value.to_string());
    }
    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|p| p.join(value))
                .find(|p| executable(p))
        })
        .map(|p| p.display().to_string())
}

fn programs(command: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(words) = shlex::split(command) else {
        return out;
    };
    let mut pos = 0;
    if words.first().is_some_and(|s| s == "env" || s == "exec") {
        pos += 1;
        while words
            .get(pos)
            .is_some_and(|s| s.contains('=') || s.starts_with('-'))
        {
            pos += 1;
        }
    }
    if let Some(word) = words.get(pos) {
        let name = Path::new(word)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("");
        if valid_program(name)
            || (word.starts_with('/')
                && !name.is_empty()
                && name.len() <= 128
                && !word.chars().any(char::is_control))
        {
            out.insert(if word.starts_with('/') {
                word.clone()
            } else {
                name.into()
            });
        }
    }
    if words
        .get(pos)
        .is_some_and(|s| matches!(s.as_str(), "sh" | "bash" | "zsh"))
        && words.get(pos + 1).is_some_and(|s| s == "-c")
    {
        if let Some(inner) = words.get(pos + 2) {
            let first = shlex::split(inner)
                .and_then(|v| v.into_iter().next())
                .unwrap_or_default();
            if valid_program(&first) {
                out.insert(first);
            }
        }
    }
    out
}

fn config_programs(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with("--") || line.starts_with(';') {
            continue;
        }
        if let Some((_, cmd)) = line.split_once(", exec,") {
            out.extend(programs(cmd.trim()));
            continue;
        }
        if let Some((_, value)) = line
            .split_once(".exec(")
            .or_else(|| line.strip_prefix("exec(").map(|v| ("", v)))
        {
            let value = value.trim();
            if let Some(quote) = value.chars().next().filter(|c| *c == '\'' || *c == '"') {
                if let Some(end) = value[1..].find(quote) {
                    out.extend(programs(&value[1..end + 1]));
                }
            }
            continue;
        }
        if let Some((key, value)) = line.split_once('=').or_else(|| line.split_once(':')) {
            let key = key.trim().trim_matches(['\'', '"']).trim_start_matches('$');
            if matches!(
                key.to_ascii_lowercase().as_str(),
                "exec" | "exec-once" | "command" | "terminal" | "browser" | "launcher" | "editor"
            ) {
                let value = value.trim().trim_end_matches(',').trim();
                let value = if value.len() >= 2
                    && ((value.starts_with('"') && value.ends_with('"'))
                        || (value.starts_with('\'') && value.ends_with('\'')))
                {
                    &value[1..value.len() - 1]
                } else {
                    value
                };
                out.extend(programs(value));
            }
        }
    }
    fn json_commands(value: &Value, depth: usize, out: &mut BTreeSet<String>) {
        if depth > 8 {
            return;
        }
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    if matches!(key.as_str(), "exec" | "command" | "exec-once") {
                        if let Some(cmd) = value.as_str() {
                            out.extend(programs(cmd));
                        }
                    }
                    json_commands(value, depth + 1, out);
                }
            }
            Value::Array(array) => {
                for value in array.iter().take(128) {
                    json_commands(value, depth + 1, out);
                }
            }
            _ => {}
        }
    }
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        json_commands(&value, 0, &mut out);
    }
    out
}

#[derive(Debug)]
pub struct Discovery {
    pub packages: Vec<Package>,
    pub sources: Vec<Source>,
    pub plans: Vec<Query>,
    pub limits: BTreeSet<String>,
    configs: Vec<Config>,
    desktop_programs: BTreeSet<String>,
}

impl Discovery {
    pub fn record(
        &mut self,
        manager: &str,
        status: &str,
        packages: Vec<Package>,
        note: Option<String>,
    ) {
        self.sources.retain(|s| s.manager != manager);
        self.sources.push(Source {
            manager: manager.into(),
            status: status.into(),
            packages: packages.len(),
            note,
        });
        self.packages.extend(packages);
    }

    pub fn finish(&mut self) -> Vec<Tool> {
        let mut tools = BTreeMap::new();
        let mut owned = BTreeSet::new();
        let mut by_alias: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut by_group: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut references: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut explicit_paths = BTreeMap::new();
        for config in &self.configs {
            by_alias
                .entry(config.group.clone())
                .or_default()
                .insert(config.path.clone());
            by_alias
                .entry(config.stem.clone())
                .or_default()
                .insert(config.path.clone());
            by_group
                .entry(config.group.clone())
                .or_default()
                .insert(config.path.clone());
            for program in &config.programs {
                references
                    .entry(program_name(program).to_string())
                    .or_default()
                    .insert(config.path.clone());
                if program.starts_with('/') {
                    explicit_paths.insert(program_name(program).to_string(), program.clone());
                }
            }
        }
        for program in &self.desktop_programs {
            references
                .entry(program_name(program).to_string())
                .or_default();
            if program.starts_with('/') {
                explicit_paths.insert(program_name(program).to_string(), program.clone());
            }
        }
        let desktop_names: BTreeSet<_> = self
            .desktop_programs
            .iter()
            .map(|v| program_name(v))
            .collect();
        for p in &self.packages {
            let mut aliases = BTreeSet::new();
            let base = p.name.split(':').next().unwrap_or(&p.name);
            aliases.insert(normalized(base));
            for suffix in ["-bin", "-git", "-stable"] {
                if let Some(name) = base.strip_suffix(suffix) {
                    aliases.insert(normalized(name));
                }
            }
            for executable in &p.executables {
                if let Some(name) = Path::new(executable).file_name().and_then(|s| s.to_str()) {
                    aliases.insert(normalized(name));
                }
            }
            let mut evidence = BTreeSet::new();
            let mut configs = BTreeSet::new();
            let mut launch_references = BTreeSet::new();
            if p.application || !p.desktop_files.is_empty() {
                evidence.insert("installed_desktop_application".to_string());
            }
            let desktop_description = p.description.to_ascii_lowercase();
            if !p.executables.is_empty()
                && [
                    "wayland",
                    "hyprland",
                    "window manager",
                    "status bar",
                    "clipboard",
                    "screen locker",
                    "notification daemon",
                ]
                .iter()
                .any(|word| desktop_description.contains(word))
            {
                evidence.insert("package_description".into());
            }
            for alias in aliases.iter().filter(|s| !s.is_empty()) {
                if let Some(paths) = by_alias.get(alias) {
                    configs.extend(paths.iter().cloned());
                    evidence.insert("config_path_name_match".into());
                }
                if alias.len() >= 4 {
                    for (group, paths) in by_group.range(alias.clone()..) {
                        if !group.starts_with(alias) {
                            break;
                        }
                        configs.extend(paths.iter().cloned());
                        evidence.insert("config_path_name_match".into());
                    }
                }
            }
            for executable in &p.executables {
                if let Some(name) = Path::new(executable).file_name().and_then(|s| s.to_str()) {
                    if let Some(paths) = references.get(name).filter(|paths| !paths.is_empty()) {
                        launch_references.extend(paths.iter().cloned());
                        evidence.insert("config_command_reference".into());
                    }
                    if desktop_names.contains(name) {
                        evidence.insert("desktop_launcher".into());
                    }
                }
            }
            if evidence.is_empty() {
                continue;
            }
            for bin in &p.executables {
                if let Some(name) = Path::new(bin).file_name().and_then(|s| s.to_str()) {
                    owned.insert(name.to_string());
                }
            }
            tools.insert(
                format!(
                    "{}:{}:{}:{}",
                    p.manager,
                    p.name,
                    p.architecture.as_deref().unwrap_or(""),
                    p.version.as_deref().unwrap_or("")
                ),
                Tool {
                    name: p.name.clone(),
                    manager: Some(p.manager.clone()),
                    package: Some(
                        p.architecture
                            .as_ref()
                            .map(|a| format!("{}:{a}", p.name))
                            .unwrap_or_else(|| p.name.clone()),
                    ),
                    version: p.version.clone(),
                    version_source: p.version.as_ref().map(|_| {
                        if p.manager == "nix" {
                            "nix_derivation_name".into()
                        } else {
                            format!("{}_installed_package", p.manager)
                        }
                    }),
                    executables: p.executables.iter().take(24).cloned().collect(),
                    config_files: configs.into_iter().take(24).collect(),
                    referenced_by: launch_references.into_iter().take(24).collect(),
                    package_config_files: p.configs.iter().take(24).cloned().collect(),
                    evidence: evidence.into_iter().collect(),
                },
            );
        }
        for (name, paths) in references {
            if owned.contains(&name) {
                continue;
            }
            let Some(executable) = explicit_paths
                .get(&name)
                .and_then(|path| executable_path(path))
                .or_else(|| executable_path(&name))
            else {
                self.limits.insert("Unresolved configuration/launcher commands omitted; references do not prove installation".into());
                continue;
            };
            tools.insert(
                format!("unmanaged:{name}"),
                Tool {
                    config_files: by_alias
                        .get(&normalized(
                            Path::new(&name)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or(&name),
                        ))
                        .into_iter()
                        .flatten()
                        .take(24)
                        .cloned()
                        .collect(),
                    name,
                    manager: None,
                    package: None,
                    version: None,
                    version_source: None,
                    executables: vec![executable],
                    referenced_by: paths.into_iter().take(24).collect(),
                    package_config_files: Vec::new(),
                    evidence: vec!["configuration_or_launcher_reference_only".into()],
                },
            );
        }
        let mut tools: Vec<_> = tools.into_values().collect();
        tools.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.manager.cmp(&b.manager))
        });
        if tools.len() > MAX_TOOLS {
            self.limits
                .insert(format!("desktop_tools truncated at {MAX_TOOLS}"));
            tools.truncate(MAX_TOOLS);
        }
        let mut bytes = 0;
        tools.retain(|tool| {
            let size = serde_json::to_vec(tool)
                .map(|v| v.len())
                .unwrap_or(usize::MAX);
            if size > (1024usize * 1024).saturating_sub(bytes) {
                self.limits.insert("Tool metadata capped at 1 MiB".into());
                false
            } else {
                bytes += size;
                true
            }
        });
        tools
    }
}

#[cfg(test)]
pub fn discover(
    fs: &FilesystemAccess,
    system_root: &Path,
    managers: &[String],
) -> anyhow::Result<Discovery> {
    discover_with_progress(fs, system_root, managers, |_, _, _| {})
}

pub fn discover_with_progress(
    fs: &FilesystemAccess,
    system_root: &Path,
    managers: &[String],
    mut progress: impl FnMut(&str, usize, Option<usize>),
) -> anyhow::Result<Discovery> {
    fs.require_approval()?;
    progress("packages", 0, None);
    let mut d = Discovery {
        packages: Vec::new(),
        sources: Vec::new(),
        plans: Vec::new(),
        limits: BTreeSet::new(),
        configs: Vec::new(),
        desktop_programs: BTreeSet::new(),
    };
    let mut native = BTreeSet::new();
    let pacman = system_root.join("var/lib/pacman/local");
    if pacman.is_dir() {
        let result = (|| -> anyhow::Result<Vec<Package>> {
            let mut out = Vec::new();
            let entries: Vec<_> = std::fs::read_dir(&pacman)?
                .filter(|entry| entry.as_ref().map(|e| e.path().is_dir()).unwrap_or(true))
                .take(MAX_PACKAGES + 1)
                .collect();
            let total = entries.len().min(MAX_PACKAGES);
            if entries.len() > MAX_PACKAGES {
                d.limits.insert("pacman package limit reached".into());
            }
            for (index, entry) in entries.into_iter().take(MAX_PACKAGES).enumerate() {
                fs.require_approval()?;
                progress("pacman", index, Some(total));
                let entry = entry?;
                if !entry.path().is_dir() {
                    continue;
                }
                let desc = bounded_text(&entry.path().join("desc"), 256 * 1024)?;
                let files = bounded_text(&entry.path().join("files"), 4 * 1024 * 1024)
                    .unwrap_or_else(|_| {
                        d.limits
                            .insert("Some pacman file ownership lists could not be read".into());
                        String::new()
                    });
                if let Some(p) = pacman_package(&desc, &files) {
                    out.push(p);
                }
            }
            progress("pacman", total, Some(total));
            Ok(out)
        })();
        match result {
            Ok(p) => {
                native.insert("pacman".into());
                d.record("pacman", "database_read", p, None)
            }
            Err(e) => d.record("pacman", "failed", Vec::new(), Some(e.to_string())),
        }
    }
    let dpkg = system_root.join("var/lib/dpkg/status");
    if dpkg.is_file() {
        match bounded_text(&dpkg, 32 * 1024 * 1024) {
            Ok(text) => {
                let mut packages = dpkg_packages(&text);
                if packages.len() > MAX_PACKAGES {
                    packages.truncate(MAX_PACKAGES);
                    d.limits.insert("dpkg package limit reached".into());
                }
                let info = system_root.join("var/lib/dpkg/info");
                let lists: BTreeMap<_, _> = std::fs::read_dir(&info)
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .filter(|e| e.path().extension().is_some_and(|v| v == "list"))
                    .filter_map(|e| Some((e.path().file_stem()?.to_str()?.to_string(), e.path())))
                    .collect();
                let total = packages.len();
                for (index, p) in packages.iter_mut().enumerate() {
                    fs.require_approval()?;
                    progress("dpkg", index, Some(total));
                    let architecture_key =
                        p.architecture.as_ref().map(|a| format!("{}:{a}", p.name));
                    let list = architecture_key
                        .as_ref()
                        .and_then(|key| lists.get(key))
                        .or_else(|| lists.get(&p.name))
                        .or_else(|| {
                            lists
                                .range(format!("{}:", p.name)..)
                                .next()
                                .filter(|(name, _)| name.starts_with(&format!("{}:", p.name)))
                                .map(|(_, path)| path)
                        });
                    if let Some(text) =
                        list.and_then(|path| bounded_text(path, 4 * 1024 * 1024).ok())
                    {
                        for line in text.lines() {
                            p.file(line);
                        }
                    } else {
                        d.limits
                            .insert("Some dpkg file ownership lists could not be read".into());
                    }
                }
                progress("dpkg", total, Some(total));
                native.insert("dpkg".into());
                d.record("dpkg", "database_read", packages, None)
            }
            Err(e) => d.record("dpkg", "failed", Vec::new(), Some(e.to_string())),
        }
    }
    let apk = system_root.join("lib/apk/db/installed");
    if apk.is_file() {
        match bounded_text(&apk, 32 * 1024 * 1024) {
            Ok(text) => {
                native.insert("apk".into());
                let mut packages = apk_packages(&text);
                if packages.len() > MAX_PACKAGES {
                    packages.truncate(MAX_PACKAGES);
                    d.limits.insert("apk package limit reached".into());
                }
                progress("apk", packages.len(), Some(packages.len()));
                d.record("apk", "database_read", packages, None)
            }
            Err(e) => d.record("apk", "failed", Vec::new(), Some(e.to_string())),
        }
    }
    let portage = system_root.join("var/db/pkg");
    if portage.is_dir() {
        let mut packages = Vec::new();
        'categories: for category in std::fs::read_dir(&portage)?.flatten().take(512) {
            if !category.path().is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(category.path())?.flatten().take(1024) {
                fs.require_approval()?;
                progress("portage", packages.len(), None);
                let name = entry.file_name().to_string_lossy().to_string();
                let (name, version) = name_version(&name);
                let mut p = Package::new("portage", &name, version.as_deref().unwrap_or(""));
                if let Ok(text) = bounded_text(&entry.path().join("CONTENTS"), 4 * 1024 * 1024) {
                    for line in text.lines() {
                        if let Some(path) = line
                            .strip_prefix("obj ")
                            .and_then(|l| l.split_whitespace().next())
                        {
                            p.file(path);
                        }
                    }
                }
                packages.push(p);
                if packages.len() >= MAX_PACKAGES {
                    d.limits.insert(format!(
                        "portage database capped at {MAX_PACKAGES} packages"
                    ));
                    break 'categories;
                }
            }
        }
        progress("portage", packages.len(), Some(packages.len()));
        d.record("portage", "database_read", packages, None);
    }
    fs.require_approval()?;
    let root = fs.inventory_path(fs.root())?;
    let mut queue = vec![(root.clone(), 0)];
    let mut visited = BTreeSet::new();
    let mut count = 0;
    let mut read_bytes = 0;
    progress("configurations", 0, None);
    while let Some((directory, depth)) = queue.pop() {
        if !visited.insert(directory.clone()) {
            continue;
        }
        let entries = match std::fs::read_dir(&directory) {
            Ok(v) => v,
            Err(_) => {
                d.limits
                    .insert("Unreadable config directories skipped".into());
                continue;
            }
        };
        let mut entries: Vec<_> = entries
            .take(MAX_ENTRIES.saturating_sub(count) + 1)
            .filter_map(Result::ok)
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            fs.require_approval()?;
            count += 1;
            progress("configurations", count, None);
            if count > MAX_ENTRIES {
                d.limits
                    .insert(format!("config scan truncated at {MAX_ENTRIES} entries"));
                break;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if sensitive(&name)
                || [
                    "Cache",
                    "GPUCache",
                    "Code Cache",
                    "node_modules",
                    ".git",
                    "IndexedDB",
                    "Local Storage",
                    "Session Storage",
                    "Service Worker",
                    "Extensions",
                ]
                .contains(&name.as_str())
            {
                continue;
            }
            let Ok(path) = fs.inventory_path(&entry.path()) else {
                continue;
            };
            if path.is_dir() {
                if depth < 4 {
                    queue.push((path, depth + 1));
                } else {
                    d.limits.insert("config scan depth limit: 4".into());
                }
                continue;
            }
            if !config_name(&path) {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&root)
                .unwrap_or(&entry.path())
                .to_path_buf();
            let group = normalized(
                relative
                    .components()
                    .next()
                    .map(|v| v.as_os_str().to_string_lossy().to_string())
                    .as_deref()
                    .unwrap_or(""),
            );
            let stem = normalized(path.file_stem().and_then(|s| s.to_str()).unwrap_or(""));
            let programs = if read_bytes < 4 * 1024 * 1024 {
                if let Ok(text) = fs.inventory_text(&path) {
                    read_bytes += text.len();
                    config_programs(&text)
                } else {
                    BTreeSet::new()
                }
            } else {
                d.limits.insert("config text scan capped at 4 MiB".into());
                BTreeSet::new()
            };
            d.configs.push(Config {
                path: path.display().to_string(),
                group,
                stem,
                programs,
            });
        }
        if count > MAX_ENTRIES {
            break;
        }
    }
    progress(
        "configurations",
        count.min(MAX_ENTRIES),
        Some(count.min(MAX_ENTRIES)),
    );
    let mut launcher_dirs: Vec<_> = [
        "usr/share/applications",
        "usr/local/share/applications",
        "var/lib/flatpak/exports/share/applications",
        "var/lib/snapd/desktop/applications",
    ]
    .into_iter()
    .map(|d| system_root.join(d))
    .collect();
    if system_root == Path::new("/") {
        if let Some(data_home) = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))
        {
            launcher_dirs.push(data_home.join("applications"));
        }
    }
    let mut launchers = 0;
    progress("launchers", 0, None);
    for directory in launcher_dirs {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten().take(2048) {
            fs.require_approval()?;
            launchers += 1;
            progress("launchers", launchers, None);
            if entry.path().extension().is_none_or(|s| s != "desktop") {
                continue;
            }
            let Ok(target) = entry.path().canonicalize() else {
                continue;
            };
            let root = directory.canonicalize()?;
            if !target.starts_with(root)
                && !target.starts_with(system_root.join("var/lib/flatpak/app"))
                && !target.starts_with(system_root.join("nix/store"))
            {
                d.limits
                    .insert("Launcher symlinks outside metadata roots skipped".into());
                continue;
            }
            if let Ok(text) = bounded_text(&entry.path(), 64 * 1024) {
                let mut desktop = false;
                for line in text.lines() {
                    if line.starts_with('[') {
                        desktop = line == "[Desktop Entry]";
                    } else if desktop {
                        if let Some(command) = line.strip_prefix("Exec=") {
                            d.desktop_programs.extend(programs(command));
                        }
                    }
                }
            }
        }
    }
    progress("launchers", launchers, Some(launchers));
    d.plans = queries(managers, &native);
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn fixture() -> (tempfile::TempDir, FilesystemAccess) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("config");
        std::fs::create_dir(&root).unwrap();
        let fs = FilesystemAccess::new(root, dir.path().join("backups"));
        (dir, fs)
    }

    #[test]
    fn discovers_unknown_tools_from_pacman_and_actual_configuration() {
        let (dir, fs) = fixture();
        fs.set_approved(true);
        let system = dir.path().join("system");
        write(
            &system.join("var/lib/pacman/local/nebula-ui-1/desc"),
            "%NAME%\nnebula-ui\n\n%VERSION%\n1.4.5-2\n\n%DESC%\nA widget workbench\n\n",
        );
        write(
            &system.join("var/lib/pacman/local/nebula-ui-1/files"),
            "%FILES%\nusr/bin/nebula-panel\netc/nebula-ui.conf\n\n",
        );
        let config = fs.root().join("hypr/hyprland.conf");
        write(
            &config,
            "exec-once = env MODE=desktop nebula-panel --quiet\napi_token = do-not-copy-this\n",
        );
        let mut events = Vec::new();
        let mut discovery =
            discover_with_progress(&fs, &system, &["pacman".into()], |phase, done, total| {
                events.push((phase.to_string(), done, total))
            })
            .unwrap();
        assert!(
            events
                .iter()
                .any(|(phase, done, total)| phase == "pacman" && *done == 1 && *total == Some(1))
        );
        assert!(
            events
                .iter()
                .any(|(phase, _, total)| phase == "configurations" && total.is_none())
        );
        assert!(
            events
                .iter()
                .all(|(_, done, total)| total.is_none_or(|total| *done <= total))
        );
        assert!(
            discovery.plans.is_empty(),
            "la base native évite une commande inutile"
        );
        let tools = discovery.finish();
        let tool = tools.iter().find(|t| t.name == "nebula-ui").unwrap();
        assert_eq!(tool.version.as_deref(), Some("1.4.5-2"));
        assert_eq!(
            tool.version_source.as_deref(),
            Some("pacman_installed_package")
        );
        assert!(tool.referenced_by.contains(&config.display().to_string()));
        assert_eq!(tool.package_config_files, vec!["/etc/nebula-ui.conf"]);
        assert!(tool.evidence.contains(&"config_command_reference".into()));
        assert!(
            !serde_json::to_string(&tools)
                .unwrap()
                .contains("do-not-copy-this")
        );
    }

    #[test]
    fn debian_installed_held_and_multiarch_packages_are_identified() {
        let (dir, fs) = fixture();
        fs.set_approved(true);
        let system = dir.path().join("system");
        write(
            &system.join("var/lib/dpkg/status"),
            "Package: orion-browser\nStatus: hold ok installed\nArchitecture: amd64\nVersion: 2:5.7-1\nConffiles:\n /etc/orion.conf hash\n\nPackage: removed-app\nStatus: deinstall ok config-files\nVersion: 1.0\n\n",
        );
        write(
            &system.join("var/lib/dpkg/info/orion-browser:amd64.list"),
            "/usr/bin/orion\n/usr/share/applications/orion.desktop\n",
        );
        write(
            &fs.root().join("orion-browser/settings.json"),
            "{\"theme\":\"dark\"}",
        );
        let mut discovery = discover(&fs, &system, &["dpkg".into()]).unwrap();
        let tools = discovery.finish();
        assert_eq!(discovery.sources[0].packages, 1);
        assert_eq!(tools[0].version.as_deref(), Some("2:5.7-1"));
        assert_eq!(tools[0].package.as_deref(), Some("orion-browser:amd64"));
        assert!(tools[0].executables.contains(&"/usr/bin/orion".into()));
        assert_eq!(tools[0].config_files.len(), 1);
    }

    #[test]
    fn rpm_query_keeps_versions_and_packaged_paths() {
        let packages=parse_query("rpm","P\tstellar-shell\t3.2-4.fc42\nF\t/usr/bin/stellar\nF\t/etc/stellar.conf\nF\t/usr/share/applications/stellar.desktop\n").unwrap();
        assert_eq!(packages[0].version.as_deref(), Some("3.2-4.fc42"));
        assert!(packages[0].configs.contains("/etc/stellar.conf"));
        assert!(
            packages[0]
                .desktop_files
                .contains("/usr/share/applications/stellar.desktop")
        );
        assert!(parse_query("rpm", "not an inventory").is_err());
    }

    #[test]
    fn apk_flatpak_snap_and_xbps_versions_are_not_guessed() {
        let apk =
            apk_packages("P:lunar-panel\nV:1.3-r2\nT:A Wayland panel\nF:usr/bin\nR:lunar\n\n");
        assert_eq!(apk[0].name, "lunar-panel");
        assert_eq!(apk[0].version.as_deref(), Some("1.3-r2"));
        assert!(apk[0].executables.contains("/usr/bin/lunar"));
        let flatpak = parse_query(
            "flatpak",
            "org.example.Lunar\t\tuser\norg.example.Orion\t2.1\tsystem\n",
        )
        .unwrap();
        assert_eq!(flatpak[0].version, None);
        assert_eq!(flatpak[1].version.as_deref(), Some("2.1"));
        let snap = parse_query(
            "snap",
            "Name Version Rev Tracking Publisher Notes\nlunar 2.3 40 latest/stable example -\n",
        )
        .unwrap();
        assert_eq!(snap[0].version.as_deref(), Some("2.3"));
        let xbps = parse_query(
            "xbps",
            "ii lunar-4.0_2 A desktop tool\nrc old-tool-1.0_1 Removed\n",
        )
        .unwrap();
        assert_eq!(xbps.len(), 1);
        assert_eq!(xbps[0].version.as_deref(), Some("4.0_2"));
    }

    #[test]
    fn nix_homebrew_pipx_and_cargo_use_installed_versions_only() {
        let nix = parse_query(
            "nix",
                r#"{"elements":{"lunar":{"storePaths":["/nix/store/00000000000000000000000000000000-lunar-panel-2.1"]}}}"#,
        )
        .unwrap();
        // Une dérivation réelle commence par un hash sans tirets.
        let legacy = parse_query(
            "nix",
            r#"{"legacy":{"name":"lunar-panel-2.1","outputs":{}}}"#,
        )
        .unwrap();
        assert_eq!(legacy[0].version.as_deref(), Some("2.1"));
        assert_eq!(nix.len(), 1);
        let brew=parse_query("homebrew",r#"{"formulae":[{"name":"lunar","versions":{"stable":"9.9"},"installed":[{"version":"1.2"}]}],"casks":[{"token":"orion","version":"8.0","installed":"2.3"}]}"#).unwrap();
        assert_eq!(brew[0].version.as_deref(), Some("1.2"));
        assert_eq!(brew[1].version.as_deref(), Some("2.3"));
        let pipx=parse_query("pipx",r#"{"venvs":{"lunar":{"metadata":{"main_package":{"package":"lunar","package_version":"3.1","apps":["lunar-ui"],"package_or_url":"https://private-user:private-token@example.org/pkg"}}}}}"#).unwrap();
        assert_eq!(pipx[0].version.as_deref(), Some("3.1"));
        let cargo = parse_query(
            "cargo",
            "lunar v1.8.0 (https://private-user:private-token@example.org/pkg):\n    lunar-ui\n",
        )
        .unwrap();
        assert_eq!(cargo[0].version.as_deref(), Some("1.8.0"));
        assert!(parse_query("homebrew", "{}").is_err());
        assert!(parse_query("pipx", "{}").is_err());
    }

    #[test]
    fn denied_access_and_symlinks_do_not_leak_private_configuration() {
        let (dir, fs) = fixture();
        let system = dir.path().join("system");
        assert!(
            discover(&fs, &system, &[])
                .unwrap_err()
                .to_string()
                .contains("non autorisé")
        );
        fs.set_approved(true);
        write(
            &fs.root().join("hyprpanel/chat.toml"),
            "command = secret-helper\n",
        );
        write(
            &fs.root().join("app/credentials.json"),
            "{\"command\":\"secret-helper\"}",
        );
        write(
            &dir.path().join("outside/config.conf"),
            "exec = secret-helper\n",
        );
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("outside"), fs.root().join("alias")).unwrap();
        let discovery = discover(&fs, &system, &[]).unwrap();
        assert!(discovery.configs.is_empty());
        let names = config_programs(
            "exec = env MODE=x lunar-ui --token never-keep-this\n$terminal = orion-ui\nhl.exec(\"stellar-ui --flag\")\ncommand = $(touch /tmp/never-run)\n",
        );
        assert!(
            names.contains("lunar-ui")
                && names.contains("orion-ui")
                && names.contains("stellar-ui")
        );
        assert!(
            !names
                .iter()
                .any(|v| v.contains("never-keep-this") || v.contains("touch"))
        );
    }

    #[test]
    fn manually_installed_tools_keep_unknown_versions() {
        let (dir, fs) = fixture();
        fs.set_approved(true);
        let binary = dir.path().join("custom-ui");
        write(&binary, "not executed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let config = fs.root().join("hypr/main.conf");
        write(
            &config,
            &format!("exec-once = '{}' --flag\n", binary.display()),
        );
        let mut discovery = discover(&fs, &dir.path().join("system"), &[]).unwrap();
        let tools = discovery.finish();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].version, None);
        assert_eq!(tools[0].package, None);
        assert_eq!(tools[0].executables, vec![binary.display().to_string()]);
        assert_eq!(std::fs::read_to_string(binary).unwrap(), "not executed");
    }

    #[test]
    fn appimage_launchers_with_quoted_paths_are_metadata_only() {
        let (dir, fs) = fixture();
        fs.set_approved(true);
        let system = dir.path().join("system");
        let image = dir.path().join("Lunar UI.AppImage");
        write(&image, "never execute me");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        write(
            &system.join("usr/share/applications/lunar.desktop"),
            &format!(
                "[Desktop Entry]\nName=Lunar\nExec=\"{}\" --flag %U\n",
                image.display()
            ),
        );
        let mut d = discover(&fs, &system, &[]).unwrap();
        let tools = d.finish();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "Lunar UI.AppImage");
        assert_eq!(tools[0].version, None);
        assert_eq!(std::fs::read_to_string(image).unwrap(), "never execute me");
    }
}
