use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind,
};
use lumen_core::glam::Vec2;
use lumen_anim::AtTime;
use lumen_core::LayerId;

use crate::app::LumenApp;
use crate::gizmo::{self, DragMode, GizmoDrag, View};
use crate::theme;

fn v(p: Pos2) -> Vec2 {
    Vec2::new(p.x, p.y)
}

fn p(v: Vec2) -> Pos2 {
    pos2(v.x, v.y)
}

pub fn show(app: &mut LumenApp, ctx: &egui::Context) {
    let frame = egui::Frame::default()
        .fill(theme::BG_CANVAS)
        .inner_margin(egui::Margin::same(16));

    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        let avail = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(avail, Sense::click_and_drag());

        let s = &app.project.settings;
        let (cw, ch) = (s.width, s.height);
        let info_h = 18.0;
        let area = Rect::from_min_max(pos2(avail.min.x, avail.min.y + info_h), avail.max);
        let aspect = s.aspect();
        let mut size = area.size();
        if size.x / size.y.max(1.0) > aspect {
            size.x = size.y * aspect;
        } else {
            size.y = size.x / aspect;
        }
        let rect = Rect::from_center_size(area.center(), size);
        let view = View {
            center: v(rect.center()),
            scale: rect.width() / cw.max(1) as f32,
        };

        handle_input(app, ui, &resp, view);

        let painter = ui.painter_at(avail);
        match app.canvas_texture() {
            Some(id) => {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(id, rect, uv, Color32::WHITE);
            }
            None => {
                painter.rect_filled(rect, 0.0, Color32::BLACK);
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "GPU preview unavailable",
                    FontId::proportional(14.0),
                    theme::TEXT_WEAK,
                );
            }
        }
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, theme::BORDER), StrokeKind::Outside);

        if app.view.safe_areas {
            let guide = Stroke::new(1.0_f32, Color32::from_white_alpha(70));
            for frac in [0.93, 0.9] {
                let r = Rect::from_center_size(rect.center(), rect.size() * frac);
                painter.rect_stroke(r, 0.0, guide, StrokeKind::Inside);
            }
            let c = rect.center();
            painter.line_segment([c - vec2(8.0, 0.0), c + vec2(8.0, 0.0)], guide);
            painter.line_segment([c - vec2(0.0, 8.0), c + vec2(0.0, 8.0)], guide);
        }

        if let Some(id) = app.selection {
            draw_gizmo(app, &painter, id, view);
        }

        let zoom = rect.width() * ctx.pixels_per_point() / cw.max(1) as f32 * 100.0;
        painter.text(
            avail.left_top(),
            Align2::LEFT_TOP,
            format!("{cw} × {ch}  ·  {zoom:.0} %"),
            FontId::proportional(12.0),
            theme::TEXT_WEAK,
        );
    });
}

/// Handle under `screen` for the selected layer, if it has content.
fn handle_at(app: &LumenApp, id: LayerId, screen: Vec2, view: View) -> Option<DragMode> {
    let scene = &app.project.scene;
    let layer = scene.layer(id)?;
    let size = layer.props.kind.content_size()?;
    let world = scene.world_transform_with(id, &AtTime(app.time()))?;
    let shape = gizmo::shape(world, size, layer.props.transform.anchor, view);
    gizmo::pick_handle(&shape, size, screen)
}

fn begin_drag(app: &mut LumenApp, id: LayerId, mode: DragMode, pointer: Vec2) {
    let time = app.time();
    let scene = &app.project.scene;
    let Some(layer) = scene.layer(id) else { return };
    let (start_transform, start_opacity) = lumen_anim::evaluate(&layer.props, time);
    app.gizmo_drag = Some(GizmoDrag {
        id,
        mode,
        start_props: layer.props.clone(),
        start_transform,
        start_opacity,
        start_pointer: pointer,
        parent_world: scene.parent_world_with(id, &AtTime(time)),
    });
}

fn press(app: &mut LumenApp, screen: Vec2, view: View, start_drag: bool) {
    let canvas_pt = view.to_canvas(screen);
    if let Some(sel) = app.selection {
        if let Some(mode) = handle_at(app, sel, screen, view) {
            if start_drag {
                begin_drag(app, sel, mode, canvas_pt);
            }
            return;
        }
    }
    let scene = &app.project.scene;
    match scene.hit_test_with(canvas_pt, &AtTime(app.time())) {
        Some(hit) => {
            // Keep a selected group selected when clicking one of its children.
            let target = match app.selection {
                Some(sel) if scene.is_ancestor(sel, hit) => sel,
                _ => hit,
            };
            app.selection = Some(target);
            if start_drag {
                begin_drag(app, target, DragMode::Move, canvas_pt);
            }
        }
        None => app.selection = None,
    }
}

fn handle_input(app: &mut LumenApp, ui: &egui::Ui, resp: &egui::Response, view: View) {
    let (press_origin, shift) = ui.input(|i| (i.pointer.press_origin(), i.modifiers.shift));

    if resp.drag_started() {
        if let Some(origin) = press_origin {
            press(app, v(origin), view, true);
        }
    } else if resp.clicked() {
        if let Some(origin) = press_origin {
            press(app, v(origin), view, false);
        }
    }

    if resp.dragged() {
        if let (Some(drag), Some(pos)) = (app.gizmo_drag.clone(), resp.interact_pointer_pos()) {
            let transform = gizmo::apply_drag(&drag, view.to_canvas(v(pos)), shift);
            let mut props = drag.start_props.clone();
            lumen_anim::write_transform(
                &mut props,
                (&drag.start_transform, drag.start_opacity),
                (&transform, drag.start_opacity),
                app.time(),
                app.record,
            );
            app.modify_layer(drag.id, props);
        }
    }
    if resp.drag_stopped() {
        app.gizmo_drag = None;
    }

    // Cursor feedback.
    let cursor = match (&app.gizmo_drag, resp.hover_pos()) {
        (Some(d), _) => Some(match d.mode {
            DragMode::Move => CursorIcon::Grabbing,
            DragMode::Scale { .. } => CursorIcon::Crosshair,
            DragMode::Rotate => CursorIcon::Alias,
        }),
        (None, Some(pos)) => {
            let handle = app
                .selection
                .and_then(|sel| handle_at(app, sel, v(pos), view));
            match handle {
                Some(DragMode::Rotate) => Some(CursorIcon::Alias),
                Some(_) => Some(CursorIcon::Crosshair),
                None if app
                    .project
                    .scene
                    .hit_test_with(view.to_canvas(v(pos)), &AtTime(app.time()))
                    .is_some() =>
                {
                    Some(CursorIcon::Grab)
                }
                None => None,
            }
        }
        (None, None) => None,
    };
    if let Some(c) = cursor {
        ui.ctx().set_cursor_icon(c);
    }
}

fn draw_gizmo(app: &LumenApp, painter: &egui::Painter, id: LayerId, view: View) {
    let scene = &app.project.scene;
    let (Some(layer), Some(world)) = (
        scene.layer(id),
        scene.world_transform_with(id, &AtTime(app.time())),
    ) else {
        return;
    };
    let accent = Stroke::new(1.5_f32, theme::ACCENT);
    let anchor = layer.props.transform.anchor;

    let Some(size) = layer.props.kind.content_size() else {
        // Groups: show only the pivot.
        let pivot = p(view.to_screen(world.transform_point2(anchor)));
        painter.circle_stroke(pivot, 6.0, accent);
        painter.line_segment([pivot - vec2(10.0, 0.0), pivot + vec2(10.0, 0.0)], accent);
        painter.line_segment([pivot - vec2(0.0, 10.0), pivot + vec2(0.0, 10.0)], accent);
        return;
    };

    let shape = gizmo::shape(world, size, anchor, view);
    let corners: Vec<Pos2> = shape.corners.iter().map(|c| p(*c)).collect();
    painter.add(Shape::closed_line(corners.clone(), accent));

    // Rotate handle.
    painter.line_segment([p(shape.top_mid), p(shape.rotate_handle)], accent);
    painter.circle(p(shape.rotate_handle), 5.0, theme::BG_DEEP, accent);

    // Corner handles.
    for c in corners {
        let r = Rect::from_center_size(c, vec2(9.0, 9.0));
        painter.rect_filled(r, 1.0, Color32::WHITE);
        painter.rect_stroke(r, 1.0, Stroke::new(1.0_f32, theme::ACCENT), StrokeKind::Inside);
    }

    // Pivot.
    let pivot = p(shape.pivot);
    painter.circle_stroke(pivot, 4.0, accent);
    painter.line_segment([pivot - vec2(7.0, 0.0), pivot + vec2(7.0, 0.0)], accent);
    painter.line_segment([pivot - vec2(0.0, 7.0), pivot + vec2(0.0, 7.0)], accent);
}
