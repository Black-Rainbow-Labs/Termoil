//! The custom shader pass: a full screen pass that runs a user's WGSL fragment function over the
//! frame vello drew. `docs/shaders.md` states the contract the user's file follows.

use std::time::Instant;

use crate::RenderError;

/// The name of the function the user's file must define.
pub const ENTRY_NAME: &str = "terminus_shade";

/// Bytes of the uniform buffer: size (8), cursor (8), time (4) and 12 of padding.
const UNIFORM_BYTES: u64 = 32;

/// What Terminus adds to the user's file. WGSL module scope declarations can come in any order, so
/// the wrapper follows the user's code and error messages keep the line numbers of the user's file.
const WRAPPER: &str = r#"

struct TerminusUniforms {
    size: vec2<f32>,
    cursor: vec2<f32>,
    time: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
}

@group(0) @binding(0) var terminus_frame: texture_2d<f32>;
@group(0) @binding(1) var terminus_sampler: sampler;
@group(0) @binding(2) var<uniform> terminus: TerminusUniforms;

struct TerminusVertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn terminus_vertex(@builtin(vertex_index) index: u32) -> TerminusVertexOut {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: TerminusVertexOut;
    out.position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

@fragment
fn terminus_fragment(in: TerminusVertexOut) -> @location(0) vec4<f32> {
    let color = textureSampleLevel(terminus_frame, terminus_sampler, in.uv, 0.0);
    return terminus_shade(in.uv, color);
}
"#;

/// The values a shader reads each frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShaderInputs {
    /// Frame size in pixels.
    pub size: (f32, f32),
    /// Cursor position in pixels, negative when no cursor is shown.
    pub cursor: (f32, f32),
}

/// A compiled shader pass.
pub struct ShaderPass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    started: Instant,
    animated: bool,
}

/// True when the source reads `terminus.time`, so the window must redraw at the display rate.
pub fn reads_time(source: &str) -> bool {
    source.lines().map(|l| l.split("//").next().unwrap_or("")).any(|l| l.contains("terminus.time"))
}

/// The full module text: the user's source followed by the wrapper.
fn module_text(source: &str) -> String {
    format!("{source}{WRAPPER}")
}

/// Runs `build` with a validation error scope and returns the first error as text.
fn scoped<T>(device: &wgpu::Device, what: &str, build: impl FnOnce() -> T) -> Result<T, RenderError> {
    let guard = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = build();
    match pollster::block_on(guard.pop()) {
        Some(e) => Err(RenderError::Gpu(format!("{what}: {e}"))),
        None => Ok(value),
    }
}

/// The bind group layout: the frame texture, a sampler and the uniform buffer.
fn bind_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let entry =
        |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("terminus shader"),
        entries: &[
            entry(
                0,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ),
            entry(1, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            entry(
                2,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                },
            ),
        ],
    })
}

/// Builds the pipeline from a compiled module.
fn build_pipeline(
    device: &wgpu::Device, module: &wgpu::ShaderModule, layout: &wgpu::BindGroupLayout, format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("terminus shader"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("terminus shader"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("terminus_vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("terminus_fragment"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

impl ShaderPass {
    /// Compiles the user's WGSL `source` into a pass that writes `format`. A validation error comes
    /// back as an error value, with line numbers of the user's file, and nothing is drawn with it.
    pub fn compile(
        device: &wgpu::Device, format: wgpu::TextureFormat, source: &str,
    ) -> Result<ShaderPass, RenderError> {
        if !source.contains(ENTRY_NAME) {
            return Err(RenderError::Gpu(format!(
                "the shader must define fn {ENTRY_NAME}(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32>"
            )));
        }
        let text = module_text(source);
        let module = scoped(device, "shader", || {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("terminus shader"),
                source: wgpu::ShaderSource::Wgsl(text.into()),
            })
        })?;
        let layout = bind_layout(device);
        let pipeline = scoped(device, "shader pipeline", || build_pipeline(device, &module, &layout, format))?;
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("terminus shader"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terminus shader"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(ShaderPass { pipeline, layout, sampler, uniforms, started: Instant::now(), animated: reads_time(source) })
    }

    /// True when the shader reads the time.
    pub fn animated(&self) -> bool {
        self.animated
    }

    /// Writes the uniforms, then draws the pass sampling `frame` into `out`.
    pub fn run(
        &self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::TextureView, out: &wgpu::TextureView, inputs: ShaderInputs,
    ) {
        let time = self.started.elapsed().as_secs_f32();
        let values = [inputs.size.0, inputs.size.1, inputs.cursor.0, inputs.cursor.1, time, 0.0, 0.0, 0.0];
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        queue.write_buffer(&self.uniforms, 0, &bytes);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terminus shader"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(frame) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: self.uniforms.as_entire_binding() },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("terminus shader"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: out,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Creates the texture vello draws into when a shader pass follows.
pub fn frame_texture(device: &wgpu::Device, format: wgpu::TextureFormat, w: u16, h: u16) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terminus shader input"),
        size: wgpu::Extent3d { width: w.into(), height: h.into(), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}
