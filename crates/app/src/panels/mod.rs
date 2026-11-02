pub mod canvas;
pub mod inspector;
pub mod library;
pub mod timeline;

use eframe::egui::{self, Button, RichText};

use crate::app::{LumenApp, NEW, OPEN, REDO, SAVE, SAVE_AS, UNDO};
use crate::theme;

pub fn menu_bar(app: &mut LumenApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            ui.label(RichText::new("◆ Lumen Motion").strong().color(theme::ACCENT));
            ui.separator();

            ui.menu_button("File", |ui| {
                let ctx = ui.ctx().clone();
                let sc = |s| ctx.format_shortcut(s);
                if ui.add(Button::new("New Project").shortcut_text(sc(&NEW))).clicked() {
                    ui.close_menu();
                    app.new_project();
                }
                if ui.add(Button::new("Open …").shortcut_text(sc(&OPEN))).clicked() {
                    ui.close_menu();
                    app.open_dialog();
                }
                ui.separator();
                if ui.add(Button::new("Save").shortcut_text(sc(&SAVE))).clicked() {
                    ui.close_menu();
                    app.save(false);
                }
                if ui.add(Button::new("Save As …").shortcut_text(sc(&SAVE_AS))).clicked() {
                    ui.close_menu();
                    app.save(true);
                }
                ui.separator();
                ui.add_enabled(false, Button::new("Export …"))
                    .on_disabled_hover_text("Not available yet");
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            ui.menu_button("Edit", |ui| {
                let undo_text = app
                    .history
                    .undo_label()
                    .map_or_else(|| "Undo".to_owned(), |l| format!("Undo: {l}"));
                let undo_btn =
                    Button::new(undo_text).shortcut_text(ui.ctx().format_shortcut(&UNDO));
                if ui.add_enabled(app.history.can_undo(), undo_btn).clicked() {
                    app.undo();
                    ui.close_menu();
                }
                let redo_text = app
                    .history
                    .redo_label()
                    .map_or_else(|| "Redo".to_owned(), |l| format!("Redo: {l}"));
                let redo_btn =
                    Button::new(redo_text).shortcut_text(ui.ctx().format_shortcut(&REDO));
                if ui.add_enabled(app.history.can_redo(), redo_btn).clicked() {
                    app.redo();
                    ui.close_menu();
                }
            });

            ui.menu_button("Layer", |ui| {
                if ui.button("Add Rectangle").clicked() {
                    ui.close_menu();
                    app.add_rectangle();
                }
                if ui.button("Add Image …").clicked() {
                    ui.close_menu();
                    app.add_image_dialog();
                }
                if ui.button("Add Group").clicked() {
                    ui.close_menu();
                    app.add_group();
                }
                ui.separator();
                let has_sel = app.selection.is_some();
                if ui
                    .add_enabled(has_sel, Button::new("Bring Forward"))
                    .clicked()
                {
                    ui.close_menu();
                    app.restack_selected(1);
                }
                if ui
                    .add_enabled(has_sel, Button::new("Send Backward"))
                    .clicked()
                {
                    ui.close_menu();
                    app.restack_selected(-1);
                }
                if ui
                    .add_enabled(has_sel, Button::new("Delete").shortcut_text("Del"))
                    .clicked()
                {
                    ui.close_menu();
                    app.delete_selected();
                }
            });

            ui.menu_button("View", |ui| {
                ui.checkbox(&mut app.view.safe_areas, "Safe Areas");
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(&app.gpu_info).small().color(theme::TEXT_WEAK));
                if let Some(msg) = &app.status {
                    ui.label(RichText::new(msg).color(theme::ERROR));
                }
            });
        });
    });
}
