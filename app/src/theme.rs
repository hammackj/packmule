//! Packmule's dark palette with gold accents, matching the Electron app.

use gpui::{App, Hsla, rgb};
use gpui_component::{Theme, ThemeMode};

pub fn hex(c: u32) -> Hsla {
    rgb(c).into()
}

pub const BG: u32 = 0x15171c;
pub const PANEL: u32 = 0x1d2027;
pub const PANEL_2: u32 = 0x252932;
pub const BORDER: u32 = 0x323744;
pub const TEXT: u32 = 0xe6e2d6;
pub const MUTED: u32 = 0x8d93a3;
pub const ACCENT: u32 = 0xd4a73c;
pub const ACCENT_TEXT: u32 = 0x1a1406;
pub const OK: u32 = 0x5fb96a;
pub const WARN: u32 = 0xe0a43a;
pub const ERR: u32 = 0xe0605a;
pub const INFO: u32 = 0x5b9ee0;

pub fn apply(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |t| {
        let c = &mut t.colors;
        c.background = hex(BG);
        c.foreground = hex(TEXT);
        c.border = hex(BORDER);
        c.input = hex(BORDER);
        c.muted = hex(PANEL_2);
        c.muted_foreground = hex(MUTED);
        c.popover = hex(PANEL);
        c.popover_foreground = hex(TEXT);
        c.accent = hex(PANEL_2);
        c.accent_foreground = hex(TEXT);
        c.ring = hex(ACCENT);
        c.caret = hex(ACCENT);
        c.selection = hex(0x5a4a22);
        c.link = hex(ACCENT);
        c.link_hover = hex(0xe8c063);
        c.link_active = hex(ACCENT);

        c.primary = hex(ACCENT);
        c.primary_hover = hex(0xe0b54e);
        c.primary_active = hex(0xb98f2f);
        c.primary_foreground = hex(ACCENT_TEXT);
        c.button_primary = hex(ACCENT);
        c.button_primary_hover = hex(0xe0b54e);
        c.button_primary_active = hex(0xb98f2f);
        c.button_primary_foreground = hex(ACCENT_TEXT);

        c.secondary = hex(PANEL_2);
        c.secondary_hover = hex(0x2e3340);
        c.secondary_active = hex(0x363c4b);
        c.secondary_foreground = hex(TEXT);
        c.button = hex(PANEL_2);
        c.button_hover = hex(0x2e3340);
        c.button_active = hex(0x363c4b);
        c.button_foreground = hex(TEXT);
        c.button_secondary = hex(PANEL_2);
        c.button_secondary_hover = hex(0x2e3340);
        c.button_secondary_active = hex(0x363c4b);
        c.button_secondary_foreground = hex(TEXT);

        c.danger = hex(ERR);
        c.button_danger = hex(0x5a2a28);
        c.button_danger_hover = hex(0x6e322f);
        c.button_danger_active = hex(0x7d3a36);
        c.button_danger_foreground = hex(0xffd9d6);
        c.success = hex(OK);
        c.warning = hex(WARN);
        c.info = hex(INFO);

        c.tab_bar = hex(PANEL);
        c.tab = hex(PANEL);
        c.tab_active = hex(PANEL_2);
        c.tab_foreground = hex(MUTED);
        c.tab_active_foreground = hex(TEXT);
        c.title_bar = hex(PANEL);
        c.title_bar_border = hex(BORDER);
        c.scrollbar = hex(BG);
        c.scrollbar_thumb = hex(BORDER);
        c.scrollbar_thumb_hover = hex(0x454c5c);
        c.list_hover = hex(PANEL_2);
        c.overlay = gpui::hsla(0., 0., 0., 0.55);
    });
}
