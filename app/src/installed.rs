//! The Installed tab: managed and manual addons grouped by category, like the
//! in-game AddOns list, plus the add-by-link bar.

use std::collections::BTreeMap;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::{Disableable, Sizable, h_flex, v_flex};
use packmule::catalog::ReviewStatus;
use packmule::manager::{AddonStatus, AddonView, AppState, ManualAddon};
use packmule::worker::Command;

use crate::theme::*;
use crate::ui::*;
use crate::view::{PackmuleView, Page};

const OTHER: &str = "Other";
const COL_INSTALLED: f32 = 170.;
const COL_LATEST: f32 = 210.;
const COL_STATUS: f32 = 200.;
const COL_ACTIONS: f32 = 200.;

fn status_label(s: AddonStatus) -> (&'static str, u32) {
    match s {
        AddonStatus::NotInstalled => ("Not installed", MUTED),
        AddonStatus::UpToDate => ("Up to date", OK),
        AddonStatus::UpdateAvailable => ("Update available", ACCENT),
        AddonStatus::MissingFiles => ("Missing files", ERR),
        AddonStatus::Unknown => ("Not checked", MUTED),
    }
}

fn addon_page(a: &AddonView) -> String {
    match &a.record.tag_prefix {
        Some(_) => format!("https://github.com/{}/releases", a.record.repo),
        None => format!("https://github.com/{}", a.record.repo),
    }
}

/// Groups addons under their .toc category; "Other" goes last.
fn group_by_category<'a>(
    addons: &'a [AddonView],
    manual: &'a [ManualAddon],
) -> Vec<(String, Vec<&'a AddonView>, Vec<&'a ManualAddon>)> {
    let mut groups: BTreeMap<String, (Vec<&AddonView>, Vec<&ManualAddon>)> = BTreeMap::new();
    for a in addons {
        groups.entry(a.category.clone().unwrap_or_else(|| OTHER.into())).or_default().0.push(a);
    }
    for m in manual {
        groups.entry(m.category.clone().unwrap_or_else(|| OTHER.into())).or_default().1.push(m);
    }
    let mut out: Vec<_> = groups
        .into_iter()
        .map(|(name, (mut a, m))| {
            a.sort_by_key(|x| x.name.to_lowercase());
            (name, a, m)
        })
        .collect();
    out.sort_by(|a, b| match (a.0 == OTHER, b.0 == OTHER) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
    });
    out
}

fn cell(width: f32) -> Div {
    div().w(px(width)).flex_none().min_w_0().overflow_hidden()
}

impl PackmuleView {
    pub(crate) fn render_installed(&mut self, s: &AppState, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy();
        let has_client = s.active_install.is_some();
        let addbar = h_flex()
            .gap_2()
            .px_4()
            .py_3()
            .child(div().flex_1().child(Input::new(&self.url_input).small().disabled(!has_client)))
            .child(
                Button::new("add")
                    .small()
                    .primary()
                    .label("Add addon")
                    .disabled(busy || !has_client)
                    .on_click(cx.listener(|this, _, window, cx| this.add_from_url(window, cx))),
            );

        let body: AnyElement = if s.addons.is_empty() && s.unmanaged.is_empty() {
            v_flex()
                .p_8()
                .items_center()
                .gap_2()
                .child(muted("No addons yet."))
                .child(Button::new("go-browse").small().label("Find some on the Browse tab").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.page = Page::Browse;
                        cx.notify();
                    },
                )))
                .child(muted("Or paste a GitHub repository link above. It must publish packaged releases with a .zip."))
                .into_any_element()
        } else {
            let mut table = v_flex().px_4().pb_4().child(
                h_flex()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(hex(MUTED))
                    .border_b_1()
                    .border_color(hex(BORDER))
                    .child(div().flex_1().child("Addon"))
                    .child(cell(COL_INSTALLED).child("Installed"))
                    .child(cell(COL_LATEST).child("Latest"))
                    .child(cell(COL_STATUS).child("Status"))
                    .child(cell(COL_ACTIONS)),
            );
            for (category, managed, manual) in group_by_category(&s.addons, &s.unmanaged) {
                let open = !self.collapsed.contains(&category);
                let count = managed.len() + manual.len();
                let name = category.clone();
                table = table.child(
                    h_flex()
                        .id(SharedString::from(format!("group-{category}")))
                        .gap_2()
                        .mt_2()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|st| st.bg(hex(PANEL)))
                        .child(div().w(px(12.)).text_color(hex(MUTED)).child(if open { "▾" } else { "▸" }))
                        .child(div().font_weight(FontWeight::SEMIBOLD).text_color(hex(ACCENT)).child(category.clone()))
                        .child(badge(count.to_string(), MUTED))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.collapsed.remove(&name) {
                                this.collapsed.insert(name.clone());
                            }
                            cx.notify();
                        })),
                );
                if open {
                    for a in managed {
                        table = table.child(self.render_addon_row(a, busy, cx));
                    }
                    for m in manual {
                        table = table.child(render_manual_row(m));
                    }
                }
            }
            if !s.unmanaged.is_empty() {
                table = table.child(div().pt_4().child(muted(
                    "Manual addons were installed outside Packmule and are never changed here. When Packmule \
                     recognises one (it's in Browse, or its .toc links a GitHub repo with a build for this client), \
                     it takes it over automatically during the next update check.",
                )));
            }
            table.into_any_element()
        };

        v_flex()
            .size_full()
            .child(addbar)
            .child(div().id("installed-scroll").flex_1().min_h_0().overflow_y_scroll().child(body))
    }

    fn render_addon_row(&mut self, a: &AddonView, busy: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let key = a.key.clone();
        let (label, color) = status_label(a.status);
        let action = match a.status {
            AddonStatus::UpdateAvailable => "Update",
            AddonStatus::UpToDate => "Reinstall",
            AddonStatus::MissingFiles => "Repair",
            _ => "Install",
        };
        let page = addon_page(a);
        let display = match (&a.record.name, &a.record.tag_prefix) {
            (None, Some(p)) if a.name == a.record.repo => format!("{} · {}", a.record.repo, p.trim_end_matches('/')),
            _ => a.name.clone(),
        };

        let mut status =
            v_flex().gap_1().items_start().child(if a.busy { badge("Working…", INFO) } else { badge(label, color) });
        if let Some(w) = &a.record.compat_warning {
            status = status.child(small(format!("⚠ {w}"), WARN));
        }
        if let Some(e) = &a.record.error {
            status = status.child(small(e.clone(), ERR));
        }

        let confirming = self.confirm_remove.as_deref() == Some(key.as_str());
        let actions = if confirming {
            let (k1, k2) = (key.clone(), key.clone());
            h_flex()
                .gap_1()
                .child(
                    Button::new(SharedString::from(format!("confirm-{k1}")))
                        .xsmall()
                        .danger()
                        .label("Delete folders")
                        .tooltip(a.record.folders.join(", "))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.confirm_remove = None;
                            this.send(Command::RemoveAddon(k1.clone()), cx);
                        })),
                )
                .child(Button::new(SharedString::from(format!("cancel-{k2}"))).xsmall().label("Cancel").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.confirm_remove = None;
                        cx.notify();
                    }),
                ))
        } else {
            let (k1, k2) = (key.clone(), key.clone());
            let mut install = Button::new(SharedString::from(format!("install-{k1}")))
                .xsmall()
                .label(action)
                .disabled(busy)
                .on_click(cx.listener(move |this, _, _, cx| this.send(Command::InstallAddon(k1.clone()), cx)));
            if a.status != AddonStatus::UpToDate {
                install = install.primary();
            }
            h_flex().gap_1().child(install).child(
                Button::new(SharedString::from(format!("remove-{k2}")))
                    .xsmall()
                    .danger()
                    .label("Remove")
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.confirm_remove = Some(k2.clone());
                        cx.notify();
                    })),
            )
        };

        let mut name_col = v_flex().flex_1().min_w_0().gap(px(2.)).child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .id(SharedString::from(format!("link-{key}")))
                        .text_color(hex(TEXT))
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .hover(|st| st.text_color(hex(ACCENT)))
                        .child(display)
                        .on_click(move |_, _, cx| cx.open_url(&page)),
                )
                .child(badge("GitHub", MUTED))
                .when_some(a.installed_review.as_ref(), |d, r| d.child(review_badge(format!("review-{key}"), r))),
        );
        if !a.record.folders.is_empty() {
            name_col = name_col.child(muted(a.record.folders.join(", ")));
        }

        let mut latest = v_flex().child(a.record.latest_tag.clone().unwrap_or_else(|| "—".into()));
        if let Some(n) = &a.record.latest_asset_name {
            latest = latest.child(muted(n.clone()));
        }
        // An update that hasn't been reviewed, from an addon whose installed build was.
        if a.status == AddonStatus::UpdateAvailable
            && let Some(r @ (ReviewStatus::NotReviewed(_) | ReviewStatus::Flagged(_))) = &a.latest_review
        {
            latest = latest.child(review_badge(format!("latest-review-{key}"), r));
        }

        row()
            .child(name_col)
            .child(cell(COL_INSTALLED).child(a.record.installed_tag.clone().unwrap_or_else(|| "—".into())))
            .child(cell(COL_LATEST).child(latest))
            .child(cell(COL_STATUS).child(status))
            .child(cell(COL_ACTIONS).child(actions))
    }
}

fn row() -> Div {
    h_flex().items_start().px_2().py_2().border_b_1().border_color(with_alpha(BORDER, 0.6))
}

fn render_manual_row(m: &ManualAddon) -> impl IntoElement {
    let title = m.title.clone().unwrap_or_else(|| m.folder.clone());
    let details: Vec<String> =
        [(title != m.folder).then(|| m.folder.clone()), m.author.clone()].into_iter().flatten().collect();
    let mut name_col =
        v_flex().flex_1().min_w_0().gap(px(2.)).child(h_flex().gap_2().child(title).child(badge("Manual", WARN)));
    if !details.is_empty() {
        name_col = name_col.child(muted(details.join(" · ")));
    }
    let status = if m.title.is_none() && m.version.is_none() {
        small("No .toc file; the game won't load this folder", WARN)
    } else {
        h_flex().child(badge("Not tracked", MUTED))
    };
    row()
        .child(name_col)
        .child(cell(COL_INSTALLED).child(m.version.clone().unwrap_or_else(|| "—".into())))
        .child(cell(COL_LATEST).text_color(hex(MUTED)).child("—"))
        .child(cell(COL_STATUS).child(status))
        .child(cell(COL_ACTIONS))
}
