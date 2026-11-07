//! État partagé entre le serveur HTTP, les outils MCP et le watcher d'événements.

use std::sync::Arc;
use std::time::SystemTime;

use hyprpanel_core::{EventLog, Settings};

#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<Settings>,
    pub events: Arc<EventLog>,
    pub started: SystemTime,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings: Arc::new(settings),
            events: Arc::new(EventLog::new(200)),
            started: SystemTime::now(),
        }
    }
}
