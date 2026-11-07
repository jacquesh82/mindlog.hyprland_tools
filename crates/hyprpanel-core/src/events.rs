//! File circulaire des événements du socket `.socket2.sock`.
//!
//! Hyprland pousse les événements ligne par ligne (`event>>payload\n`).
//! Un watcher tokio maintient un historique borné que les outils MCP
//! consultent (`hypr_recent_events`).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::io::AsyncBufReadExt;

use crate::ipc;
use crate::timefmt;

#[derive(Debug, Clone, serde::Serialize)]
pub struct EventEntry {
    pub timestamp: String,
    pub event: String,
}

pub struct EventLog {
    cap: usize,
    entries: Mutex<VecDeque<EventEntry>>,
}

impl EventLog {
    pub fn new(cap: usize) -> Self {
        Self { cap, entries: Mutex::new(VecDeque::with_capacity(cap)) }
    }

    pub fn push(&self, event: String) {
        let event = event.trim().to_string();
        if event.is_empty() {
            return;
        }
        // Garde-fou : un événement aberrant ne doit pas gonfler la mémoire.
        let event = if event.len() > 2048 { event[..2048].to_string() } else { event };
        let mut guard = self.entries.lock().expect("EventLog empoisonné");
        if guard.len() == self.cap {
            guard.pop_front();
        }
        guard.push_back(EventEntry { timestamp: timefmt::iso(std::time::SystemTime::now()), event });
    }

    /// Les `n` événements les plus récents, dans l'ordre chronologique.
    pub fn recent(&self, n: usize) -> Vec<EventEntry> {
        let guard = self.entries.lock().expect("EventLog empoisonné");
        let skip = guard.len().saturating_sub(n);
        guard.iter().skip(skip).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.lock().expect("EventLog empoisonné").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Boucle de surveillance : se connecte au socket d'événements, remplit le
/// journal, et se reconnecte indéfiniment (la signature d'instance change à
/// chaque relance de Hyprland).
pub async fn watch_forever(log: Arc<EventLog>) {
    loop {
        let attempt = async {
            let path = ipc::event_socket_path().map_err(std::io::Error::other)?;
            let stream = tokio::net::UnixStream::connect(&path).await?;
            let mut lines = tokio::io::BufReader::new(stream).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => log.push(line),
                    Ok(None) => {
                        // Hyprland a fermé la connexion (arrêt de session).
                        return Ok(());
                    }
                    Err(e) => return Err(e),
                }
            }
        };
        let _: std::io::Result<()> = attempt.await;
        // Instance absente ou connexion perdue : réessayer calmement.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borne_la_capacite_et_garde_l_ordre() {
        let log = EventLog::new(3);
        for i in 1..=5 {
            log.push(format!("e{i}"));
        }
        let recent: Vec<String> = log.recent(10).iter().map(|e| e.event.clone()).collect();
        assert_eq!(recent, vec!["e3", "e4", "e5"]);
        assert_eq!(log.len(), 3);
        let two: Vec<String> = log.recent(2).iter().map(|e| e.event.clone()).collect();
        assert_eq!(two, vec!["e4", "e5"]);
    }

    #[test]
    fn ignore_vides_et_tronque() {
        let log = EventLog::new(4);
        log.push("   ".into());
        log.push(format!("{}x", "a".repeat(3000)));
        assert_eq!(log.len(), 1);
        assert_eq!(log.recent(1)[0].event.len(), 2048);
    }
}
