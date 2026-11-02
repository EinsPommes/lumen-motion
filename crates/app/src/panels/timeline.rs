use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke,
    StrokeKind,
};
use lumen_core::time::{format_timecode, LoopMode};
use lumen_core::{Channel, FrameRate, Interpolation, LayerId, LayerProps, Time};

use crate::app::LumenApp;
use crate::theme;

const HEADER_W: f32 = 160.0;
const RULER_H: f32 = 24.0;
const LAYER_ROW_H: f32 = 24.0;
const PROPERTY_ROW_H: f32 = 20.0;
const KEY_SIZE: f32 = 6.0;

const PROPERTY_ROWS: [(&str, &[Channel]); 4] = [
    ("Position", &[Channel::PositionX, Channel::PositionY]),
    ("Scale", &[Channel::ScaleX, Channel::ScaleY]),
    ("Rotation", &[Channel::Rotation]),
    ("Opacity", &[Channel::Opacity]),
];

/// An in-progress drag of keyframes along the timeline.
#[derive(Clone, Debug)]
pub struct KeyDrag {
    layer: LayerId,
    channels: Vec<Channel>,
    from: Time,
    from_frame: i64,
    start_props: LayerProps,
    start_x: f32,
}

struct Row {
    layer: LayerId,
    label: String,
    indent: usize,
    channels: &'static [Channel],
    is_property: bool,
    visible: bool,
    is_group: bool,
    height: f32,
}

pub fn show(app: &mut LumenApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("timeline")
        .resizable(true)
        .default_height(230.0)
        .height_range(140.0..=560.0)
        .show(ctx, |ui| {
            ui.add_space(4.0);
            transport_bar(app, ui);
            ui.separator();
            timeline_area(app, ui);
        });
}

fn transport_bar(app: &mut LumenApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let range = app.frame_range();
        if ui.button("⏮").on_hover_text("Go to start (Home)").clicked() {
            app.set_playhead(range.start());
        }
        if ui.button("◀").on_hover_text("Previous frame (←)").clicked() {
            app.step(-1);
        }
        let play = if app.transport.playing { "⏸" } else { "▶" };
        let play_btn = egui::Button::new(RichText::new(play).color(theme::ACCENT))
            .min_size(vec2(34.0, 0.0));
        if ui.add(play_btn).on_hover_text("Play / pause (Space)").clicked() {
            app.toggle_playback();
        }
        if ui.button("▶|").on_hover_text("Next frame (→)").clicked() {
            app.step(1);
        }
        if ui.button("⏭").on_hover_text("Go to end (End)").clicked() {
            app.set_playhead(range.last());
        }

        let mut looping = app.transport.loop_mode == LoopMode::Loop;
        if ui.toggle_value(&mut looping, "Loop").changed() {
            app.transport.loop_mode = if looping { LoopMode::Loop } else { LoopMode::Once };
        }

        ui.separator();
        let rec_color = if app.record { theme::RECORD } else { theme::TEXT_WEAK };
        if ui
            .add(egui::Button::new(RichText::new("● Rec").color(rec_color)).selected(app.record))
            .on_hover_text("Record: every change creates a keyframe (R)")
            .clicked()
        {
            app.record = !app.record;
        }
        let has_sel = app.selection.is_some();
        if ui
            .add_enabled(has_sel, egui::Button::new("◆◀"))
            .on_hover_text("Previous keyframe (Shift+←)")
            .clicked()
        {
            app.jump_to_key(false);
        }
        if ui
            .add_enabled(has_sel, egui::Button::new("▶◆"))
            .on_hover_text("Next keyframe (Shift+→)")
            .clicked()
        {
            app.jump_to_key(true);
        }

        ui.separator();
        let rate = app.project.settings.frame_rate;
        ui.label(
            RichText::new(format_timecode(app.playhead, rate))
                .monospace()
                .size(16.0)
                .color(theme::ACCENT),
        );
        ui.label(
            RichText::new(format!("Frame {}", app.playhead))
                .monospace()
                .color(theme::TEXT_WEAK),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!(
                    "Duration {}  ·  {}",
                    format_timecode(range.len(), rate),
                    rate
                ))
                .color(theme::TEXT_WEAK),
            );
        });
    });
}

/// Picks a major tick step (in frames) so labels are at least 72 px apart.
fn major_step(rate: FrameRate, px_per_frame: f32) -> i64 {
    let fps = i64::from(rate.nominal().max(1));
    let candidates = [1, 2, 5, 10]
        .into_iter()
        .filter(|&f| f < fps)
        .chain([1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800, 3600].map(|s| s * fps));
    let mut last = fps;
    for step in candidates {
        last = step;
        if step as f32 * px_per_frame >= 72.0 {
            return step;
        }
    }
    last
}

fn short_label(frame: i64, rate: FrameRate) -> String {
    let fps = i64::from(rate.nominal().max(1));
    let secs = frame / fps;
    let ff = frame % fps;
    if ff == 0 {
        format!("{}:{:02}", secs / 60, secs % 60)
    } else {
        format!("{}:{:02}+{:02}", secs / 60, secs % 60, ff)
    }
}

fn build_rows(app: &LumenApp) -> Vec<Row> {
    let scene = &app.project.scene;
    let mut rows = Vec::new();
    for (id, depth) in scene.display_order() {
        let Some(layer) = scene.layer(id) else {
            continue;
        };
        let props = &layer.props;
        rows.push(Row {
            layer: id,
            label: props.name.clone(),
            indent: depth,
            channels: &Channel::ALL,
            is_property: false,
            visible: props.visible,
            is_group: props.kind.is_group(),
            height: LAYER_ROW_H,
        });
        if app.selection == Some(id) {
            for (label, channels) in PROPERTY_ROWS {
                if channels.iter().any(|ch| props.animation.is_animated(*ch)) {
                    rows.push(Row {
                        layer: id,
                        label: label.to_owned(),
                        indent: depth + 1,
                        channels,
                        is_property: true,
                        visible: props.visible,
                        is_group: false,
                        height: PROPERTY_ROW_H,
                    });
                }
            }
        }
    }
    rows
}

fn diamond(center: Pos2, size: f32) -> Vec<Pos2> {
    vec![
        center + vec2(0.0, -size),
        center + vec2(size, 0.0),
        center + vec2(0.0, size),
        center + vec2(-size, 0.0),
    ]
}

fn timeline_area(app: &mut LumenApp, ui: &mut egui::Ui) {
    let rate = app.project.settings.frame_rate;
    let range = app.frame_range();
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    if rect.width() <= HEADER_W + 10.0 || rect.height() <= RULER_H {
        return;
    }
    let header = Rect::from_min_max(rect.min, pos2(rect.min.x + HEADER_W, rect.max.y));
    let lane = Rect::from_min_max(pos2(header.max.x + 8.0, rect.min.y), pos2(rect.max.x - 8.0, rect.max.y));
    let ruler = Rect::from_min_max(pos2(header.max.x, rect.min.y), pos2(rect.max.x, rect.min.y + RULER_H));
    let tracks = Rect::from_min_max(pos2(header.max.x, ruler.max.y), rect.max);

    let px_per_frame = lane.width().max(1.0) / range.len() as f32;
    let x_of = |f: i64| lane.min.x + (f - range.start()) as f32 * px_per_frame;
    let frame_at = |x: f32| range.start() + ((x - lane.min.x) / px_per_frame).round() as i64;

    let ruler_resp = ui.interact(ruler, ui.id().with("ruler"), Sense::click_and_drag());
    if ruler_resp.is_pointer_button_down_on() || ruler_resp.clicked() {
        if let Some(pos) = ruler_resp.interact_pointer_pos() {
            app.set_playhead(frame_at(pos.x));
        }
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(header, 0.0, theme::BG_PANEL);
    painter.rect_filled(ruler, 0.0, theme::BG_DEEP);
    painter.rect_filled(tracks, 0.0, theme::BG_TRACK);

    // Ticks.
    let major = major_step(rate, px_per_frame);
    let minor = (major / 5).max(1);
    let step = if minor as f32 * px_per_frame >= 6.0 { minor } else { major };
    let mut f = range.start().div_euclid(step) * step;
    let mut guard = 0;
    while f < range.end() && guard < 4000 {
        guard += 1;
        if f >= range.start() {
            let x = x_of(f);
            let is_major = f % major == 0;
            let h = if is_major { RULER_H * 0.5 } else { RULER_H * 0.22 };
            painter.line_segment(
                [pos2(x, ruler.max.y - h), pos2(x, ruler.max.y)],
                Stroke::new(1.0_f32, if is_major { theme::TEXT_WEAK } else { theme::BORDER }),
            );
            if is_major {
                painter.text(
                    pos2(x + 3.0, ruler.min.y + 2.0),
                    Align2::LEFT_TOP,
                    short_label(f, rate),
                    FontId::monospace(10.0),
                    theme::TEXT_WEAK,
                );
                painter.line_segment(
                    [pos2(x, tracks.min.y), pos2(x, tracks.max.y)],
                    Stroke::new(1.0_f32, Color32::from_white_alpha(6)),
                );
            }
        }
        f += step;
    }

    let rows = build_rows(app);
    if rows.is_empty() {
        painter.text(
            pos2(tracks.center().x, tracks.min.y + 24.0),
            Align2::CENTER_CENTER,
            "No layers yet: add a rectangle or image from the Layers panel",
            FontId::proportional(12.0),
            theme::TEXT_WEAK,
        );
    }

    let mut top = tracks.min.y + 2.0;
    for row in &rows {
        if top > rect.max.y {
            break;
        }
        let row_rect = Rect::from_min_max(pos2(rect.min.x, top), pos2(rect.max.x, top + row.height));
        top += row.height;
        let selected = app.selection == Some(row.layer);

        if ui
            .interact(row_rect, ui.id().with(("row", row.layer, row.label.as_str())), Sense::click())
            .clicked()
        {
            app.selection = Some(row.layer);
        }
        if selected {
            painter.rect_filled(row_rect, 0.0, Color32::from_white_alpha(7));
        }

        let text_color = match (row.visible, row.is_property) {
            (false, _) => theme::TEXT_WEAK,
            (true, true) => Color32::from_rgb(170, 172, 180),
            (true, false) => theme::TEXT,
        };
        painter.text(
            pos2(header.min.x + 10.0 + row.indent as f32 * 12.0, row_rect.center().y),
            Align2::LEFT_CENTER,
            &row.label,
            FontId::proportional(if row.is_property { 11.5 } else { 12.5 }),
            text_color,
        );

        if !row.is_property {
            let bar = Rect::from_min_max(
                pos2(x_of(range.start()), row_rect.min.y + 3.0),
                pos2(x_of(range.end()), row_rect.max.y - 3.0),
            );
            let fill = match (selected, row.is_group) {
                (true, _) => theme::ACCENT_DIM,
                (false, true) => Color32::from_rgb(58, 62, 78),
                (false, false) => Color32::from_rgb(70, 60, 54),
            };
            let fill = if row.visible { fill } else { fill.gamma_multiply(0.4) };
            painter.rect_filled(bar, 4.0, fill);
            if selected {
                painter.rect_stroke(bar, 4.0, Stroke::new(1.0_f32, theme::ACCENT), StrokeKind::Inside);
            }
        } else {
            painter.line_segment(
                [pos2(lane.min.x, row_rect.center().y), pos2(lane.max.x, row_rect.center().y)],
                Stroke::new(1.0_f32, theme::BORDER),
            );
        }

        keyframe_markers(app, ui, &painter, row, row_rect, &x_of);
    }

    update_key_drag(app, ui, px_per_frame);

    painter.line_segment(
        [pos2(header.max.x, rect.min.y), pos2(header.max.x, rect.max.y)],
        Stroke::new(1.0_f32, theme::BORDER),
    );

    // Playhead.
    let x = x_of(app.playhead);
    painter.line_segment([pos2(x, ruler.min.y), pos2(x, rect.max.y)], Stroke::new(1.5_f32, theme::ACCENT));
    painter.add(Shape::convex_polygon(
        vec![
            pos2(x - 6.0, ruler.min.y),
            pos2(x + 6.0, ruler.min.y),
            pos2(x + 6.0, ruler.min.y + 6.0),
            pos2(x, ruler.min.y + 12.0),
            pos2(x - 6.0, ruler.min.y + 6.0),
        ],
        theme::ACCENT,
        Stroke::NONE,
    ));
}

fn keyframe_markers(
    app: &mut LumenApp,
    ui: &egui::Ui,
    painter: &egui::Painter,
    row: &Row,
    row_rect: Rect,
    x_of: &dyn Fn(i64) -> f32,
) {
    let rate = app.project.settings.frame_rate;
    let Some(times) = app
        .project
        .scene
        .layer(row.layer)
        .map(|l| l.props.animation.key_times(row.channels))
    else {
        return;
    };
    let size = if row.is_property { KEY_SIZE - 1.0 } else { KEY_SIZE };

    for time in times {
        let frame = time.to_frame_round(rate);
        let center = pos2(x_of(frame), row_rect.center().y);
        let hit = Rect::from_center_size(center, vec2(size * 2.0 + 4.0, size * 2.0 + 4.0));
        let id = ui
            .id()
            .with(("key", row.layer, row.label.as_str(), time.num(), time.den()));
        let resp = ui.interact(hit, id, Sense::click_and_drag());

        let at_playhead = frame == app.playhead;
        let fill = if at_playhead || resp.hovered() {
            theme::ACCENT
        } else {
            Color32::from_rgb(225, 225, 230)
        };
        painter.add(Shape::convex_polygon(
            diamond(center, size),
            fill,
            Stroke::new(1.0_f32, theme::BG_DEEP),
        ));

        if resp.clicked() {
            app.selection = Some(row.layer);
            app.set_playhead(frame);
        }
        if resp.drag_started() {
            if let (Some(layer), Some(pos)) =
                (app.project.scene.layer(row.layer), resp.interact_pointer_pos())
            {
                app.selection = Some(row.layer);
                app.key_drag = Some(KeyDrag {
                    layer: row.layer,
                    channels: row.channels.to_vec(),
                    from: time,
                    from_frame: frame,
                    start_props: layer.props.clone(),
                    start_x: pos.x,
                });
            }
        }
        resp.context_menu(|ui| key_menu(app, ui, row, time));
    }
}

fn key_menu(app: &mut LumenApp, ui: &mut egui::Ui, row: &Row, time: Time) {
    let Some(props) = app.project.scene.layer(row.layer).map(|l| l.props.clone()) else {
        return;
    };
    let current = props.animation.interpolation_at(row.channels, time);
    ui.label(RichText::new("Interpolation").small().color(theme::TEXT_WEAK));
    for interp in Interpolation::ALL {
        if ui
            .radio(current == Some(interp), interp.label())
            .clicked()
        {
            let mut p = props.clone();
            p.animation.set_interpolation(row.channels, time, interp);
            app.history.seal();
            app.modify_layer(row.layer, p);
            app.history.seal();
            ui.close_menu();
        }
    }
    ui.separator();
    if ui.button("Delete Keyframe").clicked() {
        let mut p = props;
        lumen_anim::remove_keys(&mut p, row.channels, time);
        app.history.seal();
        app.modify_layer(row.layer, p);
        app.history.seal();
        ui.close_menu();
    }
}

fn update_key_drag(app: &mut LumenApp, ui: &egui::Ui, px_per_frame: f32) {
    let Some(drag) = app.key_drag.clone() else {
        return;
    };
    let (down, pos) = ui.input(|i| (i.pointer.primary_down(), i.pointer.interact_pos()));
    if !down {
        app.key_drag = None;
        return;
    }
    let Some(pos) = pos else { return };
    let rate = app.project.settings.frame_rate;
    let delta = ((pos.x - drag.start_x) / px_per_frame).round() as i64;
    let target = app.frame_range().clamp(drag.from_frame.saturating_add(delta));
    let Ok(to) = Time::from_frame(target, rate) else {
        return;
    };
    let mut props = drag.start_props.clone();
    props.animation.move_keys(&drag.channels, drag.from, to);
    app.modify_layer(drag.layer, props);
    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_step_spacing() {
        assert_eq!(major_step(FrameRate::FPS_30, 100.0), 1);
        assert_eq!(major_step(FrameRate::FPS_30, 1.0), 150);
        assert_eq!(major_step(FrameRate::FPS_30, 0.000_001), 3600 * 30);
    }

    #[test]
    fn short_labels() {
        assert_eq!(short_label(0, FrameRate::FPS_25), "0:00");
        assert_eq!(short_label(25 * 61, FrameRate::FPS_25), "1:01");
        assert_eq!(short_label(30, FrameRate::FPS_25), "0:01+05");
    }
}
