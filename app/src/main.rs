//! Packmule's window: a GPUI interface over the core's worker thread.

mod browse;
mod installed;
mod settings;
mod theme;
mod ui;
mod view;

use std::fs::{self, File};
use std::sync::Arc;

use gpui::*;
use gpui_component::Root;
use packmule::catalog::Catalog;
use packmule::http::UreqHttp;
use packmule::manager::Manager;
use packmule::store::{Keyring, Store};
use packmule::worker::Worker;

fn main() {
    let config = packmule::config_dir();
    let _ = fs::create_dir_all(&config);

    // One Packmule at a time: two would race on state.json and AddOns.
    let lock = File::create(config.join("packmule.lock")).expect("cannot create the lock file");
    // After installing an update, the old Packmule starts the new one and
    // quits: give it a moment to let go of the lock.
    let restarted = std::env::args().any(|a| a == "--restarted");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(if restarted { 10 } else { 0 });
    while lock.try_lock().is_err() {
        if std::time::Instant::now() >= deadline {
            eprintln!("Packmule is already running.");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let (tx, rx) = async_channel::unbounded();
    let state_file = config.join("state.json");
    let worker = Worker::spawn(
        move || {
            let store = Store::load(&state_file, Box::new(Keyring));
            let mut m = Manager::new(
                store,
                Arc::new(UreqHttp::new()),
                Catalog::default(),
                env!("CARGO_PKG_VERSION"),
                move |e| {
                    let _ = tx.send_blocking(e);
                },
            );
            m.set_self_update_target(packmule::selfupdate::target());
            m
        },
        true,
    );

    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
        theme::apply(cx);

        let options = WindowOptions {
            titlebar: Some(TitlebarOptions { title: Some("Packmule".into()), ..Default::default() }),
            window_bounds: Some(WindowBounds::centered(size(px(1100.), px(760.)), cx)),
            window_min_size: Some(size(px(760.), px(480.))),
            app_id: Some("packmule".into()),
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| view::PackmuleView::new(worker, rx, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .expect("could not open the window");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
    drop(lock);
}
