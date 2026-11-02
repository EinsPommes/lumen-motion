# Lumen Motion

Real-time motion-graphics compositor written in Rust, built on wgpu and egui.

## Building

```sh
cargo run --release
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Set `RUST_LOG=debug` for verbose logging.

## Workspace

| Crate             | Purpose                                         |
|-------------------|-------------------------------------------------|
| `lumen-core`      | Time model, scene graph, project file, undo     |
| `lumen-anim`      | Keyframes, interpolation curves, behaviours     |
| `lumen-render`    | wgpu compositing pipeline                       |
| `lumen-text`      | Text layout and titles                          |
| `lumen-particles` | GPU particle emitters                           |
| `lumen-media`     | Image and video import/export                   |
| `lumen-app`       | Desktop application                             |

## Shortcuts

| Key                     | Action                |
|-------------------------|-----------------------|
| Space                   | Play / pause          |
| ← / →                   | Previous / next frame |
| Shift+← / Shift+→       | Previous / next keyframe |
| Home / End              | Jump to start / end   |
| R                       | Toggle record mode    |
| Ctrl+Z                  | Undo                  |
| Ctrl+Shift+Z, Ctrl+Y    | Redo                  |
| Ctrl+N / Ctrl+O         | New / open project    |
| Ctrl+S / Ctrl+Shift+S   | Save / save as        |
| Del                     | Delete selected layer |
| Esc                     | Clear selection       |

## Conventions

- No `unwrap()` or `expect()` outside tests; enforced by clippy.
- All project edits go through `lumen_core::Command` so they can be undone.
- Times are exact rationals (`Time`, `FrameRate`); floats only at the UI/GPU boundary.
