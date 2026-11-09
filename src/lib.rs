//! Packmule: a World of Warcraft addon manager that installs and updates
//! addons from their GitHub releases.
//!
//! This crate is the core, with no interface: the GPUI app (phase 2) drives a
//! [`worker::Worker`] and draws the [`manager::AppState`] it reports.

pub mod catalog;
pub mod game;
pub mod github;
pub mod http;
pub mod installer;
pub mod manager;
pub mod store;
pub mod toc;
pub mod version;
pub mod worker;
pub mod zipindex;

#[cfg(test)]
mod manager_tests;
#[cfg(test)]
mod testutil;

use std::path::PathBuf;

/// `~/.config/packmule`, shared with the Electron app.
pub fn config_dir() -> PathBuf {
    directories::BaseDirs::new().map(|d| d.config_dir().join("packmule")).unwrap_or_else(|| PathBuf::from(".packmule"))
}
