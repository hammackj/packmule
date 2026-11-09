//! Developer tool: runs the core against a state.json from a terminal, without
//! the interface.
//!
//!   cargo run --example cli -- [--config DIR] status
//!   cargo run --example cli -- [--config DIR] check     # network; adopts + checks, never installs
//!   cargo run --example cli -- [--config DIR] update    # installs every update: changes AddOns!
//!
//! --config defaults to ~/.config/packmule. Point it at a copy to avoid
//! touching your real state.

use std::path::PathBuf;
use std::sync::Arc;

use packmule::catalog::Catalog;
use packmule::http::UreqHttp;
use packmule::manager::{AddonStatus, AppState, Event, LogLevel, Manager};
use packmule::store::{Keyring, Store};

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut config = packmule::config_dir();
    if let Some(i) = args.iter().position(|a| a == "--config") {
        config = PathBuf::from(args.get(i + 1).cloned().unwrap_or_default());
        args.drain(i..=i + 1);
    }
    let store = Store::load(&config.join("state.json"), Box::new(Keyring));
    let mut m =
        Manager::new(store, Arc::new(UreqHttp::new()), Catalog::default(), env!("CARGO_PKG_VERSION"), |e| match e {
            Event::Log(l) => eprintln!("{} {}", if l.level == LogLevel::Error { "ERR " } else { "    " }, l.message),
            Event::Failed(msg) => eprintln!("FAILED {msg}"),
            Event::State(_) | Event::CommandDone => {}
        });

    match args.first().map(String::as_str) {
        Some("check") => {
            // Never install from here, whatever the settings say.
            let auto = m.state().settings.auto_update;
            if auto {
                anyhow::bail!("autoUpdate is on in this state.json; use a copy with it off");
            }
            m.refresh_catalog();
            m.check_updates()?;
        }
        Some("update") => {
            if args.get(1).map(String::as_str) != Some("--yes") {
                anyhow::bail!("update changes the AddOns folder; add --yes");
            }
            m.update_all()?;
        }
        Some("status") | None => {}
        Some(other) => anyhow::bail!("unknown command {other}"),
    }
    print(&m.state());
    Ok(())
}

fn print(s: &AppState) {
    if let Some(e) = &s.path_error {
        println!("path error: {e}");
    }
    if let Some(i) = &s.active_install {
        println!("{} ({}, interface {}, flavor {:?})", i.dir, i.product, i.interface, s.flavor);
    }
    for a in &s.addons {
        let status = match a.status {
            AddonStatus::UpToDate => "up to date",
            AddonStatus::UpdateAvailable => "UPDATE",
            AddonStatus::MissingFiles => "MISSING FILES",
            AddonStatus::NotInstalled => "not installed",
            AddonStatus::Unknown => "unknown",
        };
        println!(
            "  {:<28} {:<14} {:<22} -> {:<22} [{}]",
            a.name,
            status,
            a.record.installed_tag.as_deref().unwrap_or("-"),
            a.record.latest_tag.as_deref().unwrap_or("-"),
            a.category.as_deref().unwrap_or("Other"),
        );
    }
    for u in &s.unmanaged {
        println!("  {:<28} manual         {}", u.folder, u.version.as_deref().unwrap_or(""));
    }
    let available = s.catalog.iter().filter(|c| c.available).count();
    let installed = s.catalog.iter().filter(|c| c.installed).count();
    println!("catalog: {} entries, {available} for this client, {installed} installed", s.catalog.len());
}
