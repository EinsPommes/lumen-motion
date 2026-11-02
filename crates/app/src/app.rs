use std::path::{Path, PathBuf};
use std::time::Instant;

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use eframe::egui_wgpu::wgpu;
use lumen_core::glam::Vec2;
use lumen_core::time::{self, LoopMode};
use lumen_core::{
    AddLayer, Channel, Command, FrameRange, History, LayerId, LayerKind, LayerProps, ModifyLayer,
    MoveLayer, Project, RemoveLayer, Time,
};
use lumen_render::{CanvasRenderer, FrameParams};

use crate::gizmo::GizmoDrag;
use crate::panels::timeline::KeyDrag;
use crate::{panels, theme};

pub const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
pub const REDO: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
pub const REDO_ALT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
pub const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
pub const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
pub const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
pub const SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);

/// File extension of project files.
pub const PROJECT_EXTENSION: &str = "lumen";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibraryTab {
    Library,
    #[default]
    Layers,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InspectorTab {
    #[default]
    Properties,
    Behaviours,
    Filters,
}

/// Real-time playback state, anchored to wall-clock time.
pub struct Transport {
    pub playing: bool,
    pub loop_mode: LoopMode,
    anchor_frame: i64,
    anchor_instant: Instant,
}

impl Transport {
    fn new() -> Self {
        Self {
            playing: false,
            loop_mode: LoopMode::Loop,
            anchor_frame: 0,
            anchor_instant: Instant::now(),
        }
    }

    fn anchor(&mut self, frame: i64) {
        self.anchor_frame = frame;
        self.anchor_instant = Instant::now();
    }
}

pub struct ViewOptions {
    pub safe_areas: bool,
}

struct Canvas {
    renderer: CanvasRenderer,
    texture_id: Option<egui::TextureId>,
}

pub struct LumenApp {
    pub project: Project,
    pub history: History,
    pub playhead: i64,
    pub transport: Transport,
    pub view: ViewOptions,
    pub library_tab: LibraryTab,
    pub library_search: String,
    pub inspector_tab: InspectorTab,
    pub selection: Option<LayerId>,
    pub gizmo_drag: Option<GizmoDrag>,
    pub key_drag: Option<KeyDrag>,
    /// When set, edits to static channels create keyframes.
    pub record: bool,
    /// Last error shown in the status area.
    pub status: Option<String>,
    pub gpu_info: String,
    pub file_path: Option<PathBuf>,
    pub dirty: bool,
    window_title: String,
    canvas: Canvas,
}

impl LumenApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        theme::apply(&cc.egui_ctx);

        let rs = cc
            .wgpu_render_state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("wgpu renderer is not available"))?;
        let info = rs.adapter.get_info();
        tracing::info!(adapter = %info.name, backend = ?info.backend, "GPU initialised");

        Ok(Self {
            project: Project::new("Untitled Project"),
            history: History::default(),
            playhead: 0,
            transport: Transport::new(),
            view: ViewOptions { safe_areas: false },
            library_tab: LibraryTab::default(),
            library_search: String::new(),
            inspector_tab: InspectorTab::default(),
            selection: None,
            gizmo_drag: None,
            key_drag: None,
            record: false,
            status: None,
            gpu_info: format!("{} · {:?}", info.name, info.backend),
            file_path: None,
            dirty: false,
            window_title: String::new(),
            canvas: Canvas {
                renderer: CanvasRenderer::new(&rs.device, &rs.queue),
                texture_id: None,
            },
        })
    }

    // --- editing ---------------------------------------------------------

    /// Executes a command; continuous edits merge until the gesture ends.
    /// Returns `true` on success.
    pub fn execute(&mut self, cmd: Box<dyn Command>) -> bool {
        let ok = match self.history.execute(&mut self.project, cmd) {
            Ok(()) => {
                self.status = None;
                self.dirty = true;
                true
            }
            Err(e) => {
                tracing::warn!(error = %e, "command failed");
                self.status = Some(e.to_string());
                false
            }
        };
        self.after_edit();
        ok
    }

    /// Executes a command as its own undo step (buttons, menu actions).
    pub fn execute_discrete(&mut self, cmd: Box<dyn Command>) -> bool {
        self.history.seal();
        let ok = self.execute(cmd);
        self.history.seal();
        ok
    }

    pub fn undo(&mut self) {
        match self.history.undo(&mut self.project) {
            Ok(true) => self.dirty = true,
            Ok(false) => {}
            Err(e) => self.status = Some(e.to_string()),
        }
        self.after_edit();
    }

    pub fn redo(&mut self) {
        match self.history.redo(&mut self.project) {
            Ok(true) => self.dirty = true,
            Ok(false) => {}
            Err(e) => self.status = Some(e.to_string()),
        }
        self.after_edit();
    }

    fn after_edit(&mut self) {
        self.clamp_playhead();
        if let Some(id) = self.selection {
            if !self.project.scene.contains(id) {
                self.selection = None;
            }
        }
        if let Some(drag) = &self.gizmo_drag {
            if !self.project.scene.contains(drag.id) {
                self.gizmo_drag = None;
            }
        }
    }

    // --- layers ----------------------------------------------------------

    /// Where a new layer goes: above the selection, or into a selected group.
    fn insertion_point(&self) -> (Option<LayerId>, usize) {
        let scene = &self.project.scene;
        match self.selection.and_then(|id| scene.layer(id)) {
            Some(l) if l.props.kind.is_group() => (Some(l.id()), l.children().len()),
            Some(l) => match scene.position_of(l.id()) {
                Ok((parent, index)) => (parent, index + 1),
                Err(_) => (None, scene.root().len()),
            },
            None => (None, scene.root().len()),
        }
    }

    pub fn add_layer(&mut self, props: LayerProps) {
        let id = self.project.scene.allocate_id();
        let (parent, index) = self.insertion_point();
        if self.execute_discrete(Box::new(AddLayer::new(id, parent, index, props))) {
            self.selection = Some(id);
        }
    }

    fn next_name(&self, base: &str) -> String {
        let n = self
            .project
            .scene
            .layers()
            .filter(|l| l.props.kind.type_name() == base)
            .count();
        format!("{base} {}", n + 1)
    }

    pub fn add_rectangle(&mut self) {
        let props = LayerProps::new(
            self.next_name("Rectangle"),
            LayerKind::Rectangle {
                size: Vec2::new(480.0, 270.0),
                color: [0.92, 0.30, 0.06, 1.0],
                corner_radius: 12.0,
            },
        );
        self.add_layer(props);
    }

    pub fn add_group(&mut self) {
        let props = LayerProps::new(self.next_name("Group"), LayerKind::Group);
        self.add_layer(props);
    }

    pub fn add_image_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import Image")
            .add_filter("Images", lumen_media::IMAGE_EXTENSIONS)
            .pick_file()
        else {
            return;
        };
        self.add_image(&path);
    }

    pub fn add_image(&mut self, path: &Path) {
        let (w, h) = match lumen_media::image_dimensions(path) {
            Ok(d) => d,
            Err(e) => {
                self.status = Some(e.to_string());
                return;
            }
        };
        let size = Vec2::new(w as f32, h as f32);
        let name = path
            .file_name()
            .map_or_else(|| self.next_name("Image"), |n| n.to_string_lossy().into_owned());
        let mut props = LayerProps::new(
            name,
            LayerKind::Image {
                path: path.to_owned(),
                size,
            },
        );
        // Fit large images into 80 % of the canvas.
        let s = &self.project.settings;
        let fit = (0.8 * s.width as f32 / size.x)
            .min(0.8 * s.height as f32 / size.y)
            .min(1.0);
        props.transform.scale = Vec2::splat(fit);
        self.add_layer(props);
    }

    pub fn delete_selected(&mut self) {
        if let Some(id) = self.selection {
            self.execute_discrete(Box::new(RemoveLayer::new(id)));
        }
    }

    /// Replaces a layer's properties (merges during a gesture).
    pub fn modify_layer(&mut self, id: LayerId, props: LayerProps) {
        let Some(before) = self.project.scene.layer(id).map(|l| l.props.clone()) else {
            return;
        };
        if before != props {
            self.execute(Box::new(ModifyLayer::new(id, before, props)));
        }
    }

    pub fn set_visible(&mut self, id: LayerId, visible: bool) {
        let Some(mut props) = self.project.scene.layer(id).map(|l| l.props.clone()) else {
            return;
        };
        props.visible = visible;
        self.history.seal();
        self.modify_layer(id, props);
        self.history.seal();
    }

    /// Moves the selection up (+1, towards the top) or down within its parent.
    pub fn restack_selected(&mut self, delta: isize) {
        let Some(id) = self.selection else { return };
        let Ok((parent, index)) = self.project.scene.position_of(id) else {
            return;
        };
        let len = self
            .project
            .scene
            .children_of(parent)
            .map_or(0, |c| c.len());
        let target = index.saturating_add_signed(delta).min(len.saturating_sub(1));
        if target != index {
            self.execute_discrete(Box::new(MoveLayer::new(id, parent, target)));
        }
    }

    pub fn move_into_group(&mut self, id: LayerId, group: LayerId) {
        let len = self
            .project
            .scene
            .children_of(Some(group))
            .map_or(0, |c| c.len());
        self.execute_discrete(Box::new(MoveLayer::new(id, Some(group), len)));
    }

    pub fn move_out_of_group(&mut self, id: LayerId) {
        let scene = &self.project.scene;
        let Some(parent) = scene.layer(id).and_then(|l| l.parent()) else {
            return;
        };
        let Ok((grand, index)) = scene.position_of(parent) else {
            return;
        };
        self.execute_discrete(Box::new(MoveLayer::new(id, grand, index + 1)));
    }

    // --- files -----------------------------------------------------------

    /// Asks to save unsaved changes. Returns `true` if the caller may proceed.
    pub fn confirm_discard(&mut self) -> bool {
        if !self.dirty {
            return true;
        }
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Unsaved Changes")
            .set_description(format!(
                "Save changes to \"{}\" before continuing?",
                self.project.name
            ))
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show();
        match answer {
            rfd::MessageDialogResult::Yes => self.save(false),
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }

    fn replace_project(&mut self, project: Project, path: Option<PathBuf>) {
        self.project = project;
        self.file_path = path;
        self.history.clear();
        self.selection = None;
        self.gizmo_drag = None;
        self.key_drag = None;
        self.record = false;
        self.transport.playing = false;
        self.playhead = 0;
        self.dirty = false;
        self.status = None;
    }

    pub fn new_project(&mut self) {
        if self.confirm_discard() {
            self.replace_project(Project::new("Untitled Project"), None);
        }
    }

    pub fn open_dialog(&mut self) {
        if !self.confirm_discard() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open Project")
            .add_filter("Lumen Motion Project", &[PROJECT_EXTENSION])
            .pick_file()
        else {
            return;
        };
        self.open(&path);
    }

    pub fn open(&mut self, path: &Path) {
        let result = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| Project::from_ron(&text).map_err(|e| e.to_string()));
        match result {
            Ok(project) => {
                tracing::info!(path = %path.display(), "project opened");
                self.replace_project(project, Some(path.to_owned()));
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to open project");
                self.status = Some(format!("Could not open {}: {e}", path.display()));
            }
        }
    }

    /// Saves the project; asks for a path if needed. Returns `true` on success.
    pub fn save(&mut self, save_as: bool) -> bool {
        let path = match (&self.file_path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let Some(mut p) = rfd::FileDialog::new()
                    .set_title("Save Project")
                    .add_filter("Lumen Motion Project", &[PROJECT_EXTENSION])
                    .set_file_name(format!("{}.{PROJECT_EXTENSION}", self.project.name))
                    .save_file()
                else {
                    return false;
                };
                if p.extension().is_none_or(|e| e != PROJECT_EXTENSION) {
                    p.set_extension(PROJECT_EXTENSION);
                }
                p
            }
        };
        if let Some(stem) = path.file_stem() {
            self.project.name = stem.to_string_lossy().into_owned();
        }
        let result = self
            .project
            .to_ron()
            .map_err(|e| e.to_string())
            .and_then(|text| write_atomic(&path, &text).map_err(|e| e.to_string()));
        match result {
            Ok(()) => {
                tracing::info!(path = %path.display(), "project saved");
                self.file_path = Some(path);
                self.dirty = false;
                self.status = None;
                true
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to save project");
                self.status = Some(format!("Could not save: {e}"));
                false
            }
        }
    }

    // --- transport -------------------------------------------------------

    /// Time of the current playhead frame.
    pub fn time(&self) -> Time {
        Time::from_frame(self.playhead, self.project.settings.frame_rate).unwrap_or(Time::ZERO)
    }

    /// Jumps to the previous (`forward == false`) or next keyframe of the
    /// selected layer.
    pub fn jump_to_key(&mut self, forward: bool) {
        let Some(layer) = self.selection.and_then(|id| self.project.scene.layer(id)) else {
            return;
        };
        let anim = &layer.props.animation;
        let now = self.time();
        let target = if forward {
            anim.next_key(&Channel::ALL, now)
        } else {
            anim.previous_key(&Channel::ALL, now)
        };
        if let Some(t) = target {
            self.set_playhead(t.to_frame_round(self.project.settings.frame_rate));
        }
    }

    pub fn frame_range(&self) -> FrameRange {
        self.project.settings.frame_range()
    }

    pub fn set_playhead(&mut self, frame: i64) {
        self.playhead = self.frame_range().clamp(frame);
        if self.transport.playing {
            self.transport.anchor(self.playhead);
        }
    }

    pub fn step(&mut self, delta: i64) {
        self.set_playhead(self.playhead.saturating_add(delta));
    }

    pub fn toggle_playback(&mut self) {
        if self.transport.playing {
            self.transport.playing = false;
        } else {
            let range = self.frame_range();
            // Restart from the beginning if parked at the end in "once" mode.
            if self.transport.loop_mode == LoopMode::Once && self.playhead >= range.last() {
                self.playhead = range.start();
            }
            self.transport.playing = true;
            self.transport.anchor(self.playhead);
        }
    }

    fn clamp_playhead(&mut self) {
        let clamped = self.frame_range().clamp(self.playhead);
        if clamped != self.playhead {
            self.set_playhead(clamped);
        }
    }

    fn update_playback(&mut self, ctx: &egui::Context) {
        if !self.transport.playing {
            return;
        }
        let step = time::advance(
            self.transport.anchor_frame,
            self.transport.anchor_instant.elapsed().as_secs_f64(),
            self.project.settings.frame_rate,
            self.frame_range(),
            self.transport.loop_mode,
        );
        self.playhead = step.frame;
        if step.finished {
            self.transport.playing = false;
        } else {
            ctx.request_repaint();
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Shift variants first: consume_shortcut ignores extra Shift.
        if ctx.input_mut(|i| i.consume_shortcut(&REDO) || i.consume_shortcut(&REDO_ALT)) {
            self.redo();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&UNDO)) {
            self.undo();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&SAVE_AS)) {
            self.save(true);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&SAVE)) {
            self.save(false);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
            self.open_dialog();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&NEW)) {
            self.new_project();
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        let pressed = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
        if pressed(Key::Space) {
            self.toggle_playback();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::ArrowLeft)) {
            self.jump_to_key(false);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::ArrowRight)) {
            self.jump_to_key(true);
        }
        if pressed(Key::ArrowLeft) {
            self.step(-1);
        }
        if pressed(Key::ArrowRight) {
            self.step(1);
        }
        if pressed(Key::R) {
            self.record = !self.record;
        }
        if pressed(Key::Home) {
            self.set_playhead(self.frame_range().start());
        }
        if pressed(Key::End) {
            self.set_playhead(self.frame_range().last());
        }
        if pressed(Key::Delete) || pressed(Key::Backspace) {
            self.delete_selected();
        }
        if pressed(Key::Escape) {
            self.selection = None;
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.confirm_discard() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = format!(
            "{}{} — Lumen Motion",
            self.project.name,
            if self.dirty { " •" } else { "" }
        );
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    // --- rendering -------------------------------------------------------

    pub fn canvas_texture(&self) -> Option<egui::TextureId> {
        self.canvas.texture_id
    }

    fn render_canvas(&mut self, frame: &eframe::Frame) {
        let Some(rs) = frame.wgpu_render_state() else {
            return;
        };
        let s = &self.project.settings;
        let items = self.project.scene.flatten_with(&lumen_anim::AtTime(self.time()));
        let params = FrameParams {
            width: s.width,
            height: s.height,
            background: s.background,
            items: &items,
        };
        let recreated = self.canvas.renderer.render(&rs.device, &rs.queue, &params);

        let used: Vec<&Path> = self
            .project
            .scene
            .layers()
            .filter_map(|l| match &l.props.kind {
                LayerKind::Image { path, .. } => Some(path.as_path()),
                _ => None,
            })
            .collect();
        self.canvas.renderer.retain_images(&used);

        let Some(view) = self.canvas.renderer.view() else {
            return;
        };
        let mut egui_renderer = rs.renderer.write();
        match self.canvas.texture_id {
            Some(id) if recreated => egui_renderer.update_egui_texture_from_wgpu_texture(
                &rs.device,
                view,
                wgpu::FilterMode::Linear,
                id,
            ),
            Some(_) => {}
            None => {
                self.canvas.texture_id = Some(egui_renderer.register_native_texture(
                    &rs.device,
                    view,
                    wgpu::FilterMode::Linear,
                ));
            }
        }
    }
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("{PROJECT_EXTENSION}.tmp"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

impl eframe::App for LumenApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.handle_close_request(ctx);
        self.handle_shortcuts(ctx);
        self.update_playback(ctx);

        panels::menu_bar(self, ctx);
        panels::timeline::show(self, ctx);
        panels::library::show(self, ctx);
        panels::inspector::show(self, ctx);
        panels::canvas::show(self, ctx);

        // Render after UI edits so changes show up in the same frame.
        self.render_canvas(frame);
        self.update_title(ctx);

        // A gesture ends when no pointer button is held and no text field
        // is being edited.
        if !ctx.input(|i| i.pointer.any_down()) && !ctx.wants_keyboard_input() {
            self.history.seal();
        }
    }
}
