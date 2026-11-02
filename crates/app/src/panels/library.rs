use eframe::egui::{self, RichText};
use lumen_core::{LayerId, LayerKind};

use crate::app::{LibraryTab, LumenApp};
use crate::theme;

struct Category {
    name: &'static str,
    milestone: u8,
    items: &'static [&'static str],
}

const CATEGORIES: &[Category] = &[
    Category {
        name: "Shapes & Media",
        milestone: 2,
        items: &["Rectangle", "Image", "Group"],
    },
    Category {
        name: "Text",
        milestone: 4,
        items: &["Title", "Subtitle", "Lower Third", "Credits"],
    },
    Category {
        name: "Behaviours",
        milestone: 5,
        items: &["Throw", "Spin", "Fade In/Out", "Wiggle", "Oscillate", "Grow/Shrink"],
    },
    Category {
        name: "Filters",
        milestone: 6,
        items: &["Blur", "Glow", "Neon", "Color Correction"],
    },
    Category {
        name: "Particles",
        milestone: 7,
        items: &["Smoke", "Sparks", "Confetti", "Snow", "Rain", "Fire", "Dust", "Bubbles", "Stars", "Embers"],
    },
    Category {
        name: "Templates",
        milestone: 11,
        items: &["Titles", "Transitions", "Effects"],
    },
];

pub fn show(app: &mut LumenApp, ctx: &egui::Context) {
    egui::SidePanel::left("library")
        .resizable(true)
        .default_width(260.0)
        .width_range(200.0..=440.0)
        .show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut app.library_tab, LibraryTab::Layers, "Layers");
                ui.selectable_value(&mut app.library_tab, LibraryTab::Library, "Library");
            });
            ui.separator();
            match app.library_tab {
                LibraryTab::Library => library(app, ui),
                LibraryTab::Layers => layers(app, ui),
            }
        });
}

fn kind_icon(kind: &LayerKind) -> &'static str {
    match kind {
        LayerKind::Rectangle { .. } => "⬛",
        LayerKind::Image { .. } => "🖼",
        LayerKind::Group => "📁",
    }
}

fn layers(app: &mut LumenApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("+ Rectangle").clicked() {
            app.add_rectangle();
        }
        if ui.button("+ Image …").clicked() {
            app.add_image_dialog();
        }
        if ui.button("+ Group").clicked() {
            app.add_group();
        }
    });
    ui.horizontal(|ui| {
        let has_sel = app.selection.is_some();
        if ui
            .add_enabled(has_sel, egui::Button::new("▲"))
            .on_hover_text("Bring forward")
            .clicked()
        {
            app.restack_selected(1);
        }
        if ui
            .add_enabled(has_sel, egui::Button::new("▼"))
            .on_hover_text("Send backward")
            .clicked()
        {
            app.restack_selected(-1);
        }
        if ui
            .add_enabled(has_sel, egui::Button::new("Delete"))
            .on_hover_text("Delete layer (Del)")
            .clicked()
        {
            app.delete_selected();
        }
    });
    ui.separator();

    let order = app.project.scene.display_order();
    if order.is_empty() {
        ui.label(RichText::new("No layers yet.").color(theme::TEXT_WEAK));
        ui.label(
            RichText::new("Add a rectangle, an image or a group above.")
                .small()
                .color(theme::TEXT_WEAK),
        );
        return;
    }
    let groups: Vec<(LayerId, String)> = app
        .project
        .scene
        .layers()
        .filter(|l| l.props.kind.is_group())
        .map(|l| (l.id(), l.props.name.clone()))
        .collect();

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (id, depth) in order {
            let Some(layer) = app.project.scene.layer(id) else {
                continue;
            };
            let label = format!("{} {}", kind_icon(&layer.props.kind), layer.props.name);
            let mut visible = layer.props.visible;
            let parent = layer.parent();
            let selected = app.selection == Some(id);

            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 14.0);
                if ui
                    .checkbox(&mut visible, "")
                    .on_hover_text("Visible")
                    .changed()
                {
                    app.set_visible(id, visible);
                }
                let text = if visible {
                    RichText::new(label)
                } else {
                    RichText::new(label).color(theme::TEXT_WEAK)
                };
                let resp = ui.selectable_label(selected, text);
                if resp.clicked() {
                    app.selection = Some(id);
                }
                resp.context_menu(|ui| {
                    app.selection = Some(id);
                    let targets: Vec<&(LayerId, String)> = groups
                        .iter()
                        .filter(|(g, _)| {
                            *g != id
                                && Some(*g) != parent
                                && !app.project.scene.is_ancestor(id, *g)
                        })
                        .collect();
                    ui.add_enabled_ui(!targets.is_empty(), |ui| {
                        ui.menu_button("Move into Group", |ui| {
                            for (g, name) in targets {
                                if ui.button(name).clicked() {
                                    app.move_into_group(id, *g);
                                    ui.close_menu();
                                }
                            }
                        });
                    });
                    if ui
                        .add_enabled(parent.is_some(), egui::Button::new("Move out of Group"))
                        .clicked()
                    {
                        app.move_out_of_group(id);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Delete").clicked() {
                        app.delete_selected();
                        ui.close_menu();
                    }
                });
            });
        }
    });
}

fn library(app: &mut LumenApp, ui: &mut egui::Ui) {
    ui.add(
        egui::TextEdit::singleline(&mut app.library_search)
            .hint_text("Search …")
            .desired_width(f32::INFINITY),
    );
    ui.add_space(4.0);

    let query = app.library_search.trim().to_lowercase();
    let searching = !query.is_empty();

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let mut any = false;
        for cat in CATEGORIES {
            let items: Vec<&str> = cat
                .items
                .iter()
                .copied()
                .filter(|i| !searching || i.to_lowercase().contains(&query))
                .collect();
            if searching && items.is_empty() {
                continue;
            }
            any = true;
            let available = cat.milestone <= 2;
            egui::CollapsingHeader::new(RichText::new(cat.name).strong())
                .id_salt(cat.name)
                .default_open(available)
                .open(searching.then_some(true))
                .show(ui, |ui| {
                    for item in items {
                        if available {
                            if ui
                                .add(egui::Button::new(item).frame(false))
                                .on_hover_text("Click to add")
                                .clicked()
                            {
                                match item {
                                    "Rectangle" => app.add_rectangle(),
                                    "Image" => app.add_image_dialog(),
                                    "Group" => app.add_group(),
                                    _ => {}
                                }
                            }
                        } else {
                            ui.label(RichText::new(item).color(theme::TEXT_WEAK))
                                .on_hover_text(format!("Available from milestone {}", cat.milestone));
                        }
                    }
                });
        }
        if !any {
            ui.label(RichText::new("No matches.").color(theme::TEXT_WEAK));
        }
    });
}
