# Lumen Motion

Real-time motion-graphics compositor in Rust (wgpu + egui).

## Build & run

```sh
cargo run --release            # starts the app (binary: lumen-motion)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Logging: `RUST_LOG=debug cargo run`.

## Workspace

| Crate             | Purpose                                                   |
|-------------------|-----------------------------------------------------------|
| `lumen-core`      | Time model, project document, undo/redo (scene graph: M2) |
| `lumen-anim`      | Keyframes, curves, behaviours (M3, M5)                    |
| `lumen-render`    | wgpu pipeline, compositing, filters                       |
| `lumen-text`      | Text layout, titles, extrusion (M4, M9)                   |
| `lumen-particles` | GPU particle emitters (M7)                                |
| `lumen-media`     | FFmpeg import/export, frame cache (M10)                   |
| `lumen-app`       | egui application                                          |

## Shortcuts

Space play/pause · ←/→ step frame · Home/End jump · Ctrl+Z undo · Ctrl+Shift+Z / Ctrl+Y redo

## Conventions

- No `unwrap()`/`expect()` outside tests (enforced by clippy).
- All project mutations go through `lumen_core::Command` + `History`.
- Times are exact rationals (`Time`, `FrameRate`); floats only at the UI/GPU edge.
