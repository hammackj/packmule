//! Small shared pieces: badges, muted text, times.

use gpui::*;
use gpui_component::tooltip::Tooltip;
use packmule::catalog::{ReviewStatus, Verified};

use crate::theme::*;

pub fn with_alpha(c: u32, a: f32) -> Hsla {
    let mut h = hex(c);
    h.a = a;
    h
}

/// A small rounded label, tinted with `color`.
pub fn badge(label: impl Into<SharedString>, color: u32) -> Div {
    div()
        .flex_none()
        .px_2()
        .py(px(1.))
        .rounded(px(10.))
        .text_xs()
        .bg(with_alpha(color, 0.16))
        .text_color(hex(color))
        .child(label.into())
}

/// A badge that explains itself on hover and, with a URL, opens it on click.
fn explained(id: SharedString, b: Div, tip: String, url: Option<String>) -> AnyElement {
    let mut el = b.id(id).tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
    if let Some(url) = url {
        el = el.cursor_pointer().on_click(move |_, _, cx| cx.open_url(&url));
    }
    el.into_any_element()
}

fn report_url(report: &str) -> String {
    format!("https://github.com/hammackj/packmule-catalog/blob/main/{report}")
}

/// "Reviewed" / "Flagged" / "Not reviewed" for a build.
pub fn review_badge(id: impl Into<SharedString>, status: &ReviewStatus) -> AnyElement {
    let (b, tip, report) = match status {
        ReviewStatus::Reviewed(r) => (
            badge("Reviewed", OK),
            match &r.signoff {
                None => format!(
                    "Claude and Codex reviewed this exact build ({}) on {} for malicious code, and both passed it. Click for the report.",
                    r.tag, r.date
                ),
                Some(s) => format!(
                    "Claude and Codex reviewed this exact build ({}) for malicious code (Claude: {}, Codex: {}). {} read the report and passed it on {}: {} Click for the report.",
                    r.tag, r.claude, r.codex, s.by, s.date, s.note
                ),
            },
            Some(&r.report),
        ),
        ReviewStatus::Flagged(r) => (
            badge("Flagged", ERR),
            format!(
                "This build ({}) did not pass its code review (Claude: {}, Codex: {}). Read the report before using it.",
                r.tag, r.claude, r.codex
            ),
            Some(&r.report),
        ),
        ReviewStatus::NotReviewed(Some(r)) => (
            badge("Not reviewed", MUTED),
            format!("This build hasn't been code reviewed yet. {} was reviewed and passed.", r.tag),
            None,
        ),
        ReviewStatus::NotReviewed(None) => {
            (badge("Not reviewed", MUTED), "No release of this addon has been code reviewed yet.".into(), None)
        }
    };
    explained(id.into(), b, tip, report.map(|r| report_url(r)))
}

/// "Verified source": the repository is the author's official one.
pub fn verified_badge(id: impl Into<SharedString>, v: &Verified) -> AnyElement {
    let built = if v.ci_builds {
        "Releases are built automatically from the tagged source."
    } else {
        "Release zips are uploaded by the author, not built from the source automatically."
    };
    let mut tip = format!("This is the author's official repository (checked {}). {built}", v.date);
    if !v.note.is_empty() {
        tip = format!("{tip} {}", v.note);
    }
    explained(id.into(), badge("Verified source", INFO), tip, None)
}

pub fn muted(text: impl Into<SharedString>) -> Div {
    div().text_xs().text_color(hex(MUTED)).child(text.into())
}

pub fn small(text: impl Into<SharedString>, color: u32) -> Div {
    div().text_xs().text_color(hex(color)).child(text.into())
}

/// "3m ago", "5h ago", "2d ago", "never".
pub fn time_ago(iso: Option<&str>) -> String {
    let Some(t) = iso.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()) else { return "never".into() };
    let mins = (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_minutes();
    match mins {
        m if m < 1 => "just now".into(),
        m if m < 60 => format!("{m}m ago"),
        m if m < 48 * 60 => format!("{}h ago", (m + 30) / 60),
        m => format!("{}d ago", (m + 720) / 1440),
    }
}

/// Local clock time of an ISO timestamp, for the log.
pub fn clock(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|t| t.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
        .unwrap_or_default()
}
