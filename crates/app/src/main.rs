#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod app;
mod gizmo;
mod panels;
mod theme;

use eframe::egui;
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,wgpu_core=warn,wgpu_hal=error,naga=warn"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lumen Motion")
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([960.0, 540.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Lumen Motion",
        options,
        Box::new(|cc| {
            let app = app::LumenApp::new(cc)?;
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("failed to start Lumen Motion: {e}"))
}
