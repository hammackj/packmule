//! Small shared pieces: badges, muted text, times.

use gpui::*;

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
