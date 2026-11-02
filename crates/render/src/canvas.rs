use std::collections::HashMap;
use std::mem::size_of;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};

use lumen_core::{DrawItem, LayerKind};

/// Format of the canvas texture. sRGB storage, linear blending.
pub const CANVAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Format used for imported images.
const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Floats per layer uniform block (mirrors `LayerData` in `layers.wgsl`).
const LAYER_FLOATS: usize = 16;
const LAYER_BYTES: u64 = (LAYER_FLOATS * size_of::<f32>()) as u64;

/// Inputs for rendering one frame.
#[derive(Clone, Copy, Debug)]
pub struct FrameParams<'a> {
    pub width: u32,
    pub height: u32,
    /// Linear-light RGBA.
    pub background: [f32; 4],
    /// Layers in draw order (bottom first).
    pub items: &'a [DrawItem<'a>],
}

#[derive(Debug)]
struct Target {
    // Owns the memory behind `view`.
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

#[derive(Debug)]
enum ImageEntry {
    Ready {
        _texture: wgpu::Texture,
        bind_group: wgpu::BindGroup,
    },
    Failed,
}

/// Composites the scene into an offscreen texture.
#[derive(Debug)]
pub struct CanvasRenderer {
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    layer_layout: wgpu::BindGroupLayout,
    layer_buffer: wgpu::Buffer,
    layer_bind_group: wgpu::BindGroup,
    layer_capacity: usize,
    layer_stride: u64,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: ImageEntry,
    images: HashMap<PathBuf, ImageEntry>,
    target: Option<Target>,
    scratch: Vec<u8>,
}

fn align_up(v: u64, align: u64) -> u64 {
    let a = align.max(1);
    v.div_ceil(a) * a
}

/// Uniform data for one layer, or `None` if it would not be visible.
fn layer_uniform(item: &DrawItem<'_>, image_ready: bool) -> Option<[f32; LAYER_FLOATS]> {
    let m = item.world.matrix2;
    let t = item.world.translation;
    let det = m.determinant();
    if !det.is_finite() || det.abs() < 1e-8 || item.opacity <= 0.0 {
        return None;
    }
    let (size, color, radius, kind) = match item.kind {
        LayerKind::Rectangle {
            size,
            color,
            corner_radius,
        } => (*size, *color, *corner_radius, 0.0),
        LayerKind::Image { size, .. } => {
            (*size, [1.0; 4], 0.0, if image_ready { 1.0 } else { 2.0 })
        }
        LayerKind::Group => return None,
    };
    if !(size.x > 0.0 && size.y > 0.0) {
        return None;
    }
    // Expand the quad by ~1.5 canvas pixels so edges can be antialiased.
    let px_per_unit = m.x_axis.length().min(m.y_axis.length()).max(1e-4);
    let margin = (1.5 / px_per_unit).min(1e4);
    Some([
        m.x_axis.x,
        m.x_axis.y,
        m.y_axis.x,
        m.y_axis.y,
        t.x,
        t.y,
        size.x,
        size.y,
        color[0],
        color[1],
        color[2],
        color[3],
        radius.max(0.0),
        item.opacity.clamp(0.0, 1.0),
        kind,
        margin,
    ])
}

impl CanvasRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lumen.layers.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/layers.wgsl").into()),
        });

        let layer_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lumen.layers.bgl0"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(LAYER_BYTES),
                    },
                    count: None,
                },
            ],
        });

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lumen.layers.bgl1"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lumen.layers.globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let layer_stride = align_up(
            LAYER_BYTES,
            u64::from(device.limits().min_uniform_buffer_offset_alignment),
        );
        let layer_capacity = 64;
        let (layer_buffer, layer_bind_group) =
            Self::create_layer_buffer(device, &layer_layout, &globals, layer_stride, layer_capacity);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lumen.layers.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lumen.layers.layout"),
            bind_group_layouts: &[&layer_layout, &texture_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lumen.layers.pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: CANVAS_FORMAT,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let mut renderer = Self {
            pipeline,
            globals,
            layer_layout,
            layer_buffer,
            layer_bind_group,
            layer_capacity,
            layer_stride,
            texture_layout,
            sampler,
            white: ImageEntry::Failed,
            images: HashMap::new(),
            target: None,
            scratch: Vec::new(),
        };
        renderer.white = renderer.upload_rgba(device, queue, "lumen.white", 1, 1, &[255; 4]);
        renderer
    }

    fn create_layer_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        globals: &wgpu::Buffer,
        stride: u64,
        capacity: usize,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lumen.layers.uniforms"),
            size: stride * capacity as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lumen.layers.bg0"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buffer,
                        offset: 0,
                        size: NonZeroU64::new(LAYER_BYTES),
                    }),
                },
            ],
        });
        (buffer, bind_group)
    }

    fn upload_rgba(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> ImageEntry {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: IMAGE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        ImageEntry::Ready {
            _texture: texture,
            bind_group,
        }
    }

    fn load_image(&self, device: &wgpu::Device, queue: &wgpu::Queue, path: &Path) -> ImageEntry {
        let max = device.limits().max_texture_dimension_2d;
        match lumen_media::load_image(path, max) {
            Ok(img) => {
                tracing::debug!(path = %path.display(), img.width, img.height, "image uploaded");
                self.upload_rgba(device, queue, "lumen.image", img.width, img.height, &img.rgba)
            }
            Err(e) => {
                tracing::warn!(error = %e, "image could not be loaded");
                ImageEntry::Failed
            }
        }
    }

    /// Drops cached GPU images that the scene no longer references.
    pub fn retain_images(&mut self, used: &[&Path]) {
        self.images.retain(|p, _| used.contains(&p.as_path()));
    }

    /// (Re)creates the target if the size changed. Returns `true` if so.
    fn ensure_target(&mut self, device: &wgpu::Device, width: u32, height: u32) -> bool {
        if let Some(t) = &self.target {
            if t.width == width && t.height == height {
                return false;
            }
        }
        tracing::debug!(width, height, "creating canvas texture");
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lumen.canvas.target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CANVAS_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.target = Some(Target {
            _texture: texture,
            view,
            width,
            height,
        });
        true
    }

    /// Renders one frame. Returns `true` if the target texture was recreated,
    /// in which case UI texture handles must be refreshed.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        params: &FrameParams<'_>,
    ) -> bool {
        let max = device.limits().max_texture_dimension_2d;
        let width = params.width.clamp(1, max);
        let height = params.height.clamp(1, max);
        let recreated = self.ensure_target(device, width, height);

        // Load images referenced for the first time.
        for item in params.items {
            if let LayerKind::Image { path, .. } = item.kind {
                if !self.images.contains_key(path) {
                    let entry = self.load_image(device, queue, path);
                    self.images.insert(path.clone(), entry);
                }
            }
        }

        // Grow the uniform buffer if needed.
        let count = params.items.len();
        if count > self.layer_capacity {
            self.layer_capacity = count.next_power_of_two();
            let (buffer, bind_group) = Self::create_layer_buffer(
                device,
                &self.layer_layout,
                &self.globals,
                self.layer_stride,
                self.layer_capacity,
            );
            self.layer_buffer = buffer;
            self.layer_bind_group = bind_group;
        }

        // Pack per-layer uniforms.
        let stride = self.layer_stride as usize;
        self.scratch.clear();
        self.scratch.resize(count * stride, 0);
        let mut draws: Vec<(u32, Option<&wgpu::BindGroup>)> = Vec::with_capacity(count);
        for (i, item) in params.items.iter().enumerate() {
            let image = match item.kind {
                LayerKind::Image { path, .. } => match self.images.get(path) {
                    Some(ImageEntry::Ready { bind_group, .. }) => Some(bind_group),
                    _ => None,
                },
                _ => None,
            };
            let Some(data) = layer_uniform(item, image.is_some()) else {
                continue;
            };
            let offset = i * stride;
            self.scratch[offset..offset + LAYER_BYTES as usize]
                .copy_from_slice(bytemuck::cast_slice(&data));
            let Ok(dyn_offset) = u32::try_from(offset) else {
                continue;
            };
            draws.push((dyn_offset, image));
        }

        let globals = [width as f32, height as f32, 0.0, 0.0];
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&globals));
        if !self.scratch.is_empty() {
            queue.write_buffer(&self.layer_buffer, 0, &self.scratch);
        }

        let white = match &self.white {
            ImageEntry::Ready { bind_group, .. } => Some(bind_group),
            ImageEntry::Failed => None,
        };
        let Some(target) = &self.target else {
            return recreated;
        };
        let bg = params.background;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("lumen.canvas.encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumen.canvas.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(bg[0] * bg[3]),
                            g: f64::from(bg[1] * bg[3]),
                            b: f64::from(bg[2] * bg[3]),
                            a: f64::from(bg[3]),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            for (offset, image) in draws {
                let Some(texture) = image.or(white) else {
                    continue;
                };
                pass.set_bind_group(0, &self.layer_bind_group, &[offset]);
                pass.set_bind_group(1, texture, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        queue.submit(std::iter::once(encoder.finish()));
        recreated
    }

    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|t| &t.view)
    }

    pub fn size(&self) -> Option<(u32, u32)> {
        self.target.as_ref().map(|t| (t.width, t.height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Affine2, Vec2};
    use lumen_core::LayerId;

    fn rect_kind() -> LayerKind {
        LayerKind::Rectangle {
            size: Vec2::new(100.0, 50.0),
            color: [1.0, 0.5, 0.25, 1.0],
            corner_radius: 4.0,
        }
    }

    #[test]
    fn uniform_layout_for_rectangle() {
        let kind = rect_kind();
        let item = DrawItem {
            id: LayerId(1),
            world: Affine2::from_scale_angle_translation(Vec2::splat(2.0), 0.0, Vec2::new(10.0, 20.0)),
            opacity: 0.5,
            kind: &kind,
        };
        let u = layer_uniform(&item, false).unwrap();
        assert_eq!(&u[..8], &[2.0, 0.0, 0.0, 2.0, 10.0, 20.0, 100.0, 50.0]);
        assert_eq!(&u[8..12], &[1.0, 0.5, 0.25, 1.0]);
        assert_eq!(u[12], 4.0);
        assert_eq!(u[13], 0.5);
        assert_eq!(u[14], 0.0);
        assert!((u[15] - 0.75).abs() < 1e-6, "1.5 px margin at 2 px/unit");
    }

    #[test]
    fn invisible_or_degenerate_layers_are_skipped() {
        let kind = rect_kind();
        let mut item = DrawItem {
            id: LayerId(1),
            world: Affine2::from_scale(Vec2::new(0.0, 1.0)),
            opacity: 1.0,
            kind: &kind,
        };
        assert!(layer_uniform(&item, false).is_none());
        item.world = Affine2::IDENTITY;
        item.opacity = 0.0;
        assert!(layer_uniform(&item, false).is_none());
        let group = LayerKind::Group;
        item.opacity = 1.0;
        item.kind = &group;
        assert!(layer_uniform(&item, false).is_none());
    }

    #[test]
    fn missing_image_uses_placeholder_kind() {
        let kind = LayerKind::Image {
            path: PathBuf::from("x.png"),
            size: Vec2::new(10.0, 10.0),
        };
        let item = DrawItem {
            id: LayerId(1),
            world: Affine2::IDENTITY,
            opacity: 1.0,
            kind: &kind,
        };
        assert_eq!(layer_uniform(&item, true).unwrap()[14], 1.0);
        assert_eq!(layer_uniform(&item, false).unwrap()[14], 2.0);
    }

    #[test]
    fn stride_alignment() {
        assert_eq!(align_up(64, 256), 256);
        assert_eq!(align_up(300, 256), 512);
        assert_eq!(align_up(64, 0), 64);
    }
}

#[cfg(test)]
mod shader_tests {
    #[test]
    fn layers_shader_validates() {
        let src = include_str!("shaders/layers.wgsl");
        let module = naga::front::wgsl::parse_str(src).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }
}
