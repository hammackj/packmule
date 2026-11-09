//! The main window: top bar, banners, the Installed and Browse tabs, the
//! footer and log, and the Settings dialog.

use std::collections::HashSet;

use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{Disableable, Sizable, h_flex, v_flex};
use packmule::manager::{AddonStatus, AppState, Event, LogEntry, LogLevel, SettingsUpdate};
use packmule::worker::{Command, Worker};

use crate::settings::SettingsForm;
use crate::theme::*;
use crate::ui::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Installed,
    Browse,
}

pub struct PackmuleView {
    worker: Worker,
    pub(crate) state: Option<AppState>,
    pub(crate) log: Vec<LogEntry>,
    pub(crate) error: Option<String>,
    /// Commands sent and not finished yet.
    pending: usize,
    pub(crate) page: Page,
    pub(crate) collapsed: HashSet<String>,
    show_log: bool,
    pub(crate) url_input: Entity<InputState>,
    pub(crate) search_input: Entity<InputState>,
    pub(crate) browse_category: Option<String>,
    pub(crate) show_all: bool,
    pub(crate) settings: Option<SettingsForm>,
    /// Key of the addon whose Remove button is asking for confirmation.
    pub(crate) confirm_remove: Option<String>,
    /// The catalog id being installed from Browse.
    pub(crate) installing: Option<String>,
    opened_settings_once: bool,
    _subscriptions: Vec<Subscription>,
    _events: Task<()>,
}

impl PackmuleView {
    pub fn new(
        worker: Worker,
        rx: async_channel::Receiver<Event>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let url_input = cx.new(|cx| InputState::new(window, cx).placeholder("https://github.com/owner/repo"));
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("Search addons"));
        let subscriptions = vec![
            cx.subscribe_in(&url_input, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.add_from_url(window, cx);
                }
            }),
            cx.subscribe(&search_input, |_, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
                }
            }),
        ];
        let events = cx.spawn_in(window, async move |this, cx| {
            while let Ok(ev) = rx.recv().await {
                if this.update_in(cx, |view, window, cx| view.on_event(ev, window, cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            worker,
            state: None,
            log: Vec::new(),
            error: None,
            pending: 0,
            page: Page::Installed,
            collapsed: HashSet::new(),
            show_log: false,
            url_input,
            search_input,
            browse_category: None,
            show_all: false,
            settings: None,
            confirm_remove: None,
            installing: None,
            opened_settings_once: false,
            _subscriptions: subscriptions,
            _events: events,
        }
    }

    fn on_event(&mut self, ev: Event, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            Event::State(s) => {
                let needs_setup = s.settings.wow_path.is_empty();
                self.state = Some(*s);
                if !self.opened_settings_once {
                    self.opened_settings_once = true;
                    // Debug builds only: PACKMULE_DEBUG_PAGE=browse|settings|log, for screenshots.
                    let debug_page =
                        if cfg!(debug_assertions) { std::env::var("PACKMULE_DEBUG_PAGE").ok() } else { None };
                    match debug_page.as_deref() {
                        Some("browse") => self.page = Page::Browse,
                        Some("log") => self.show_log = true,
                        Some("settings") => self.open_settings(window, cx),
                        _ if needs_setup => self.open_settings(window, cx),
                        _ => {}
                    }
                }
            }
            Event::Log(entry) => {
                self.log.push(entry);
                if self.log.len() > 300 {
                    self.log.remove(0);
                }
            }
            Event::Failed(msg) => self.error = Some(msg),
            Event::CommandDone => {
                self.pending = self.pending.saturating_sub(1);
                if self.pending == 0 {
                    self.installing = None;
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn send(&mut self, cmd: Command, cx: &mut Context<Self>) {
        self.error = None;
        self.pending += 1;
        self.worker.send(cmd);
        cx.notify();
    }

    /// True while commands are queued or an update check runs.
    pub(crate) fn busy(&self) -> bool {
        self.pending > 0 || self.state.as_ref().is_some_and(|s| s.checking || s.addons.iter().any(|a| a.busy))
    }

    pub(crate) fn add_from_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.url_input.read(cx).value().trim().to_string();
        if url.is_empty() || self.busy() {
            return;
        }
        self.url_input.update(cx, |s, cx| s.set_value("", window, cx));
        self.send(Command::AddAddon(url), cx);
    }

    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(state) = &self.state {
            self.settings = Some(SettingsForm::new(state, window, cx));
            cx.notify();
        }
    }

    pub(crate) fn save_settings(&mut self, update: SettingsUpdate, cx: &mut Context<Self>) {
        self.settings = None;
        self.send(Command::UpdateSettings(update), cx);
    }

    fn render_topbar(&mut self, s: &AppState, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy();
        let needs_work = s.addons.iter().filter(|a| a.status != AddonStatus::UpToDate).count();
        let page = self.page;
        h_flex()
            .gap_3()
            .px_4()
            .py_2()
            .bg(hex(PANEL))
            .border_b_1()
            .border_color(hex(BORDER))
            .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(hex(ACCENT)).child("Packmule"))
            .child(
                TabBar::new("pages")
                    .segmented()
                    .small()
                    .selected_index(if page == Page::Installed { 0 } else { 1 })
                    .child(Tab::new().label("Installed"))
                    .child(Tab::new().label("Browse"))
                    .on_click(cx.listener(|this, ix: &usize, _, cx| {
                        this.page = if *ix == 0 { Page::Installed } else { Page::Browse };
                        cx.notify();
                    })),
            )
            .child(self.render_clients(s, cx))
            .child(div().flex_1())
            .child(
                Button::new("check")
                    .small()
                    .label(if s.checking { "Checking…" } else { "Check for updates" })
                    .loading(s.checking)
                    .disabled(busy || s.active_install.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.send(Command::CheckUpdates, cx))),
            )
            .child(
                Button::new("update-all")
                    .small()
                    .primary()
                    .label(if needs_work > 0 { format!("Update all ({needs_work})") } else { "Update all".into() })
                    .disabled(busy || needs_work == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.send(Command::UpdateAll, cx))),
            )
            .child(
                Button::new("settings")
                    .small()
                    .label("Settings")
                    .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
            )
    }

    /// One button per game client (_retail_, _forever_, ...).
    fn render_clients(&mut self, s: &AppState, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy();
        h_flex().gap_1().children(s.installs.iter().enumerate().map(|(i, inst)| {
            let active = s.active_install.as_ref().is_some_and(|a| a.dir == inst.dir);
            let dir = inst.dir.clone();
            let tip = format!(
                "{} · {} · interface {}",
                inst.dir,
                if inst.version.is_empty() { "unknown version" } else { &inst.version },
                inst.interface
            );
            let mut b = Button::new(("client", i))
                .small()
                .label(inst.dir.trim_matches('_').replace('_', " "))
                .tooltip(tip)
                .disabled(busy && !active);
            b = if active { b.primary() } else { b.ghost() };
            b.on_click(cx.listener(move |this, _, _, cx| {
                if !active {
                    this.send(
                        Command::UpdateSettings(SettingsUpdate {
                            active_install: Some(dir.clone()),
                            ..Default::default()
                        }),
                        cx,
                    );
                }
            }))
        }))
    }

    fn render_banners(&mut self, s: &AppState, cx: &mut Context<Self>) -> impl IntoElement {
        let banner = |color: u32| {
            h_flex()
                .gap_3()
                .px_4()
                .py_2()
                .bg(with_alpha(color, 0.12))
                .border_b_1()
                .border_color(with_alpha(color, 0.35))
                .text_sm()
                .text_color(hex(TEXT))
        };
        let mut out = v_flex();
        if let Some(update) = &s.app_update {
            let url = update.url.clone();
            out = out.child(
                banner(INFO)
                    .child(format!("Packmule {} is available (you have {}).", update.version, s.app_version))
                    .child(
                        Button::new("download").xsmall().label("Download").on_click(move |_, _, cx| cx.open_url(&url)),
                    ),
            );
        }
        if let Some(err) = &s.path_error {
            out = out.child(
                banner(WARN).child(err.clone()).child(
                    Button::new("open-settings")
                        .xsmall()
                        .label("Open settings")
                        .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
                ),
            );
        }
        if let Some(err) = &self.error {
            out = out.child(banner(ERR).child(div().flex_1().child(err.clone())).child(
                Button::new("dismiss").xsmall().label("Dismiss").on_click(cx.listener(|this, _, _, cx| {
                    this.error = None;
                    cx.notify();
                })),
            ));
        }
        out
    }

    fn render_footer(&mut self, s: &AppState, cx: &mut Context<Self>) -> impl IntoElement {
        let mut summary = format!("Last check {}", time_ago(s.last_check.as_deref()));
        if s.settings.auto_update {
            summary.push_str(" · auto-update on");
        }
        if s.settings.check_interval_hours > 0.0 {
            summary.push_str(&format!(" · every {}h", s.settings.check_interval_hours));
        } else {
            summary.push_str(" · automatic checks off");
        }
        let addons_dir = s
            .active_install
            .as_ref()
            .map(|i| std::path::Path::new(&s.settings.wow_path).join(&i.dir).join("Interface").join("AddOns"));
        h_flex()
            .gap_3()
            .px_4()
            .py_1p5()
            .bg(hex(PANEL))
            .border_t_1()
            .border_color(hex(BORDER))
            .child(muted(summary))
            .child(div().flex_1())
            .child(
                Button::new("open-addons")
                    .xsmall()
                    .ghost()
                    .label("Open AddOns folder")
                    .disabled(addons_dir.is_none())
                    .on_click(move |_, _, cx| {
                        if let Some(dir) = &addons_dir {
                            cx.open_with_system(dir);
                        }
                    }),
            )
            .child(
                Button::new("toggle-log")
                    .xsmall()
                    .ghost()
                    .label(if self.show_log { "Hide log".to_string() } else { format!("Log ({})", self.log.len()) })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_log = !this.show_log;
                        cx.notify();
                    })),
            )
    }

    fn render_log(&self) -> impl IntoElement {
        let lines: Vec<AnyElement> = if self.log.is_empty() {
            vec![muted("Nothing yet.").into_any_element()]
        } else {
            self.log
                .iter()
                .rev()
                .map(|l| {
                    let (mark, color) = if l.level == LogLevel::Error { ("✖ ", ERR) } else { ("", TEXT) };
                    div()
                        .text_xs()
                        .font_family("monospace")
                        .text_color(hex(color))
                        .child(format!("{}  {mark}{}", clock(&l.time), l.message))
                        .into_any_element()
                })
                .collect()
        };
        div()
            .id("log")
            .h(px(180.))
            .flex_none()
            .overflow_y_scroll()
            .px_4()
            .py_2()
            .bg(hex(0x101216))
            .border_t_1()
            .border_color(hex(BORDER))
            .children(lines)
    }
}

impl Render for PackmuleView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = v_flex().size_full().bg(hex(BG)).text_color(hex(TEXT)).text_sm();
        let Some(state) = self.state.clone() else {
            return root.items_center().justify_center().child(muted("Loading…"));
        };

        let content = match self.page {
            Page::Installed => self.render_installed(&state, cx).into_any_element(),
            Page::Browse => self.render_browse(&state, cx).into_any_element(),
        };
        let mut root = root
            .child(self.render_topbar(&state, cx))
            .child(self.render_banners(&state, cx))
            .child(div().flex_1().min_h_0().child(content))
            .child(self.render_footer(&state, cx));
        if self.show_log {
            root = root.child(self.render_log());
        }
        if self.settings.is_some() {
            root = root.child(self.render_settings(&state, window, cx));
        }
        root
    }
}
