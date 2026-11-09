//! Runs the manager on a background thread. The interface sends commands and
//! gets events back; commands run one at a time, in order.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::manager::{Manager, SettingsUpdate};

#[derive(Debug, Clone)]
pub enum Command {
    /// Re-send the current state.
    Refresh,
    UpdateSettings(SettingsUpdate),
    AddAddon(String),
    InstallCatalog(String),
    InstallAddon(String),
    RemoveAddon(String),
    UpdateAll,
    CheckUpdates,
    Shutdown,
}

pub struct Worker {
    tx: Sender<Command>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// Starts the thread. `make` builds the manager on that thread; its event
    /// callback is how results reach the interface.
    pub fn spawn(make: impl FnOnce() -> Manager + Send + 'static, startup_checks: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        let handle = thread::Builder::new()
            .name("packmule-worker".into())
            .spawn(move || run(make(), rx, startup_checks))
            .expect("could not start the worker thread");
        Self { tx, handle: Some(handle) }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        // Don't block the window closing on a long download.
        if let Some(h) = self.handle.take()
            && h.is_finished()
        {
            let _ = h.join();
        }
    }
}

fn interval(m: &Manager) -> Option<Duration> {
    let hours = m.check_interval_hours();
    (hours > 0.0).then(|| Duration::from_secs_f64(hours * 3600.0))
}

fn run(mut m: Manager, rx: Receiver<Command>, startup_checks: bool) {
    m.changed();
    if startup_checks {
        m.refresh_catalog();
        if m.has_active_install() {
            report(&mut m, |m| m.check_updates());
        }
        m.check_app_update();
    }
    let mut last_check = Instant::now();

    loop {
        let cmd = match interval(&m) {
            Some(every) => {
                let wait = every.saturating_sub(last_check.elapsed());
                match rx.recv_timeout(wait) {
                    Ok(cmd) => cmd,
                    Err(RecvTimeoutError::Timeout) => Command::CheckUpdates,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            None => match rx.recv() {
                Ok(cmd) => cmd,
                Err(_) => return,
            },
        };
        match cmd {
            Command::Shutdown => return,
            Command::Refresh => m.changed(),
            Command::UpdateSettings(u) => report(&mut m, |m| m.update_settings(u)),
            Command::AddAddon(url) => report(&mut m, |m| m.add_addon(&url)),
            Command::InstallCatalog(id) => report(&mut m, |m| m.install_catalog(&id)),
            Command::InstallAddon(key) => report(&mut m, |m| m.install_addon(&key)),
            Command::RemoveAddon(key) => report(&mut m, |m| m.remove_addon(&key)),
            Command::UpdateAll => report(&mut m, |m| m.update_all()),
            Command::CheckUpdates => {
                last_check = Instant::now();
                if m.has_active_install() {
                    report(&mut m, |m| m.check_updates());
                }
            }
        }
    }
}

fn report(m: &mut Manager, f: impl FnOnce(&mut Manager) -> anyhow::Result<()>) {
    if let Err(e) = f(m) {
        m.emit_failure(format!("{e:#}"));
        m.changed();
    }
}
