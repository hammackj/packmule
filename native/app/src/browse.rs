//! The Browse tab: catalog addons with a category list on the left, like the
//! in-game AddOns list.

use std::collections::{BTreeMap, HashMap};

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;
use gpui_component::{Disableable, Sizable, h_flex, v_flex};
use packmule::manager::{AppState, CatalogView};
use packmule::worker::Command;

use crate::theme::*;
use crate::ui::*;
use crate::view::PackmuleView;

pub fn flavor_label(flavor: Option<&str>) -> String {
    match flavor {
        Some("forever") => "WoW Forever".into(),
        Some("mainline") => "Retail".into(),
        Some("classic") => "Classic Era".into(),
        Some("mists") => "Mists Classic".into(),
        Some("bcc") => "Burning Crusade Classic".into(),
        Some("wrath") => "Wrath Classic".into(),
        Some("cata") => "Cataclysm Classic".into(),
        Some("titan") => "Titan Reforged".into(),
        Some(other) => other.into(),
        None => "this game client".into(),
    }
}

impl PackmuleView {
    pub(crate) fn render_browse(&mut self, s: &AppState, cx: &mut Context<Self>) -> impl IntoElement {
        let pool: Vec<&CatalogView> = s.catalog.iter().filter(|e| self.show_all || e.available).collect();
        let hidden = s.catalog.iter().filter(|e| !e.available).count();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for e in &pool {
            *counts.entry(e.entry.category.clone()).or_default() += 1;
        }

        let query = self.search_input.read(cx).value().trim().to_lowercase();
        let mut shown: Vec<&CatalogView> = pool
            .iter()
            .copied()
            .filter(|e| self.browse_category.as_ref().is_none_or(|c| *c == e.entry.category))
            .filter(|e| {
                query.is_empty()
                    || format!("{} {} {}", e.entry.name, e.entry.description, e.entry.repo)
                        .to_lowercase()
                        .contains(&query)
            })
            .collect();
        shown.sort_by_key(|e| (e.entry.category.to_lowercase(), e.entry.name.to_lowercase()));
        let names: HashMap<&str, &str> =
            s.catalog.iter().map(|e| (e.entry.id.as_str(), e.entry.name.as_str())).collect();

        let category_button = |id: SharedString, label: String, count: usize, selected: bool, value: Option<String>| {
            h_flex()
                .id(id)
                .px_3()
                .py_1p5()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |d| d.bg(hex(PANEL_2)).text_color(hex(ACCENT)))
                .hover(|st| st.bg(hex(PANEL_2)))
                .child(div().flex_1().child(label))
                .child(muted(count.to_string()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.browse_category = value.clone();
                    cx.notify();
                }))
        };

        let mut sidebar =
            v_flex().w(px(220.)).flex_none().p_2().gap(px(2.)).border_r_1().border_color(hex(BORDER)).child(
                category_button(
                    "cat-all".into(),
                    "All addons".into(),
                    pool.len(),
                    self.browse_category.is_none(),
                    None,
                ),
            );
        for (name, count) in &counts {
            let selected = self.browse_category.as_deref() == Some(name.as_str());
            sidebar = sidebar.child(category_button(
                SharedString::from(format!("cat-{name}")),
                name.clone(),
                *count,
                selected,
                Some(name.clone()),
            ));
        }

        let flavor = flavor_label(s.flavor.as_deref());
        let mut bar =
            h_flex().gap_4().child(div().w(px(320.)).child(Input::new(&self.search_input).small().cleanable(true)));
        if hidden > 0 {
            bar = bar.child(
                Checkbox::new("show-all")
                    .checked(self.show_all)
                    .label(format!("Show {hidden} without a {flavor} build"))
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.show_all = *checked;
                        cx.notify();
                    })),
            );
        }

        let cards: AnyElement = if shown.is_empty() {
            div().p_8().child(muted("No addons match.")).into_any_element()
        } else {
            let disabled = self.busy() || s.active_install.is_none();
            h_flex()
                .flex_wrap()
                .items_start()
                .gap_3()
                .children(
                    shown
                        .into_iter()
                        .map(|e| self.render_card(e, &names, disabled, cx).into_any_element())
                        .collect::<Vec<_>>(),
                )
                .into_any_element()
        };

        h_flex().size_full().items_start().child(sidebar).child(
            div().id("browse-scroll").flex_1().h_full().min_w_0().overflow_y_scroll().child(
                v_flex().p_4().gap_4().child(bar).child(cards).child(muted(
                    "Only addons that publish packaged releases on GitHub are listed. Any other addon with packaged \
                     GitHub releases can be added by pasting its link on the Installed tab.",
                )),
            ),
        )
    }

    fn render_card(
        &mut self,
        e: &CatalogView,
        names: &HashMap<&str, &str>,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = e.entry.id.clone();
        let repo_url = format!("https://github.com/{}", e.entry.repo);
        let working = e.busy || self.installing.as_deref() == Some(id.as_str());
        let action: AnyElement = if e.installed {
            badge("Installed", OK).into_any_element()
        } else if e.available {
            let id2 = id.clone();
            Button::new(SharedString::from(format!("install-{id}")))
                .xsmall()
                .primary()
                .label(if working { "Installing…" } else { "Install" })
                .loading(working)
                .disabled(disabled || working)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.installing = Some(id2.clone());
                    this.send(Command::InstallCatalog(id2.clone()), cx);
                }))
                .into_any_element()
        } else {
            let builds: Vec<String> = e.entry.flavors.iter().map(|f| flavor_label(Some(f))).collect();
            div()
                .id(SharedString::from(format!("na-{id}")))
                .child(small("Not available for this client", MUTED))
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(format!("Builds for: {}", builds.join(", ")))
                        .build(window, cx)
                })
                .into_any_element()
        };

        let mut card = v_flex()
            .w(px(268.))
            .min_h(px(150.))
            .p_3()
            .gap_2()
            .rounded_lg()
            .bg(hex(PANEL))
            .border_1()
            .border_color(hex(BORDER))
            .when(!e.available, |d| d.opacity(0.6))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .id(SharedString::from(format!("repo-{id}")))
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .cursor_pointer()
                            .hover(|st| st.text_color(hex(ACCENT)))
                            .child(e.entry.name.clone())
                            .on_click(move |_, _, cx| cx.open_url(&repo_url)),
                    )
                    .child(badge(e.entry.category.clone(), MUTED)),
            )
            .child(div().flex_1().text_color(hex(TEXT)).child(e.entry.description.clone()));
        if !e.entry.requires.is_empty() {
            let needs: Vec<&str> =
                e.entry.requires.iter().map(|r| names.get(r.as_str()).copied().unwrap_or(r)).collect();
            card = card.child(muted(format!("Needs {}", needs.join(", "))));
        }
        card.child(h_flex().gap_2().child(div().flex_1().child(muted(e.entry.repo.clone()))).child(action))
    }
}
