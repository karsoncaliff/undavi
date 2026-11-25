//! The dynamic background on the GPU: a custom wgpu primitive drawn by iced's
//! `shader` widget.
//!
//! Each frame runs three stages (see `assets/shaders/`):
//!
//! 1. **Scene** (`scene.wgsl`), at half resolution into an offscreen texture: the
//!    album art laid out as soft-edged copies, one cover or tiles, or the Ambient
//!    circles; two layers for the cross-fade between tracks; and the animated
//!    warp (Liquid, Warp, Spiral) of the whole picture.
//! 2. **Blur** (`kawase.wgsl`): dual Kawase, a chain of half-size downsamples and
//!    upsamples. Blurring the composed scene (rather than each cover) is what the
//!    original CSS design did, and it makes the Blur slider and the Focus pulse
//!    work instantly.
//! 3. **Composite** (`composite.wgsl`), onto the window: grading, dim, header
//!    scrim, vignette and grain.
//!
//! The CPU only uploads a cover when it changes; the rest is uniforms.

use crate::shaders::Compiled;
use iced::Rectangle;
use iced::futures::FutureExt;
use iced::wgpu;
use iced::widget::shader::{self, Viewport};
use image::RgbaImage;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The most down/up steps the blur takes (radius doubles with each).
const MAX_LEVELS: usize = 6;
/// The scene and blur run at this fraction of the window's pixels; the small
/// preview can afford all of them.
const SCENE_SCALE: f32 = 0.5;
const PREVIEW_SCALE: f32 = 1.0;
/// Pixel format of the offscreen textures.
const OFFSCREEN: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A cover image ready for the GPU, with an id so it's uploaded once.
#[derive(Clone)]
pub struct Cover {
    pub id: u64,
    pub image: Arc<RgbaImage>,
}

impl std::fmt::Debug for Cover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cover")
            .field("id", &self.id)
            .field("size", &self.image.dimensions())
            .finish()
    }
}

impl Cover {
    pub fn new(image: RgbaImage) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            image: Arc::new(image),
        }
    }
}

/// Uniforms for `scene.wgsl`'s `UndaviScene`, in field order (8 × vec4).
pub const SCENE_FLOATS: usize = 32;
/// Uniforms for its `UndaviAudio`: levels, extra, 16 bands, accent (7 × vec4).
pub const AUDIO_FLOATS: usize = 28;
/// Uniforms for `composite.wgsl` (4 × vec4).
pub const COMPOSITE_FLOATS: usize = 16;

/// Where a picture is drawn. Both can be on screen at once, each with its own
/// uniforms and textures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// The app itself.
    Screen,
    /// The mini preview in the settings window.
    Preview,
}

/// One frame of the background.
#[derive(Debug)]
pub struct Primitive {
    pub place: Place,
    pub scene: [f32; SCENE_FLOATS],
    pub composite: [f32; COMPOSITE_FLOATS],
    /// Blur radius in logical pixels.
    pub blur: f32,
    /// The covers the scene's two layers can show (slots A and B).
    pub covers: [Option<Cover>; 2],
    /// The background's shader.
    pub shader: Compiled,
    pub audio: [f32; AUDIO_FLOATS],
    /// The shader's `settings` uniform (`shaders::settings::Resolved::pack`).
    pub settings: Vec<u8>,
}

fn bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_ne_bytes()).collect()
}

/// Steps and tap offset for a dual Kawase blur of `radius` scene pixels. Each
/// step doubles the reach; the offset fills in between, so the radius changes
/// smoothly as the slider (or the Focus pulse) moves.
pub fn kawase(radius: f32) -> (usize, f32) {
    if radius < 0.75 {
        return (0, 0.0);
    }
    let steps = ((radius / 1.5).log2().floor() as usize).clamp(1, MAX_LEVELS);
    let offset = (radius / (1.5 * (1u32 << steps) as f32)).clamp(0.5, 4.0);
    (steps, offset)
}

struct Target {
    view: wgpu::TextureView,
    size: (u32, u32),
}

fn target(device: &wgpu::Device, size: (u32, u32), label: &str) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OFFSCREEN,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    Target {
        view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
        size,
    }
}

/// The offscreen textures for one window size, and the bind groups reading them.
struct Targets {
    scene: Target,
    blurred: Target,
    levels: Vec<Target>,
    down: Vec<wgpu::BindGroup>,
    up: Vec<wgpu::BindGroup>,
    composite_scene: wgpu::BindGroup,
    composite_blurred: wgpu::BindGroup,
}

/// What `prepare` worked out for `render`.
#[derive(Clone, Copy, Default)]
struct Frame {
    /// The scene pipeline to use (by shader id), if it built.
    scene: Option<u64>,
    steps: usize,
    /// The widget's bounds on the window, in physical pixels.
    bounds: [f32; 4],
}

pub struct Pipeline {
    cover_format: wgpu::TextureFormat,
    /// Scene pipelines by shader id (`None`: it didn't build), and whether this
    /// frame used them. The built-in one is always there.
    scene_pipelines: HashMap<u64, (Option<wgpu::RenderPipeline>, bool)>,
    down_pipeline: wgpu::RenderPipeline,
    up_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    scene_layout: wgpu::BindGroupLayout,
    passes: Passes,
    /// The picture in each place it's drawn, and whether this frame drew it.
    pictures: HashMap<Place, (Picture, bool)>,
    /// Uploaded covers by id, and whether this frame used them.
    covers: HashMap<u64, (wgpu::TextureView, bool)>,
    blank: wgpu::TextureView,
}

/// What the blur and composite passes share.
struct Passes {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

/// The buffers and textures behind one picture.
struct Picture {
    scene_uniforms: wgpu::Buffer,
    audio_uniforms: wgpu::Buffer,
    settings_uniforms: wgpu::Buffer,
    settings_size: u64,
    composite_uniforms: wgpu::Buffer,
    /// One per blur step: down passes first, then up passes.
    pass_uniforms: Vec<wgpu::Buffer>,
    targets: Option<Targets>,
    scene_bind: Option<wgpu::BindGroup>,
    frame: Frame,
}

fn uniform_buffer(device: &wgpu::Device, floats: usize, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (floats * 4) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn layout_entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    }
}

const UNIFORM: wgpu::BindingType = wgpu::BindingType::Buffer {
    ty: wgpu::BufferBindingType::Uniform,
    has_dynamic_offset: false,
    min_binding_size: None,
};
const TEXTURE: wgpu::BindingType = wgpu::BindingType::Texture {
    sample_type: wgpu::TextureSampleType::Float { filterable: true },
    view_dimension: wgpu::TextureViewDimension::D2,
    multisampled: false,
};
const SAMPLER: wgpu::BindingType = wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering);

#[allow(clippy::too_many_arguments)]
fn pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    module: &wgpu::ShaderModule,
    (vertex, fragment): (&str, &str),
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vertex),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = |label: &str, source: &'static str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
        };
        let kawase_module = module("undavi blur", include_str!("../assets/shaders/kawase.wgsl"));
        let composite_module = module(
            "undavi composite",
            include_str!("../assets/shaders/composite.wgsl"),
        );

        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("undavi scene"),
            entries: &[
                layout_entry(0, UNIFORM),
                layout_entry(1, TEXTURE),
                layout_entry(2, TEXTURE),
                layout_entry(3, SAMPLER),
                layout_entry(4, UNIFORM),
                layout_entry(5, UNIFORM),
            ],
        });
        // Blur and composite passes: uniforms, one texture, a sampler.
        let pass_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("undavi pass"),
            entries: &[
                layout_entry(0, UNIFORM),
                layout_entry(1, TEXTURE),
                layout_entry(2, SAMPLER),
            ],
        });

        // Covers match iced's own images: sRGB textures only on an sRGB surface,
        // so colors come out the same as everywhere else in the app.
        let cover_format = cover_format(format);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("undavi background"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // A 1×1 stand-in for an empty cover slot.
        let blank = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("undavi blank cover"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: cover_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            blank.as_image_copy(),
            &[0, 0, 0, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );

        Self {
            cover_format,
            scene_pipelines: HashMap::new(),
            down_pipeline: pipeline(
                device,
                "undavi blur down",
                &pass_layout,
                &kawase_module,
                ("vs", "down"),
                OFFSCREEN,
            ),
            up_pipeline: pipeline(
                device,
                "undavi blur up",
                &pass_layout,
                &kawase_module,
                ("vs", "up"),
                OFFSCREEN,
            ),
            composite_pipeline: pipeline(
                device,
                "undavi composite",
                &pass_layout,
                &composite_module,
                ("vs", "fs"),
                format,
            ),
            scene_layout,
            passes: Passes {
                layout: pass_layout,
                sampler,
            },
            pictures: HashMap::new(),
            covers: HashMap::new(),
            blank: blank.create_view(&wgpu::TextureViewDescriptor::default()),
        }
    }

    /// Let go of covers, shaders and pictures that weren't drawn this frame.
    fn trim(&mut self) {
        self.pictures.retain(|_, (_, used)| std::mem::take(used));
        self.covers.retain(|_, (_, used)| std::mem::take(used));
        self.scene_pipelines
            .retain(|_, (_, used)| std::mem::take(used));
    }
}

impl Pipeline {
    /// The scene pipeline for `shader`, built the first time it's seen. Shaders
    /// are checked before they get here, but any error the GPU still finds is
    /// caught (rather than ending the app); the background is left out then.
    fn scene_pipeline(&mut self, device: &wgpu::Device, shader: &Compiled) -> Option<u64> {
        let layout = &self.scene_layout;
        let entry = self.scene_pipelines.entry(shader.id).or_insert_with(|| {
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("undavi scene"),
                source: wgpu::ShaderSource::Wgsl(shader.source.as_ref().into()),
            });
            let built = pipeline(
                device,
                "undavi scene",
                layout,
                &module,
                ("undavi_vs", "undavi_fs"),
                OFFSCREEN,
            );
            match device.pop_error_scope().now_or_never().flatten() {
                None => (Some(built), true),
                Some(e) => {
                    eprintln!("undavi: background shader failed on the GPU: {e}");
                    (None, true)
                }
            }
        });
        entry.1 = true;
        entry.0.is_some().then_some(shader.id)
    }

    /// Upload `cover` the first time it's seen; mark it used either way.
    fn cover(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, cover: &Cover) {
        if let Some((_, used)) = self.covers.get_mut(&cover.id) {
            *used = true;
            return;
        }
        let view = upload_cover(device, queue, self.cover_format, cover);
        self.covers.insert(cover.id, (view, true));
    }
}

impl Passes {
    fn bind(
        &self,
        device: &wgpu::Device,
        uniforms: &wgpu::Buffer,
        src: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("undavi pass"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(src),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
}

impl Picture {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            scene_uniforms: uniform_buffer(device, SCENE_FLOATS, "undavi scene"),
            audio_uniforms: uniform_buffer(device, AUDIO_FLOATS, "undavi audio"),
            settings_uniforms: uniform_buffer(device, 4, "undavi background settings"),
            settings_size: 16,
            composite_uniforms: uniform_buffer(device, COMPOSITE_FLOATS, "undavi composite"),
            pass_uniforms: (0..2 * MAX_LEVELS)
                .map(|_| uniform_buffer(device, 4, "undavi blur pass"))
                .collect(),
            targets: None,
            scene_bind: None,
            frame: Frame::default(),
        }
    }

    /// Make the offscreen textures for a scene of `size` pixels, if they changed.
    fn resize(&mut self, device: &wgpu::Device, passes: &Passes, size: (u32, u32)) {
        if self.targets.as_ref().is_some_and(|t| t.scene.size == size) {
            return;
        }
        let scene = target(device, size, "undavi scene");
        let blurred = target(device, size, "undavi blurred");
        let mut levels = Vec::new();
        let mut s = size;
        for _ in 0..MAX_LEVELS {
            s = ((s.0 / 2).max(1), (s.1 / 2).max(1));
            levels.push(target(device, s, "undavi blur level"));
        }
        // Down pass i reads the scene (i = 0) or level i - 1; up pass j reads level j.
        let down = (0..MAX_LEVELS)
            .map(|i| {
                let src = if i == 0 {
                    &scene.view
                } else {
                    &levels[i - 1].view
                };
                passes.bind(device, &self.pass_uniforms[i], src)
            })
            .collect();
        let up = (0..MAX_LEVELS)
            .map(|j| passes.bind(device, &self.pass_uniforms[MAX_LEVELS + j], &levels[j].view))
            .collect();
        let composite_scene = passes.bind(device, &self.composite_uniforms, &scene.view);
        let composite_blurred = passes.bind(device, &self.composite_uniforms, &blurred.view);
        self.targets = Some(Targets {
            scene,
            blurred,
            levels,
            down,
            up,
            composite_scene,
            composite_blurred,
        });
    }
}

/// The texture format covers use with a target of `format`: sRGB only on an sRGB
/// surface, like iced's own images, so colors match the rest of the app.
pub fn cover_format(format: wgpu::TextureFormat) -> wgpu::TextureFormat {
    if format.is_srgb() {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    }
}

/// Put a cover on the GPU.
pub fn upload_cover(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    cover: &Cover,
) -> wgpu::TextureView {
    let (width, height) = cover.image.dimensions();
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("undavi cover"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        cover.image.as_raw(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: None,
        },
        size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let scene = pipeline.scene_pipeline(device, &self.shader);
        for cover in self.covers.iter().flatten() {
            pipeline.cover(device, queue, cover);
        }
        let entry = pipeline
            .pictures
            .entry(self.place)
            .or_insert_with(|| (Picture::new(device), true));
        entry.1 = true;
        let picture = &mut entry.0;

        let size = (self.settings.len() as u64).max(16);
        if size != picture.settings_size {
            picture.settings_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("undavi background settings"),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            picture.settings_size = size;
        }
        if !self.settings.is_empty() {
            queue.write_buffer(&picture.settings_uniforms, 0, &self.settings);
        }
        let physical = *bounds * viewport.scale_factor();
        // The preview is drawn smaller than it's laid out (the size the uniforms
        // have), so its pixels aren't the display's scale factor apart.
        let scale = physical.width / self.scene[0].max(1.0);
        let detail = match self.place {
            Place::Screen => SCENE_SCALE,
            Place::Preview => PREVIEW_SCALE,
        };
        let size = (
            ((physical.width * detail).round() as u32).max(1),
            ((physical.height * detail).round() as u32).max(1),
        );
        picture.resize(device, &pipeline.passes, size);

        let view = |slot: &Option<Cover>| {
            slot.as_ref()
                .and_then(|c| pipeline.covers.get(&c.id))
                .map_or(&pipeline.blank, |(v, _)| v)
        };
        let scene_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("undavi scene"),
            layout: &pipeline.scene_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: picture.scene_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view(&self.covers[0])),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(view(&self.covers[1])),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&pipeline.passes.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: picture.audio_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: picture.settings_uniforms.as_entire_binding(),
                },
            ],
        });
        picture.scene_bind = Some(scene_bind);

        queue.write_buffer(&picture.scene_uniforms, 0, &bytes(&self.scene));
        queue.write_buffer(&picture.audio_uniforms, 0, &bytes(&self.audio));
        queue.write_buffer(&picture.composite_uniforms, 0, &bytes(&self.composite));

        // The blur in scene pixels; each pass reads half a texel of its source.
        let (steps, offset) = kawase(self.blur * scale * detail);
        if let Some(targets) = &picture.targets {
            for i in 0..steps {
                let src = if i == 0 {
                    targets.scene.size
                } else {
                    targets.levels[i - 1].size
                };
                let params = [0.5 / src.0 as f32, 0.5 / src.1 as f32, offset, 0.0];
                queue.write_buffer(&picture.pass_uniforms[i], 0, &bytes(&params));
                let src = targets.levels[i].size;
                let params = [0.5 / src.0 as f32, 0.5 / src.1 as f32, offset, 0.0];
                queue.write_buffer(&picture.pass_uniforms[MAX_LEVELS + i], 0, &bytes(&params));
            }
        }
        picture.frame = Frame {
            scene,
            steps,
            bounds: [physical.x, physical.y, physical.width, physical.height],
        };
    }

    fn render(
        &self,
        pipeline: &Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let Some((picture, _)) = pipeline.pictures.get(&self.place) else {
            return;
        };
        let (Some(targets), Some(scene_bind)) = (&picture.targets, &picture.scene_bind) else {
            return;
        };
        let Some((Some(scene_pipeline), _)) = picture
            .frame
            .scene
            .and_then(|id| pipeline.scene_pipelines.get(&id))
        else {
            return;
        };
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    view: &wgpu::TextureView,
                    pipe: &wgpu::RenderPipeline,
                    bind: &wgpu::BindGroup| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("undavi background"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        };

        pass(encoder, &targets.scene.view, scene_pipeline, scene_bind);

        let steps = picture.frame.steps;
        for i in 0..steps {
            pass(
                encoder,
                &targets.levels[i].view,
                &pipeline.down_pipeline,
                &targets.down[i],
            );
        }
        for j in (0..steps).rev() {
            let dst = if j == 0 {
                &targets.blurred.view
            } else {
                &targets.levels[j - 1].view
            };
            pass(encoder, dst, &pipeline.up_pipeline, &targets.up[j]);
        }

        // Onto the window, inside the widget's bounds.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("undavi background composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let [x, y, w, h] = picture.frame.bounds;
        pass.set_viewport(x, y, w, h, 0.0, 1.0);
        pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );
        pass.set_pipeline(&pipeline.composite_pipeline);
        let source = if steps == 0 {
            &targets.composite_scene
        } else {
            &targets.composite_blurred
        };
        pass.set_bind_group(0, source, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kawase_reach_grows_smoothly() {
        assert_eq!(kawase(0.0), (0, 0.0));
        let reach = |r: f32| {
            let (steps, offset) = kawase(r);
            1.5 * (1u32 << steps) as f32 * offset
        };
        // The reach tracks the radius within each step…
        for r in [4.0, 10.0, 30.0, 60.0] {
            assert!((reach(r) - r).abs() / r < 0.05, "{r}: {}", reach(r));
        }
        // …and never shrinks as the radius grows.
        let mut last = 0.0;
        for i in 1..400 {
            let r = i as f32 * 0.5;
            assert!(reach(r) >= last - 1e-3, "{r}");
            last = reach(r);
        }
        assert!(kawase(1000.0).0 <= MAX_LEVELS);
    }
}
