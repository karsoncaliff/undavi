//! The visualizer on the GPU: a custom wgpu primitive drawn by iced's `shader`
//! widget, inside the stage. Every visualizer (Bars, Radial, Wave and custom
//! ones) is one full-area pass of its shader, blended over the background; see
//! `assets/shaders/visualizer.wgsl` for what it gets.

use crate::backdrop::{Cover, Place, cover_format, upload_cover};
use crate::shaders::Compiled;
use iced::Rectangle;
use iced::futures::FutureExt;
use iced::wgpu;
use iced::widget::shader::{self, Viewport};
use std::collections::HashMap;

/// Uniforms for `visualizer.wgsl`'s `UndaviVis`, in field order (70 × vec4).
pub const UNIFORM_FLOATS: usize = 4 * 70;
/// Where the pixel size (`size.w`) and the sRGB flag (`info.z`) sit; filled in
/// here, where they're known. (`size.x` is the logical width.)
const WIDTH: usize = 0;
const PIXEL_SIZE: usize = 3;
const SRGB_FLAG: usize = 6;

/// One frame of the visualizer.
#[derive(Debug)]
pub struct Primitive {
    pub place: Place,
    pub uniforms: Vec<f32>,
    /// The shader's `settings` uniform (`shaders::settings::Resolved::pack`).
    pub settings: Vec<u8>,
    pub cover: Option<Cover>,
    pub shader: Compiled,
}

fn bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_ne_bytes()).collect()
}

/// One picture: its buffers, and what `prepare` set up for `draw`.
struct Picture {
    uniforms: wgpu::Buffer,
    settings: wgpu::Buffer,
    settings_size: u64,
    shader: Option<u64>,
    bind: Option<wgpu::BindGroup>,
}

impl Picture {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            uniforms: uniform_buffer(device, (UNIFORM_FLOATS * 4) as u64, "undavi visualizer"),
            settings: uniform_buffer(device, 16, "undavi visualizer settings"),
            settings_size: 16,
            shader: None,
            bind: None,
        }
    }
}

pub struct Pipeline {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    /// Pipelines by shader id (`None`: it didn't build), and whether this frame
    /// used them.
    pipelines: HashMap<u64, (Option<wgpu::RenderPipeline>, bool)>,
    /// The picture in each place it's drawn, and whether this frame drew it.
    pictures: HashMap<Place, (Picture, bool)>,
    sampler: wgpu::Sampler,
    covers: HashMap<u64, (wgpu::TextureView, bool)>,
    blank: wgpu::TextureView,
}

fn uniform_buffer(device: &wgpu::Device, size: u64, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty,
            count: None,
        };
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("undavi visualizer"),
            entries: &[
                entry(0, uniform),
                entry(
                    1,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    2,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(3, uniform),
            ],
        });
        let blank = upload_cover(
            device,
            queue,
            cover_format(format),
            &Cover::new(image::RgbaImage::from_pixel(
                1,
                1,
                image::Rgba([0, 0, 0, 255]),
            )),
        );
        Self {
            format,
            layout,
            pipelines: HashMap::new(),
            pictures: HashMap::new(),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("undavi visualizer"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            covers: HashMap::new(),
            blank,
        }
    }

    fn trim(&mut self) {
        self.pictures.retain(|_, (_, used)| std::mem::take(used));
        self.covers.retain(|_, (_, used)| std::mem::take(used));
        self.pipelines.retain(|_, (_, used)| std::mem::take(used));
    }
}

impl Pipeline {
    /// The pipeline for `shader`, built the first time it's seen. Shaders are
    /// checked before they get here, but an error the GPU still finds is caught
    /// (not fatal); nothing is drawn then.
    fn pipeline(&mut self, device: &wgpu::Device, shader: &Compiled) -> Option<u64> {
        let (layout, format) = (&self.layout, self.format);
        let entry = self.pipelines.entry(shader.id).or_insert_with(|| {
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("undavi visualizer"),
                source: wgpu::ShaderSource::Wgsl(shader.source.as_ref().into()),
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("undavi visualizer"),
                bind_group_layouts: &[layout],
                push_constant_ranges: &[],
            });
            let built = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("undavi visualizer"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("undavi_vs_full"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("undavi_fs_custom"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::SrcAlpha,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
            match device.pop_error_scope().now_or_never().flatten() {
                None => (Some(built), true),
                Some(e) => {
                    eprintln!("undavi: visualizer shader failed on the GPU: {e}");
                    (None, true)
                }
            }
        });
        entry.1 = true;
        entry.0.is_some().then_some(shader.id)
    }
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
        let shader = pipeline.pipeline(device, &self.shader);
        let mut uniforms = self.uniforms.clone();
        uniforms.resize(UNIFORM_FLOATS, 0.0);
        // The preview is drawn smaller than it's laid out, so a screen pixel covers
        // more than the display's scale factor says.
        let physical = (bounds.width * viewport.scale_factor()).max(1.0);
        uniforms[PIXEL_SIZE] = uniforms[WIDTH] / physical;
        uniforms[SRGB_FLAG] = if pipeline.format.is_srgb() { 1.0 } else { 0.0 };
        let entry = pipeline
            .pictures
            .entry(self.place)
            .or_insert_with(|| (Picture::new(device), true));
        entry.1 = true;
        let picture = &mut entry.0;
        queue.write_buffer(&picture.uniforms, 0, &bytes(&uniforms));

        let size = (self.settings.len() as u64).max(16);
        if size != picture.settings_size {
            picture.settings = uniform_buffer(device, size, "undavi visualizer settings");
            picture.settings_size = size;
        }
        if !self.settings.is_empty() {
            queue.write_buffer(&picture.settings, 0, &self.settings);
        }

        if let Some(cover) = &self.cover {
            let format = cover_format(pipeline.format);
            pipeline
                .covers
                .entry(cover.id)
                .or_insert_with(|| (upload_cover(device, queue, format, cover), true))
                .1 = true;
        }
        let view = self
            .cover
            .as_ref()
            .and_then(|c| pipeline.covers.get(&c.id))
            .map_or(&pipeline.blank, |(v, _)| v);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("undavi visualizer"),
            layout: &pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: picture.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: picture.settings.as_entire_binding(),
                },
            ],
        });
        picture.shader = shader;
        picture.bind = Some(bind);
    }

    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        if let Some((picture, _)) = pipeline.pictures.get(&self.place)
            && let (Some(id), Some(bind)) = (picture.shader, &picture.bind)
            && let Some((Some(built), _)) = pipeline.pipelines.get(&id)
        {
            pass.set_bind_group(0, bind, &[]);
            pass.set_pipeline(built);
            pass.draw(0..3, 0..1);
        }
        true
    }
}
