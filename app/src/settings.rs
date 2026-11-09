//! The Settings dialog.

use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputState};
use gpui_component::{Sizable, h_flex, v_flex};
use packmule::manager::{AppState, SettingsUpdate};

use crate::theme::*;
use crate::ui::*;
use crate::view::PackmuleView;

pub struct SettingsForm {
    wow_path: Entity<InputState>,
    interval: Entity<InputState>,
    token: Entity<InputState>,
    auto_update: bool,
    clear_token: bool,
}

impl SettingsForm {
    pub fn new(state: &AppState, window: &mut Window, cx: &mut Context<PackmuleView>) -> Self {
        let s = &state.settings;
        let token_hint = if state.has_token {
            "•••••••• saved; type to replace"
        } else {
            "Raises the API limit from 60 to 5000 requests an hour"
        };
        Self {
            wow_path: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(".../drive_c/Program Files (x86)/World of Warcraft")
                    .default_value(s.wow_path.clone())
            }),
            interval: cx.new(|cx| InputState::new(window, cx).default_value(format!("{}", s.check_interval_hours))),
            token: cx.new(|cx| InputState::new(window, cx).placeholder(token_hint).masked(true)),
            auto_update: s.auto_update,
            clear_token: false,
        }
    }

    fn update(&self, cx: &App) -> SettingsUpdate {
        let token = self.token.read(cx).value().trim().to_string();
        let interval = self.interval.read(cx).value().trim().parse::<f64>().ok().filter(|h| *h >= 0.0);
        SettingsUpdate {
            wow_path: Some(self.wow_path.read(cx).value().trim().to_string()),
            active_install: None,
            github_token: if self.clear_token { Some(String::new()) } else { (!token.is_empty()).then_some(token) },
            check_interval_hours: interval,
            auto_update: Some(self.auto_update),
        }
    }
}

/// "GitHub API: 4,990 of 5,000 requests left · resets at 10:42 (in 38 min)".
fn rate_limit_line(state: &AppState) -> Div {
    let Some(r) = &state.rate_limit else { return muted("GitHub API allowance: checking…") };
    let fmt = |n: u32| {
        let s = n.to_string();
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
        out
    };
    let now = chrono::Utc::now().timestamp();
    let mins = ((r.reset - now).max(0) + 59) / 60;
    let at = chrono::DateTime::from_timestamp(r.reset, 0)
        .map(|t| t.with_timezone(&chrono::Local).format("%-I:%M %p").to_string())
        .unwrap_or_default();
    let who = if r.limit > 60 { "with your token" } else { "without a token" };
    let color = if r.remaining == 0 {
        ERR
    } else if r.remaining * 10 < r.limit {
        WARN
    } else {
        MUTED
    };
    let text = if r.remaining == r.limit {
        format!("GitHub API: all {} requests an hour available ({who})", fmt(r.limit))
    } else {
        format!(
            "GitHub API: {} of {} requests left this hour ({who}) · resets at {at} (in {mins} min)",
            fmt(r.remaining),
            fmt(r.limit)
        )
    };
    small(text, color)
}

fn field(label: &str, input: impl IntoElement) -> Div {
    v_flex().gap_1().child(div().text_sm().child(label.to_string())).child(input)
}

impl PackmuleView {
    pub(crate) fn render_settings(
        &mut self,
        state: &AppState,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let form = self.settings.as_ref().expect("settings open");
        let wow_path = form.wow_path.clone();

        let panel = v_flex()
            .id("settings-panel")
            .w(px(580.))
            .p_5()
            .gap_4()
            .rounded_lg()
            .bg(hex(PANEL))
            .border_1()
            .border_color(hex(BORDER))
            .shadow_lg()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Settings"))
            .child(
                field(
                    "World of Warcraft folder",
                    h_flex().gap_2().child(div().flex_1().child(Input::new(&form.wow_path).small())).child(
                        Button::new("pick-folder").small().label("Browse…").on_click(cx.listener(
                            move |_, _, window, cx| {
                                let picked = cx.prompt_for_paths(PathPromptOptions {
                                    files: false,
                                    directories: true,
                                    multiple: false,
                                    prompt: Some("Choose".into()),
                                });
                                let wow_path = wow_path.clone();
                                cx.spawn_in(window, async move |_, cx| {
                                    if let Ok(Ok(Some(paths))) = picked.await
                                        && let Some(p) = paths.first()
                                    {
                                        let text = p.to_string_lossy().to_string();
                                        let _ = wow_path.update_in(cx, |s, window, cx| s.set_value(text, window, cx));
                                    }
                                })
                                .detach();
                            },
                        )),
                    ),
                )
                .child(muted(
                    "The folder that contains _retail_, _classic_, _forever_ and so on. Pick the game client at \
                     the top of the main window.",
                )),
            )
            .child(field(
                "Check for updates every (hours, 0 = off)",
                div().w(px(120.)).child(Input::new(&form.interval).small()),
            ))
            .child(
                Checkbox::new("auto-update")
                    .checked(form.auto_update)
                    .label("Install updates automatically after a check")
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        if let Some(f) = &mut this.settings {
                            f.auto_update = *checked;
                        }
                        cx.notify();
                    })),
            )
            .child({
                let mut f = field("GitHub token (optional)", Input::new(&form.token).small().mask_toggle());
                if state.has_token {
                    f = f.child(
                        Checkbox::new("clear-token").checked(form.clear_token).label("Remove saved token").on_click(
                            cx.listener(|this, checked: &bool, _, cx| {
                                if let Some(f) = &mut this.settings {
                                    f.clear_token = *checked;
                                }
                                cx.notify();
                            }),
                        ),
                    );
                }
                f.child(muted("Stored in the system keyring (KWallet / GNOME Keyring) when one is available."))
                    .child(rate_limit_line(state))
            })
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("cancel-settings").small().label("Cancel").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.settings = None;
                            cx.notify();
                        },
                    )))
                    .child(Button::new("save-settings").small().primary().label("Save").on_click(cx.listener(
                        |this, _, _, cx| {
                            if let Some(f) = &this.settings {
                                let update = f.update(cx);
                                this.save_settings(update, cx);
                            }
                        },
                    ))),
            );

        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::hsla(0., 0., 0., 0.55))
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings = None;
                cx.notify();
            }))
            .child(panel)
    }
}
