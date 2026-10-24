use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub card_border: Color32,
    pub track: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub nav_selected: Color32,
    pub bar_inactive: Color32,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x17, 0x1b, 0x20),
    sidebar: Color32::from_rgb(0x13, 0x16, 0x1a),
    card: Color32::from_rgb(0x1f, 0x25, 0x2c),
    card_border: Color32::from_rgb(0x2a, 0x31, 0x39),
    track: Color32::from_rgb(0x2a, 0x30, 0x38),
    text: Color32::from_rgb(0xe8, 0xea, 0xed),
    muted: Color32::from_rgb(0x9a, 0xa4, 0xae),
    accent: Color32::from_rgb(0x19, 0xc3, 0xb3),
    accent_soft: Color32::from_rgb(0x1b, 0x3b, 0x3d),
    nav_selected: Color32::from_rgb(0x17, 0x38, 0x39),
    bar_inactive: Color32::from_rgb(0x4a, 0x55, 0x63),
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xf3, 0xf5, 0xf7),
    sidebar: Color32::from_rgb(0xff, 0xff, 0xff),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    card_border: Color32::from_rgb(0xe3, 0xe7, 0xeb),
    track: Color32::from_rgb(0xe9, 0xed, 0xf1),
    text: Color32::from_rgb(0x1c, 0x22, 0x28),
    muted: Color32::from_rgb(0x67, 0x72, 0x7e),
    accent: Color32::from_rgb(0x0d, 0x94, 0x88),
    accent_soft: Color32::from_rgb(0xdc, 0xf2, 0xef),
    nav_selected: Color32::from_rgb(0xdf, 0xf3, 0xf1),
    bar_inactive: Color32::from_rgb(0xc3, 0xca, 0xd3),
};

pub const ACTIVE_COLOR: Color32 = Color32::from_rgb(0x2e, 0xcc, 0x71);
pub const OPEN_COLOR: Color32 = Color32::from_rgb(0xf1, 0xc4, 0x0f);
pub const PAUSED_COLOR: Color32 = Color32::from_rgb(0x8a, 0x94, 0x9e);

/// Colours for the top apps in the hourly timeline and its legend.
pub const APP_COLORS: [Color32; 6] = [
    Color32::from_rgb(0x14, 0xb8, 0xa6),
    Color32::from_rgb(0x3b, 0x82, 0xf6),
    Color32::from_rgb(0xf9, 0x73, 0x16),
    Color32::from_rgb(0xea, 0xb3, 0x08),
    Color32::from_rgb(0x8b, 0x5c, 0xf6),
    Color32::from_rgb(0x22, 0xc5, 0x5e),
];
pub const OTHER_APP_COLOR: Color32 = Color32::from_rgb(0x64, 0x74, 0x8b);

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn radius(r: u8) -> CornerRadius {
    CornerRadius::same(r)
}

/// Segoe UI from Windows (falling back to egui's font) plus Phosphor icons,
/// and a "semibold" family for headings and values.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let fonts_dir = std::env::var_os("WINDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "C:\\Windows".into())
        .join("Fonts");

    let mut load = |key: &str, file: &str| -> bool {
        match std::fs::read(fonts_dir.join(file)) {
            Ok(bytes) => {
                fonts
                    .font_data
                    .insert(key.into(), Arc::new(FontData::from_owned(bytes)));
                true
            }
            Err(_) => false,
        }
    };
    let has_regular = load("segoe", "segoeui.ttf");
    let has_semibold = load("segoe-sb", "seguisb.ttf");

    fonts.font_data.insert(
        "phosphor".into(),
        Arc::new(egui_phosphor::Variant::Regular.font_data()),
    );

    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let mut proportional = Vec::new();
    if has_regular {
        proportional.push("segoe".to_string());
    }
    proportional.push("phosphor".to_string());
    proportional.extend(fallbacks.iter().cloned());

    let mut semibold_family = Vec::new();
    if has_semibold {
        semibold_family.push("segoe-sb".to_string());
    }
    semibold_family.extend(proportional.iter().cloned());

    fonts
        .families
        .insert(FontFamily::Proportional, proportional);
    fonts
        .families
        .insert(FontFamily::Name("semibold".into()), semibold_family);
    ctx.set_fonts(fonts);

    ctx.style_mut(|style| {
        style.text_styles.insert(TextStyle::Body, regular(14.5));
        style.text_styles.insert(TextStyle::Button, regular(14.5));
        style.text_styles.insert(TextStyle::Small, regular(12.0));
        style.text_styles.insert(TextStyle::Heading, semibold(18.0));
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
    });
}

pub fn visuals(p: &Palette, dark: bool) -> egui::Visuals {
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.extreme_bg_color = p.track;
    v.faint_bg_color = p.track;
    v.override_text_color = Some(p.text);
    v.window_stroke = Stroke::new(1.0_f32, p.card_border);
    v.window_corner_radius = radius(10);
    v.menu_corner_radius = radius(8);
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.card_border);
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = radius(8);
    }
    v.widgets.inactive.weak_bg_fill = p.track;
    v.widgets.inactive.bg_fill = p.track;
    v.widgets.hovered.weak_bg_fill = p.card_border;
    v.widgets.hovered.bg_fill = p.card_border;
    v
}
