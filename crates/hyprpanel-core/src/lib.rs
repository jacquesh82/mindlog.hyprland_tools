//! hyprpanel-core — cœur réutilisable du panneau de configuration Hyprland.
//!
//! Tout ce qui parle au compositeur ou manipule ses fichiers de configuration
//! vit ici, sans dépendance réseau : le serveur MCP (`hyprpanel-server`) et la
//! future interface Slint v2 s'appuient sur ce crate.

pub mod backup;
pub mod clients;
pub mod events;
pub mod hypr_files;
pub mod ipc;
pub mod settings;
pub mod timefmt;

pub use backup::{create_backup, list_backups, restore_backup};
pub use clients::{ClientRegistrations, detect_clients, registration_snippets};
pub use events::{EventEntry, EventLog};
pub use settings::Settings;
