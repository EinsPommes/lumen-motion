use eframe::egui::{self, DragValue, RichText};
use lumen_core::project::MAX_DIMENSION;
use lumen_core::{
    Channel, FrameRate, LayerId, LayerKind, LayerProps, SetProjectSettings, Time, Transform2D,
};

use crate::app::{InspectorTab, LumenApp};
use crate::theme;

pub fn show(app: &mut LumenApp, ctx: &egui::Context) {
    egui::SidePanel::right("inspector")
        .resizable(true)
        .default_width(300.0)
        .width_range(240.0..=480.0)
        .show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (InspectorTab::Properties, "Properties"),
                    (InspectorTab::Behaviours, "Behaviours"),
                    (InspectorTab::Filters, "Filters"),
                ] {
                    ui.selectable_value(&mut app.inspector_tab, tab, label);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                match (app.inspector_tab, app.selection) {
                    (InspectorTab::Properties, Some(id)) => layer_properties(app, ui, id),
                    (InspectorTab::Properties, None) => project_properties(app, ui),
                    (InspectorTab::Behaviours, _) => placeholder(ui, "No behaviours applied."),
                    (InspectorTab::Filters, _) => placeholder(ui, "No filters applied."),
                }
            });
        });
}

fn placeholder(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::TEXT_WEAK));
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).strong());
    ui.add_space(2.0);
}

/// Two drag values side by side; returns `true` if either changed.
fn xy(ui: &mut egui::Ui, x: &mut f32, y: &mut f32, speed: f32, suffix: &str) -> bool {
    ui.horizontal(|ui| {
        let a = ui
            .add(DragValue::new(x).speed(speed).prefix("X ").suffix(suffix).max_decimals(2))
            .changed();
        let b = ui
            .add(DragValue::new(y).speed(speed).prefix("Y ").suffix(suffix).max_decimals(2))
            .changed();
        a || b
    })
    .inner
}

const POSITION: &[Channel] = &[Channel::PositionX, Channel::PositionY];
const SCALE: &[Channel] = &[Channel::ScaleX, Channel::ScaleY];
const ROTATION: &[Channel] = &[Channel::Rotation];
const OPACITY: &[Channel] = &[Channel::Opacity];
const TRANSFORM: &[Channel] = &[
    Channel::PositionX,
    Channel::PositionY,
    Channel::ScaleX,
    Channel::ScaleY,
    Channel::Rotation,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyState {
    Static,
    Animated,
    Keyed,
}

fn key_state(props: &LayerProps, channels: &[Channel], time: Time) -> KeyState {
    let anim = &props.animation;
    if channels.iter().all(|ch| anim.has_key(*ch, time)) {
        KeyState::Keyed
    } else if channels.iter().any(|ch| anim.is_animated(*ch)) {
        KeyState::Animated
    } else {
        KeyState::Static
    }
}

/// Keyframe toggle shown in front of an animatable property.
fn key_button(ui: &mut egui::Ui, state: KeyState) -> bool {
    let (glyph, color, tip) = match state {
        KeyState::Keyed => ("◆", theme::ACCENT, "Remove keyframe"),
        KeyState::Animated => ("◇", theme::ACCENT, "Add keyframe"),
        KeyState::Static => ("◇", theme::TEXT_WEAK, "Add keyframe"),
    };
    ui.add(egui::Button::new(RichText::new(glyph).color(color)).frame(false))
        .on_hover_text(tip)
        .clicked()
}

fn layer_properties(app: &mut LumenApp, ui: &mut egui::Ui, id: LayerId) {
    let Some(original) = app.project.scene.layer(id).map(|l| l.props.clone()) else {
        return;
    };
    let time = app.time();
    let (eval_t, eval_opacity) = lumen_anim::evaluate(&original, time);
    let mut p = original.clone();
    let mut t = eval_t.clone();
    let mut opacity = eval_opacity;
    let mut toggles: Vec<&'static [Channel]> = Vec::new();
    let mut reset = false;

    ui.horizontal(|ui| {
        ui.label(RichText::new(p.kind.type_name()).strong().color(theme::ACCENT));
        ui.label(RichText::new(id.to_string()).small().color(theme::TEXT_WEAK));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("Project")
                .on_hover_text("Show project settings (Esc)")
                .clicked()
            {
                app.selection = None;
            }
        });
    });

    egui::Grid::new("layer_basic")
        .num_columns(3)
        .spacing([6.0, 8.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(f32::INFINITY));
            ui.end_row();

            ui.label("");
            ui.label("Visible");
            ui.checkbox(&mut p.visible, "");
            ui.end_row();

            if key_button(ui, key_state(&original, OPACITY, time)) {
                toggles.push(OPACITY);
            }
            ui.label("Opacity");
            let mut pct = opacity * 100.0;
            if ui
                .add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix(" %").max_decimals(0))
                .changed()
            {
                opacity = (pct / 100.0).clamp(0.0, 1.0);
            }
            ui.end_row();
        });

    section(ui, "Transform");
    egui::Grid::new("layer_transform")
        .num_columns(3)
        .spacing([6.0, 8.0])
        .show(ui, |ui| {
            if key_button(ui, key_state(&original, POSITION, time)) {
                toggles.push(POSITION);
            }
            ui.label("Position");
            xy(ui, &mut t.position.x, &mut t.position.y, 1.0, " px");
            ui.end_row();

            if key_button(ui, key_state(&original, SCALE, time)) {
                toggles.push(SCALE);
            }
            ui.label("Scale");
            let mut sx = t.scale.x * 100.0;
            let mut sy = t.scale.y * 100.0;
            if xy(ui, &mut sx, &mut sy, 0.5, " %") {
                t.scale.x = sx / 100.0;
                t.scale.y = sy / 100.0;
            }
            ui.end_row();

            if key_button(ui, key_state(&original, ROTATION, time)) {
                toggles.push(ROTATION);
            }
            ui.label("Rotation");
            ui.add(DragValue::new(&mut t.rotation_deg).speed(0.5).suffix("°").max_decimals(2));
            ui.end_row();

            ui.label("");
            ui.label("Anchor");
            xy(ui, &mut t.anchor.x, &mut t.anchor.y, 1.0, " px");
            ui.end_row();
        });
    if ui
        .small_button("Reset Transform")
        .on_hover_text("Clears transform keyframes and restores defaults")
        .clicked()
    {
        reset = true;
    }

    match &mut p.kind {
        LayerKind::Rectangle {
            size,
            color,
            corner_radius,
        } => {
            section(ui, "Rectangle");
            egui::Grid::new("layer_rect")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Size");
                    if xy(ui, &mut size.x, &mut size.y, 1.0, " px") {
                        size.x = size.x.max(1.0);
                        size.y = size.y.max(1.0);
                    }
                    ui.end_row();

                    ui.label("Corner Radius");
                    ui.add(
                        DragValue::new(corner_radius)
                            .speed(0.5)
                            .range(0.0..=10_000.0)
                            .suffix(" px"),
                    );
                    ui.end_row();

                    ui.label("Color");
                    ui.color_edit_button_rgba_unmultiplied(color);
                    ui.end_row();
                });
        }
        LayerKind::Image { path, size } => {
            section(ui, "Image");
            let file = path
                .file_name()
                .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            ui.label(file).on_hover_text(path.display().to_string());
            ui.label(
                RichText::new(format!("{} × {} px", size.x, size.y))
                    .small()
                    .color(theme::TEXT_WEAK),
            );
            if !path.exists() {
                ui.label(RichText::new("File not found").color(theme::ERROR));
            }
        }
        LayerKind::Group => {
            section(ui, "Group");
            let n = app.project.scene.layer(id).map_or(0, |l| l.children().len());
            ui.label(RichText::new(format!("{n} layer(s)")).color(theme::TEXT_WEAK));
        }
    }

    if reset {
        for ch in TRANSFORM {
            p.animation.clear_channel(*ch);
        }
        p.transform = Transform2D::default();
    } else {
        lumen_anim::write_transform(
            &mut p,
            (&eval_t, eval_opacity),
            (&t, opacity),
            time,
            app.record,
        );
    }
    let discrete = reset || !toggles.is_empty();
    for channels in toggles {
        lumen_anim::toggle_keys(&mut p, channels, time);
    }

    if p != original {
        if discrete {
            app.history.seal();
        }
        app.modify_layer(id, p);
        if discrete {
            app.history.seal();
        }
        ui.ctx().request_repaint();
    }
}

const RESOLUTION_PRESETS: [(&str, u32, u32); 5] = [
    ("HD 1080p", 1920, 1080),
    ("HD 720p", 1280, 720),
    ("4K UHD", 3840, 2160),
    ("Portrait", 1080, 1920),
    ("Square", 1080, 1080),
];

fn project_properties(app: &mut LumenApp, ui: &mut egui::Ui) {
    ui.label(RichText::new("Project").strong().color(theme::ACCENT));
    ui.label(RichText::new(&app.project.name).color(theme::TEXT_WEAK));
    ui.add_space(6.0);

    let mut s = app.project.settings.clone();
    egui::Grid::new("project_settings")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Width");
            ui.add(DragValue::new(&mut s.width).range(16..=MAX_DIMENSION).suffix(" px"));
            ui.end_row();

            ui.label("Height");
            ui.add(DragValue::new(&mut s.height).range(16..=MAX_DIMENSION).suffix(" px"));
            ui.end_row();

            ui.label("Frame rate");
            let current = FrameRate::PRESETS
                .iter()
                .find(|(_, r)| *r == s.frame_rate)
                .map_or_else(|| s.frame_rate.to_string(), |(l, _)| (*l).to_owned());
            egui::ComboBox::from_id_salt("frame_rate")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (label, rate) in FrameRate::PRESETS {
                        ui.selectable_value(&mut s.frame_rate, rate, label);
                    }
                });
            ui.end_row();

            ui.label("Duration");
            let mut secs = s.duration.as_secs_f64();
            let resp = ui.add(
                DragValue::new(&mut secs)
                    .range(0.1..=3600.0)
                    .speed(0.05)
                    .max_decimals(2)
                    .suffix(" s"),
            );
            if resp.changed() {
                if let Ok(t) = Time::from_secs_f64(secs, s.frame_rate) {
                    s.duration = t;
                }
            }
            ui.end_row();

            ui.label("Background");
            let mut rgb = [s.background[0], s.background[1], s.background[2]];
            if ui.color_edit_button_rgb(&mut rgb).changed() {
                s.background[..3].copy_from_slice(&rgb);
            }
            ui.end_row();
        });

    if s != app.project.settings {
        let before = app.project.settings.clone();
        app.execute(Box::new(SetProjectSettings::new(before, s)));
        ui.ctx().request_repaint();
    }

    section(ui, "Format Presets");
    ui.horizontal_wrapped(|ui| {
        for (label, w, h) in RESOLUTION_PRESETS {
            let active = app.project.settings.width == w && app.project.settings.height == h;
            if ui.selectable_label(active, label).clicked() && !active {
                let before = app.project.settings.clone();
                let mut after = before.clone();
                after.width = w;
                after.height = h;
                app.execute_discrete(Box::new(SetProjectSettings::new(before, after)));
            }
        }
    });

    ui.add_space(10.0);
    ui.label(
        RichText::new(format!(
            "{} frames · {} · {} layer(s)",
            app.project.settings.frame_count(),
            app.project.settings.frame_rate,
            app.project.scene.len()
        ))
        .small()
        .color(theme::TEXT_WEAK),
    );
}
