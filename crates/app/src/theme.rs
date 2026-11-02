use eframe::egui::{self, Color32, Stroke};

pub const ACCENT: Color32 = Color32::from_rgb(255, 122, 48);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(150, 72, 30);
pub const ERROR: Color32 = Color32::from_rgb(240, 90, 90);
pub const RECORD: Color32 = Color32::from_rgb(232, 64, 64);

pub const BG_PANEL: Color32 = Color32::from_rgb(29, 30, 34);
pub const BG_WINDOW: Color32 = Color32::from_rgb(35, 36, 41);
pub const BG_DEEP: Color32 = Color32::from_rgb(19, 19, 22);
pub const BG_CANVAS: Color32 = Color32::from_rgb(14, 14, 16);
pub const BG_TRACK: Color32 = Color32::from_rgb(24, 25, 28);
pub const BORDER: Color32 = Color32::from_rgb(50, 52, 59);
pub const TEXT: Color32 = Color32::from_rgb(214, 216, 222);
pub const TEXT_WEAK: Color32 = Color32::from_rgb(128, 131, 140);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);

    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_WINDOW;
    v.extreme_bg_color = BG_DEEP;
    v.faint_bg_color = Color32::from_rgb(33, 34, 39);
    v.code_bg_color = BG_DEEP;
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.window_stroke = Stroke::new(1.0_f32, BORDER);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG_PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_WEAK);
    w.inactive.bg_fill = Color32::from_rgb(44, 46, 52);
    w.inactive.weak_bg_fill = Color32::from_rgb(42, 44, 50);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    w.hovered.bg_fill = Color32::from_rgb(58, 60, 68);
    w.hovered.weak_bg_fill = Color32::from_rgb(56, 58, 66);
    w.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_DIM);
    w.active.bg_fill = ACCENT_DIM;
    w.active.weak_bg_fill = ACCENT_DIM;
    w.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    w.open.weak_bg_fill = Color32::from_rgb(50, 52, 60);

    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.interact_size.y = 22.0;
    });
}
