use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Rect, RichText, Sense, Shape, Stroke, StrokeKind,
};
use lumen_core::time::{format_timecode, LoopMode};
use lumen_core::FrameRate;

use crate::app::LumenApp;
use crate::theme;

const HEADER_W: f32 = 150.0;
const RULER_H: f32 = 24.0;
const ROW_H: f32 = 26.0;

pub fn show(app: &mut LumenApp, ctx: &egui::Context) {
    egui::TopBottomPanel::bottom("timeline")
        .resizable(true)
        .default_height(210.0)
        .height_range(130.0..=520.0)
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
        let rate = app.project.settings.frame_rate;
        ui.label(
            RichText::new(format_timecode(app.playhead, rate))
                .monospace()
                .size(16.0)
                .color(theme::ACCENT),
        );
        ui.label(RichText::new(format!("Frame {}", app.playhead)).monospace().color(theme::TEXT_WEAK));

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

/// Picks a major tick step (in frames) so labels are at least `min_px` apart.
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

fn timeline_area(app: &mut LumenApp, ui: &mut egui::Ui) {
    let rate = app.project.settings.frame_rate;
    let range = app.frame_range();

    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    if rect.width() <= HEADER_W + 10.0 || rect.height() <= RULER_H {
        return;
    }
    let header = Rect::from_min_max(rect.min, pos2(rect.min.x + HEADER_W, rect.max.y));
    let lane = Rect::from_min_max(pos2(header.max.x, rect.min.y), rect.max);
    let ruler = Rect::from_min_size(lane.min, vec2(lane.width(), RULER_H));
    let tracks = Rect::from_min_max(pos2(lane.min.x, ruler.max.y), lane.max);

    let px_per_frame = (lane.width() - 8.0).max(1.0) / range.len() as f32;
    let x_of = |f: i64| lane.min.x + (f - range.start()) as f32 * px_per_frame;

    // Scrubbing on the ruler.
    let ruler_resp = ui.interact(ruler, ui.id().with("ruler"), Sense::click_and_drag());
    if ruler_resp.is_pointer_button_down_on() || ruler_resp.clicked() {
        if let Some(pos) = ruler_resp.interact_pointer_pos() {
            let f = range.start() + ((pos.x - lane.min.x) / px_per_frame).round() as i64;
            app.set_playhead(f);
        }
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(header, 0.0, theme::BG_PANEL);
    painter.rect_filled(ruler, 0.0, theme::BG_DEEP);
    painter.rect_filled(tracks, 0.0, theme::BG_TRACK);

    // Ticks.
    let major = major_step(rate, px_per_frame);
    let minor = (major / 5).max(1);
    let draw_minor = minor as f32 * px_per_frame >= 6.0;
    let step = if draw_minor { minor } else { major };
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

    // One row per layer (topmost first). Keyframes arrive in milestone 3.
    let order = app.project.scene.display_order();
    if order.is_empty() {
        painter.text(
            pos2(tracks.center().x, tracks.min.y + 24.0),
            Align2::CENTER_CENTER,
            "No layers yet: add a rectangle or image from the Layers panel",
            FontId::proportional(12.0),
            theme::TEXT_WEAK,
        );
    }
    for (row_index, (id, depth)) in order.into_iter().enumerate() {
        let top = tracks.min.y + 4.0 + row_index as f32 * ROW_H;
        if top > rect.max.y {
            break;
        }
        let Some(layer) = app.project.scene.layer(id) else {
            continue;
        };
        let selected = app.selection == Some(id);
        let visible = layer.props.visible;
        let is_group = layer.props.kind.is_group();
        let name = layer.props.name.clone();

        let row = Rect::from_min_max(pos2(rect.min.x, top), pos2(rect.max.x, top + ROW_H));
        if ui
            .interact(row, ui.id().with(("layer_row", id)), Sense::click())
            .clicked()
        {
            app.selection = Some(id);
        }
        if selected {
            painter.rect_filled(row, 0.0, Color32::from_white_alpha(8));
        }
        painter.text(
            pos2(header.min.x + 10.0 + depth as f32 * 12.0, row.center().y),
            Align2::LEFT_CENTER,
            &name,
            FontId::proportional(12.5),
            if visible { theme::TEXT } else { theme::TEXT_WEAK },
        );
        let bar = Rect::from_min_max(
            pos2(x_of(range.start()), row.min.y + 3.0),
            pos2(x_of(range.end()), row.max.y - 3.0),
        );
        let fill = match (selected, is_group) {
            (true, _) => theme::ACCENT_DIM,
            (false, true) => Color32::from_rgb(58, 62, 78),
            (false, false) => Color32::from_rgb(70, 60, 54),
        };
        let fill = if visible { fill } else { fill.gamma_multiply(0.4) };
        painter.rect_filled(bar, 4.0, fill);
        if selected {
            painter.rect_stroke(bar, 4.0, Stroke::new(1.0_f32, theme::ACCENT), StrokeKind::Inside);
        }
    }
    painter.line_segment(
        [pos2(header.max.x, rect.min.y), pos2(header.max.x, rect.max.y)],
        Stroke::new(1.0_f32, theme::BORDER),
    );

    // Playhead.
    let x = x_of(app.playhead);
    painter.line_segment(
        [pos2(x, ruler.min.y), pos2(x, lane.max.y)],
        Stroke::new(1.5_f32, theme::ACCENT),
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_step_spacing() {
        // Very zoomed in: single frames.
        assert_eq!(major_step(FrameRate::FPS_30, 100.0), 1);
        // 1 px/frame at 30 fps: 72 px needs ≥ 72 frames → 5 s = 150 frames? 2 s = 60 < 72.
        assert_eq!(major_step(FrameRate::FPS_30, 1.0), 150);
        // Extremely zoomed out falls back to the largest step.
        assert_eq!(major_step(FrameRate::FPS_30, 0.000_001), 3600 * 30);
    }

    #[test]
    fn short_labels() {
        assert_eq!(short_label(0, FrameRate::FPS_25), "0:00");
        assert_eq!(short_label(25 * 61, FrameRate::FPS_25), "1:01");
        assert_eq!(short_label(30, FrameRate::FPS_25), "0:01+05");
    }
}
