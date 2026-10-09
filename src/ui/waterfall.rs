//! The waterfall: the sweep history drawn as a scrolling image.
//!
//! Replaces `WaterfallPlotWidget` and the `pg.ImageItem` inside it from
//! QSpectrumAnalyzer's `plot.py`.
//!
//! The history lives in one `R32Float` texture holding raw dB values, so a new
//! sweep uploads exactly one row and nothing else. Level windowing and the
//! colour-map lookup happen in the fragment shader, which is why dragging the
//! level sliders or switching colour map costs a 48-byte uniform write rather
//! than re-colouring the whole image. The ring buffer is never re-ordered: the
//! write head goes to the shader as a uniform and the wrap is resolved there.

use eframe::egui_wgpu;
use eframe::wgpu;

use crate::data::HistoryBuffer;

/// Texels per `bytes_per_row` alignment unit for an `R32Float` texture.
///
/// `queue.write_texture` tolerates an unaligned `bytes_per_row`, but
/// `copy_buffer_to_texture` does not and the WebGPU spec reserves the right to
/// require it, so the texture width is rounded up to a whole number of these
/// and every upload uses the padded stride. The padding columns are never read:
/// the shader rejects any column at or beyond the live column count.
const ALIGN_TEXELS: usize = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize / 4;

/// Height of the left-hand time-axis gutter's tick marks, in points.
const TICK_LEN: f32 = 5.0;

/// How the GUI wants the history mapped onto the widget this frame.
#[derive(Clone, Copy, Debug)]
pub struct WaterfallView {
    /// dB mapped to the bottom of the colour map.
    pub low: f32,
    /// dB mapped to the top of the colour map.
    pub high: f32,
    /// First and last bin centre, Hz.
    pub data_x: (f64, f64),
    /// Visible x range, Hz, taken from the linked spectrum plot.
    pub view_x: (f64, f64),
    /// Seconds per history row; `0` when no sweep has been timed yet.
    pub row_interval: f64,
}

// ---------------------------------------------------------------------------
// Pure mapping maths -- shared by the GPU shader and the CPU fallback
// ---------------------------------------------------------------------------

/// Colour-map index for one dB value.
///
/// A degenerate window (`high <= low`, or either end not finite) collapses to
/// the bottom of the map rather than dividing by zero, and NaN -- which a
/// backend can legitimately produce for a dropped bin -- does the same instead
/// of propagating into an out-of-range index.
fn level_index(v: f32, low: f32, high: f32) -> u8 {
    let span = high - low;
    let t = if span > 0.0 && span.is_finite() {
        (v - low) / span
    } else {
        0.0
    };
    if t.is_nan() {
        return 0;
    }
    (t.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The one value-to-colour step; the shader reimplements exactly this.
fn level_color(lut: &[[u8; 3]; 256], v: f32, low: f32, high: f32) -> [u8; 3] {
    lut[level_index(v, low, high) as usize]
}

/// `1 / (high - low)`, or `0` for a degenerate window, matching [`level_index`].
fn inv_span(low: f32, high: f32) -> f32 {
    let span = high - low;
    if span > 0.0 && span.is_finite() {
        1.0 / span
    } else {
        0.0
    }
}

/// Backing-store row holding the sweep shown at `screen_row`.
///
/// Screen row 0 is the newest sweep, which is the row just before the write
/// head; rows increase downward into the past, like the Qt image that scrolled
/// down from `y = 0`. `None` means there is no sweep that old yet, and the
/// caller must draw background instead of stale or zeroed data.
fn ring_row(head: usize, rows: usize, rows_filled: usize, screen_row: usize) -> Option<usize> {
    if rows == 0 || screen_row >= rows_filled.min(rows) {
        return None;
    }
    Some((head % rows + rows - 1 - screen_row) % rows)
}

/// Horizontal mapping from the widget's normalised x to data columns.
///
/// Returns `(column at uv.x == 0, columns per unit of uv.x)` in texel units,
/// where integer `n` is the centre of column `n`. `None` when nothing can be
/// drawn -- no columns, an empty view range, or a zero-width data range that
/// claims more than one column.
fn u_mapping(cols: usize, data_x: (f64, f64), view_x: (f64, f64)) -> Option<(f32, f32)> {
    if cols == 0 {
        return None;
    }
    let view_span = view_x.1 - view_x.0;
    if !(view_span > 0.0) || !view_span.is_finite() {
        return None;
    }
    if cols == 1 {
        // One column covers everything there is to show.
        return Some((0.0, 0.0));
    }

    let step = (data_x.1 - data_x.0) / (cols - 1) as f64;
    if step == 0.0 || !step.is_finite() {
        return None;
    }

    let at_left = (view_x.0 - data_x.0) / step;
    let per_uv = view_span / step;
    if !at_left.is_finite() || !per_uv.is_finite() {
        return None;
    }
    Some((at_left as f32, per_uv as f32))
}

/// Column sampled at normalised x `uv_x`, or `None` when outside the data.
fn column_at(at_left: f32, per_uv: f32, uv_x: f32, cols: usize) -> Option<usize> {
    let c = (at_left + uv_x * per_uv + 0.5).floor();
    if !c.is_finite() || c < 0.0 || c >= cols as f32 {
        return None;
    }
    Some(c as usize)
}

/// Texture width whose row stride is a whole number of alignment units.
fn padded_cols(cols: usize) -> usize {
    cols.max(1).div_ceil(ALIGN_TEXELS) * ALIGN_TEXELS
}

/// Source bin feeding stored column `j` when the two counts differ.
fn source_bin(j: usize, bins: usize, cols: usize) -> usize {
    if cols == 0 || bins == 0 {
        return 0;
    }
    if cols >= bins {
        return j.min(bins - 1);
    }
    (((j as u64) * (bins as u64)) / cols as u64).min(bins as u64 - 1) as usize
}

/// Lay one sweep out into `cols` stored columns.
///
/// Short rows are zero-filled and long ones truncated, so a backend that
/// changes its bin count mid-run cannot smear the previous sweep's tail across
/// the new one. When `cols < bins` -- only possible once `bins` exceeds the
/// device's maximum texture dimension -- columns are picked by nearest
/// neighbour, which keeps the full span visible rather than cropping it.
fn fill_columns(dst: &mut [f32], row: &[f32], bins: usize) {
    let cols = dst.len();
    if cols == 0 {
        return;
    }
    if cols == bins && row.len() == bins {
        dst.copy_from_slice(row);
        return;
    }
    for (j, out) in dst.iter_mut().enumerate() {
        let src = source_bin(j, bins, cols);
        *out = row.get(src).copied().unwrap_or(0.0);
    }
}

// ---------------------------------------------------------------------------
// Time axis
// ---------------------------------------------------------------------------

/// A readable tick spacing for a time axis covering `total_s` seconds.
///
/// Returns `0` when no axis can be drawn.
fn nice_time_step(total_s: f64, target_ticks: usize) -> f64 {
    const CANDIDATES: [f64; 20] = [
        0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0,
        3600.0, 7200.0, 21600.0, 43200.0, 86400.0,
    ];
    if !(total_s > 0.0) || !total_s.is_finite() || target_ticks == 0 {
        return 0.0;
    }
    let target = target_ticks as f64;
    for step in CANDIDATES {
        if total_s / step <= target {
            return step;
        }
    }
    total_s / target
}

/// `-12 s`, `-1:03`, `-1:02:03`: how long ago a waterfall row was swept.
fn format_age(seconds: f64, sub_second: bool) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0 s".to_owned();
    }
    if sub_second && seconds < 60.0 {
        return format!("-{seconds:.1} s");
    }
    let total = seconds.round() as u64;
    let (m, s) = (total / 60, total % 60);
    let (h, m) = (m / 60, m % 60);
    if h > 0 {
        format!("-{h}:{m:02}:{s:02}")
    } else if m > 0 {
        format!("-{m}:{s:02}")
    } else {
        format!("-{s} s")
    }
}

// ---------------------------------------------------------------------------
// GPU uniforms
// ---------------------------------------------------------------------------

/// Padded to 48 bytes so the WGSL struct's 16-byte alignment holds.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    /// Shown wherever there is no data; gamma space, premultiplied.
    background: [f32; 4],
    low: f32,
    inv_span: f32,
    at_left: f32,
    per_uv: f32,
    cols: u32,
    rows: u32,
    head: u32,
    rows_filled: u32,
}

fn uniforms(
    cols: usize,
    rows: usize,
    head: usize,
    rows_filled: usize,
    view: &WaterfallView,
    background: egui::Color32,
) -> Uniforms {
    let bg = background.to_array();
    let mapping = u_mapping(cols, view.data_x, view.view_x);
    let (at_left, per_uv) = mapping.unwrap_or((0.0, 0.0));

    Uniforms {
        background: [
            bg[0] as f32 / 255.0,
            bg[1] as f32 / 255.0,
            bg[2] as f32 / 255.0,
            bg[3] as f32 / 255.0,
        ],
        low: view.low,
        inv_span: inv_span(view.low, view.high),
        at_left,
        per_uv,
        // A failed mapping is reported as "no columns", which is the shader's
        // cue to fill the whole rect with background.
        cols: if mapping.is_some() { cols as u32 } else { 0 },
        rows: rows as u32,
        head: if rows == 0 { 0 } else { (head % rows) as u32 },
        rows_filled: rows_filled.min(rows) as u32,
    }
}

const SHADER: &str = r#"
struct Uniforms {
    background: vec4<f32>,
    low: f32,
    inv_span: f32,
    at_left: f32,
    per_uv: f32,
    cols: u32,
    rows: u32,
    head: u32,
    rows_filled: u32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var data_tex: texture_2d<f32>;
@group(0) @binding(2) var lut_tex: texture_2d<f32>;
@group(0) @binding(3) var lut_sampler: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) index: u32) -> VsOut {
    // One oversized triangle covers the viewport without a vertex buffer.
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0, -3.0),
        vec2<f32>( 3.0,  1.0),
    );
    let p = corners[index];
    var out: VsOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
    return out;
}

fn shade(uv: vec2<f32>) -> vec4<f32> {
    if u.cols == 0u || u.rows == 0u || u.rows_filled == 0u {
        return u.background;
    }

    let screen_row = i32(floor(uv.y * f32(u.rows)));
    if screen_row < 0 || screen_row >= i32(u.rows_filled) {
        return u.background;
    }

    let col = i32(floor(u.at_left + uv.x * u.per_uv + 0.5));
    if col < 0 || col >= i32(u.cols) {
        return u.background;
    }

    // Newest sweep on top: the write head points one past it. The operands are
    // arranged so the dividend can never go negative.
    let ring = (i32(u.head) + i32(u.rows) - 1 - screen_row) % i32(u.rows);

    let v = textureLoad(data_tex, vec2<i32>(col, ring), 0).r;
    var t = (v - u.low) * u.inv_span;
    if v != v {
        t = 0.0;
    }
    t = clamp(t, 0.0, 1.0);

    // Hit texel centres so t = 0 and t = 1 land exactly on the end colours.
    // textureSampleLevel, not textureSample: this runs in non-uniform control
    // flow because of the early returns above.
    let rgb = textureSampleLevel(
        lut_tex, lut_sampler, vec2<f32>((t * 255.0 + 0.5) / 256.0, 0.5), 0.0);
    return vec4<f32>(rgb.rgb, 1.0);
}

fn linear_from_gamma(srgb: vec3<f32>) -> vec3<f32> {
    let cutoff = srgb < vec3<f32>(0.04045);
    let lower = srgb / vec3<f32>(12.92);
    let higher = pow((srgb + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(higher, lower, cutoff);
}

@fragment
fn fs_gamma(in: VsOut) -> @location(0) vec4<f32> {
    return shade(in.uv);
}

@fragment
fn fs_linear(in: VsOut) -> @location(0) vec4<f32> {
    let c = shade(in.uv);
    return vec4<f32>(linear_from_gamma(c.rgb), c.a);
}
"#;

// ---------------------------------------------------------------------------
// GPU resources
// ---------------------------------------------------------------------------

/// The single entry this widget keeps in `CallbackResources`.
struct WaterfallResources {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct WaterfallCallback {
    uniforms: Uniforms,
}

impl egui_wgpu::CallbackTrait for WaterfallCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(r) = res.get::<WaterfallResources>() {
            queue.write_buffer(&r.uniform, 0, bytemuck::bytes_of(&self.uniforms));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        res: &egui_wgpu::CallbackResources,
    ) {
        if let Some(r) = res.get::<WaterfallResources>() {
            pass.set_pipeline(&r.pipeline);
            pass.set_bind_group(0, &r.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

/// Everything that outlives a geometry change.
struct Gpu {
    render: egui_wgpu::RenderState,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    lut: wgpu::Texture,
    lut_view: wgpu::TextureView,
    data: wgpu::Texture,
    data_view: wgpu::TextureView,
    /// Row stride of `data` in texels, always a multiple of [`ALIGN_TEXELS`].
    stride: usize,
    max_dim: usize,
}

impl Gpu {
    fn new(render: egui_wgpu::RenderState) -> Self {
        let device = &render.device;
        let max_dim = device.limits().max_texture_dimension_2d as usize;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("waterfall-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("waterfall-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        // R32Float is not filterable without an optional
                        // feature, hence textureLoad in the shader.
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("waterfall-pipeline-layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let target_format = render.target_format;
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("waterfall-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                // The oversized triangle's winding is irrelevant.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            // egui-wgpu builds its own pass with this many samples; eframe
            // leaves `NativeOptions::multisampling` at 0, i.e. one sample.
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                // An sRGB target converts on write, so hand it linear values;
                // a plain target wants the gamma-encoded ones. Same split
                // egui's own shader makes.
                entry_point: Some(if target_format.is_srgb() {
                    "fs_linear"
                } else {
                    "fs_gamma"
                }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("waterfall-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("waterfall-lut-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let lut = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("waterfall-lut"),
            size: wgpu::Extent3d {
                width: 256,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let lut_view = lut.create_view(&wgpu::TextureViewDescriptor::default());

        // A minimal data texture so the bind group is valid before the first
        // `reset`; `cols = 0` in the uniform keeps the shader off it.
        let (data, data_view, stride) = create_data_texture(device, 1, 1);

        let gpu = Self {
            render,
            layout,
            pipeline,
            uniform,
            sampler,
            lut,
            lut_view,
            data,
            data_view,
            stride,
            max_dim,
        };
        gpu.publish();
        gpu
    }

    /// Install (or replace) the one `CallbackResources` entry.
    fn publish(&self) {
        let bind_group = self
            .render
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("waterfall-bind-group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.data_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&self.lut_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });

        self.render
            .renderer
            .write()
            .callback_resources
            .insert(WaterfallResources {
                pipeline: self.pipeline.clone(),
                uniform: self.uniform.clone(),
                bind_group,
            });
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        let (data, data_view, stride) = create_data_texture(&self.render.device, cols, rows);
        self.data = data;
        self.data_view = data_view;
        self.stride = stride;
        self.publish();
    }

    fn write_lut(&self, lut: &[[u8; 3]; 256]) {
        let mut rgba = [0u8; 256 * 4];
        for (i, [r, g, b]) in lut.iter().enumerate() {
            rgba[i * 4..i * 4 + 4].copy_from_slice(&[*r, *g, *b, 255]);
        }
        self.render.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.lut,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256 * 4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 256,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Upload `rows` consecutive rows starting at `row`.
    ///
    /// `values` is laid out at the padded stride, so `bytes_per_row` is aligned
    /// whatever the column count.
    fn write_rows(&self, row: usize, rows: usize, cols: usize, values: &[f32]) {
        if rows == 0 || cols == 0 || values.is_empty() {
            return;
        }
        self.render.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.data,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: row as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(values),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some((self.stride * 4) as u32),
                rows_per_image: Some(rows as u32),
            },
            wgpu::Extent3d {
                width: cols as u32,
                height: rows as u32,
                depth_or_array_layers: 1,
            },
        );
    }
}

fn create_data_texture(
    device: &wgpu::Device,
    cols: usize,
    rows: usize,
) -> (wgpu::Texture, wgpu::TextureView, usize) {
    let stride = padded_cols(cols);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("waterfall-history"),
        size: wgpu::Extent3d {
            width: stride as u32,
            height: rows.max(1) as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view, stride)
}

// ---------------------------------------------------------------------------
// CPU fallback
// ---------------------------------------------------------------------------

/// Everything the rendered CPU image depends on. An unchanged key means the
/// previous upload is still valid.
#[derive(Clone, Copy, PartialEq)]
struct CpuKey {
    width: usize,
    height: usize,
    head: usize,
    rows_filled: usize,
    low: u32,
    high: u32,
    at_left: u32,
    per_uv: u32,
    background: [u8; 4],
}

#[derive(Default)]
struct Cpu {
    /// Mirror of the history, `rows` rows of `cols`, same ring layout.
    data: Vec<f32>,
    texture: Option<egui::TextureHandle>,
    dirty: bool,
    key: Option<CpuKey>,
}

// ---------------------------------------------------------------------------
// The widget
// ---------------------------------------------------------------------------

pub struct Waterfall {
    gpu: Option<Gpu>,
    /// Already reversed if the user asked for it, so both paths index it
    /// straight.
    lut: [[u8; 3]; 256],
    /// Bins the GUI reported.
    bins: usize,
    /// Columns actually stored; below `bins` only when the device cannot hold a
    /// texture that wide.
    cols: usize,
    rows: usize,
    /// Reused so a sweep never allocates.
    scratch: Vec<f32>,
    cpu: Cpu,
}

impl Waterfall {
    pub fn new(render_state: Option<egui_wgpu::RenderState>) -> Self {
        let lut = crate::colormap::LUTS[0];
        let gpu = render_state.map(Gpu::new);
        if let Some(g) = &gpu {
            g.write_lut(&lut);
        }
        Self {
            gpu,
            lut,
            bins: 0,
            cols: 0,
            rows: 0,
            scratch: Vec::new(),
            cpu: Cpu::default(),
        }
    }

    pub fn is_gpu(&self) -> bool {
        self.gpu.is_some()
    }

    /// Columns the widget stores, after clamping to what the device allows.
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// History rows the widget stores, after clamping to what the device
    /// allows. The GUI must not push a row index at or beyond this.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Re-shape for a new sweep geometry and drop the stored history.
    ///
    /// Nothing is cleared: the shader and the CPU path both mask everything at
    /// or beyond `rows_filled`, which the GUI resets along with this call.
    pub fn reset(&mut self, bins: usize, rows: usize) {
        let max_dim = self.gpu.as_ref().map_or(usize::MAX, |g| g.max_dim);
        let cols = bins.min(max_dim);
        let rows = rows.min(max_dim);

        if cols < bins {
            log::warn!(
                "waterfall: {bins} bins exceed the maximum texture width of {max_dim}; \
                 showing {cols} resampled columns"
            );
        }

        let geometry_changed = cols != self.cols || rows != self.rows;
        self.bins = bins;
        self.cols = cols;
        self.rows = rows;
        self.scratch.clear();
        self.scratch.resize(cols, 0.0);

        if let Some(g) = &mut self.gpu {
            if geometry_changed {
                g.resize(cols, rows);
            }
        } else {
            self.cpu.data.clear();
            self.cpu.data.resize(cols * rows, 0.0);
            self.cpu.dirty = true;
        }
    }

    /// Upload one sweep. This is the hot path: `O(bins)` and nothing more.
    pub fn push_row(&mut self, row_index: usize, row: &[f32]) {
        if self.cols == 0 || row_index >= self.rows {
            return;
        }

        if let Some(g) = &self.gpu {
            if self.cols == self.bins && row.len() == self.bins {
                // Exact match: upload straight out of the caller's buffer.
                g.write_rows(row_index, 1, self.cols, row);
            } else {
                fill_columns(&mut self.scratch, row, self.bins);
                g.write_rows(row_index, 1, self.cols, &self.scratch);
            }
        } else {
            let start = row_index * self.cols;
            let Some(dst) = self.cpu.data.get_mut(start..start + self.cols) else {
                return;
            };
            fill_columns(dst, row, self.bins);
            self.cpu.dirty = true;
        }
    }

    /// Rebuild the whole image from `history`.
    ///
    /// Only for a full refresh -- a baseline re-level or a geometry change --
    /// never for a new sweep.
    pub fn upload_history(&mut self, history: &HistoryBuffer) {
        if history.bins() != self.bins || history.capacity() != self.rows {
            self.reset(history.bins(), history.capacity());
        }
        if self.cols == 0 || self.rows == 0 {
            return;
        }

        let bins = self.bins;
        let cols = self.cols;
        let raw = history.raw();

        if let Some(g) = &self.gpu {
            // One aligned upload for the whole texture; the padding columns are
            // left as zero and never sampled.
            let stride = g.stride;
            let mut staged = vec![0.0f32; stride * self.rows];
            for r in 0..self.rows {
                let src = r * bins;
                let row = raw.get(src..src + bins).unwrap_or(&[]);
                let dst = r * stride;
                if let Some(out) = staged.get_mut(dst..dst + cols) {
                    fill_columns(out, row, bins);
                }
            }
            g.write_rows(0, self.rows, cols, &staged);
        } else {
            self.cpu.data.clear();
            self.cpu.data.resize(cols * self.rows, 0.0);
            for r in 0..self.rows {
                let src = r * bins;
                let row = raw.get(src..src + bins).unwrap_or(&[]);
                let dst = r * cols;
                if let Some(out) = self.cpu.data.get_mut(dst..dst + cols) {
                    fill_columns(out, row, bins);
                }
            }
            self.cpu.dirty = true;
        }
    }

    pub fn set_colormap(&mut self, lut: &[[u8; 3]; 256], reverse: bool) {
        for (i, dst) in self.lut.iter_mut().enumerate() {
            *dst = lut[if reverse { 255 - i } else { i }];
        }
        if let Some(g) = &self.gpu {
            g.write_lut(&self.lut);
        } else {
            self.cpu.dirty = true;
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        head: usize,
        rows_filled: usize,
        view: &WaterfallView,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return response;
        }

        let background = ui.visuals().extreme_bg_color;
        if self.gpu.is_some() {
            let u = uniforms(self.cols, self.rows, head, rows_filled, view, background);
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                WaterfallCallback { uniforms: u },
            ));
        } else {
            self.paint_cpu(ui, rect, head, rows_filled, view, background);
        }

        self.draw_time_axis(ui, rect, view.row_interval);
        response
    }

    // -- CPU fallback -------------------------------------------------------

    fn paint_cpu(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        head: usize,
        rows_filled: usize,
        view: &WaterfallView,
        background: egui::Color32,
    ) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, background);

        let mapping = u_mapping(self.cols, view.data_x, view.view_x);
        let Some((at_left, per_uv)) = mapping else {
            return;
        };
        let rows_filled = rows_filled.min(self.rows);
        if rows_filled == 0 {
            return;
        }

        // One image pixel per screen pixel horizontally, one per history row
        // vertically, so the result matches what the shader would draw.
        let ppp = ui.ctx().pixels_per_point().max(0.1);
        let width = ((rect.width() * ppp).round() as usize).clamp(1, 4096);
        let height = self.rows.clamp(1, 2048);

        let key = CpuKey {
            width,
            height,
            head,
            rows_filled,
            low: view.low.to_bits(),
            high: view.high.to_bits(),
            at_left: at_left.to_bits(),
            per_uv: per_uv.to_bits(),
            background: background.to_array(),
        };

        if self.cpu.dirty || self.cpu.key != Some(key) {
            let image = self.cpu_image(width, height, head, rows_filled, view, background);
            match &mut self.cpu.texture {
                Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
                None => {
                    self.cpu.texture = Some(ui.ctx().load_texture(
                        "spectroscope-waterfall",
                        image,
                        egui::TextureOptions::NEAREST,
                    ))
                }
            }
            self.cpu.dirty = false;
            self.cpu.key = Some(key);
        }

        if let Some(tex) = &self.cpu.texture {
            painter.image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    fn cpu_image(
        &self,
        width: usize,
        height: usize,
        head: usize,
        rows_filled: usize,
        view: &WaterfallView,
        background: egui::Color32,
    ) -> egui::ColorImage {
        let Some((at_left, per_uv)) = u_mapping(self.cols, view.data_x, view.view_x) else {
            return egui::ColorImage::filled([width, height], background);
        };

        // The column per screen pixel is the same for every row.
        let columns: Vec<Option<usize>> = (0..width)
            .map(|x| {
                let uv_x = (x as f32 + 0.5) / width as f32;
                column_at(at_left, per_uv, uv_x, self.cols)
            })
            .collect();

        let mut pixels = vec![background; width * height];
        for y in 0..height {
            // `height` is capped, so several history rows can share a line.
            let screen_row = y * self.rows / height;
            let Some(ring) = ring_row(head, self.rows, rows_filled, screen_row) else {
                continue;
            };
            let base = ring * self.cols;
            let Some(src) = self.cpu.data.get(base..base + self.cols) else {
                continue;
            };
            for x in 0..width {
                let Some(col) = columns[x] else { continue };
                let [r, g, b] = level_color(&self.lut, src[col], view.low, view.high);
                pixels[y * width + x] = egui::Color32::from_rgb(r, g, b);
            }
        }

        egui::ColorImage::new([width, height], pixels)
    }

    // -- Time axis ----------------------------------------------------------

    /// Seconds into the past down the left edge. The frequency axis belongs to
    /// the spectrum plot above, so nothing else is drawn.
    fn draw_time_axis(&self, ui: &egui::Ui, rect: egui::Rect, row_interval: f64) {
        if self.rows == 0 || !(row_interval > 0.0) || !row_interval.is_finite() {
            return;
        }
        let total = self.rows as f64 * row_interval;
        let step = nice_time_step(total, 6);
        if step <= 0.0 {
            return;
        }

        let painter = ui.painter_at(rect);
        let color = ui.visuals().weak_text_color();
        let font = egui::FontId::proportional(10.0);
        let stroke = egui::Stroke::new(1.0, color);

        let mut t = step;
        while t <= total {
            let y = rect.top() + (t / total) as f32 * rect.height();
            painter.line_segment(
                [
                    egui::pos2(rect.left(), y),
                    egui::pos2(rect.left() + TICK_LEN, y),
                ],
                stroke,
            );

            let galley = painter.layout_no_wrap(format_age(t, step < 1.0), font.clone(), color);
            let pos = egui::pos2(rect.left() + TICK_LEN + 3.0, y - galley.size().y * 0.5);
            // The label sits over the image, so give it something to read on.
            painter.rect_filled(
                egui::Rect::from_min_size(pos, galley.size()).expand(2.0),
                2.0,
                egui::Color32::from_black_alpha(140),
            );
            painter.galley(pos, galley, color);

            t += step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREY: [[u8; 3]; 256] = {
        let mut lut = [[0u8; 3]; 256];
        let mut i = 0;
        while i < 256 {
            lut[i] = [i as u8, i as u8, i as u8];
            i += 1;
        }
        lut
    };

    // -- value to colour ----------------------------------------------------

    #[test]
    fn level_windowing_spans_the_map() {
        assert_eq!(level_index(-100.0, -100.0, -20.0), 0);
        assert_eq!(level_index(-20.0, -100.0, -20.0), 255);
        assert_eq!(level_index(-60.0, -100.0, -20.0), 128);
    }

    #[test]
    fn level_windowing_clamps_at_both_ends() {
        assert_eq!(level_index(-500.0, -100.0, -20.0), 0);
        assert_eq!(level_index(1000.0, -100.0, -20.0), 255);
        assert_eq!(level_index(f32::NEG_INFINITY, -100.0, -20.0), 0);
        assert_eq!(level_index(f32::INFINITY, -100.0, -20.0), 255);
    }

    #[test]
    fn degenerate_window_does_not_divide_by_zero() {
        assert_eq!(level_index(0.0, -50.0, -50.0), 0);
        assert_eq!(level_index(0.0, -20.0, -100.0), 0);
        assert_eq!(level_index(0.0, f32::NAN, 0.0), 0);
        assert_eq!(inv_span(-50.0, -50.0), 0.0);
        assert_eq!(inv_span(-100.0, -20.0), 1.0 / 80.0);
    }

    #[test]
    fn nan_maps_to_the_bottom_without_panicking() {
        assert_eq!(level_index(f32::NAN, -100.0, -20.0), 0);
        assert_eq!(level_color(&GREY, f32::NAN, -100.0, -20.0), [0, 0, 0]);
    }

    #[test]
    fn reverse_flips_the_table() {
        let mut w = Waterfall {
            gpu: None,
            lut: [[0; 3]; 256],
            bins: 0,
            cols: 0,
            rows: 0,
            scratch: Vec::new(),
            cpu: Cpu::default(),
        };

        w.set_colormap(&GREY, false);
        assert_eq!(level_color(&w.lut, -20.0, -100.0, -20.0), [255; 3]);
        assert_eq!(level_color(&w.lut, -100.0, -100.0, -20.0), [0; 3]);

        w.set_colormap(&GREY, true);
        assert_eq!(level_color(&w.lut, -20.0, -100.0, -20.0), [0; 3]);
        assert_eq!(level_color(&w.lut, -100.0, -100.0, -20.0), [255; 3]);
    }

    // -- ring arithmetic ----------------------------------------------------

    #[test]
    fn full_ring_puts_the_newest_sweep_on_top() {
        // head = 3 means row 2 was written last.
        assert_eq!(ring_row(3, 4, 4, 0), Some(2));
        assert_eq!(ring_row(3, 4, 4, 1), Some(1));
        assert_eq!(ring_row(3, 4, 4, 2), Some(0));
        assert_eq!(ring_row(3, 4, 4, 3), Some(3));
        assert_eq!(ring_row(3, 4, 4, 4), None);
    }

    #[test]
    fn wrapped_head_is_handled() {
        assert_eq!(ring_row(0, 4, 4, 0), Some(3));
        assert_eq!(ring_row(0, 4, 4, 1), Some(2));
        // A head that has not been reduced must not change the answer.
        assert_eq!(ring_row(4, 4, 4, 0), ring_row(0, 4, 4, 0));
    }

    #[test]
    fn partly_filled_ring_stops_at_the_last_real_sweep() {
        assert_eq!(ring_row(2, 8, 2, 0), Some(1));
        assert_eq!(ring_row(2, 8, 2, 1), Some(0));
        assert_eq!(ring_row(2, 8, 2, 2), None);
        assert_eq!(ring_row(0, 8, 0, 0), None);
    }

    #[test]
    fn empty_geometry_draws_nothing() {
        assert_eq!(ring_row(0, 0, 0, 0), None);
        assert_eq!(ring_row(0, 0, 5, 0), None);
    }

    #[test]
    fn ring_row_agrees_with_the_history_buffer() {
        let mut h = HistoryBuffer::new(1, 5);
        for i in 0..8 {
            h.append(&[i as f32]);
        }
        for age in 0..h.len() + 2 {
            assert_eq!(
                ring_row(h.head(), h.capacity(), h.len(), age),
                h.row_index_from_newest(age),
                "age {age}"
            );
        }
    }

    // -- horizontal mapping -------------------------------------------------

    #[test]
    fn unzoomed_view_spans_every_column() {
        let (at_left, per_uv) = u_mapping(101, (100e6, 200e6), (100e6, 200e6)).expect("mapping");
        assert!((at_left - 0.0).abs() < 1e-3);
        assert!((per_uv - 100.0).abs() < 1e-3);
        assert_eq!(column_at(at_left, per_uv, 0.0, 101), Some(0));
        assert_eq!(column_at(at_left, per_uv, 1.0, 101), Some(100));
        assert_eq!(column_at(at_left, per_uv, 0.5, 101), Some(50));
    }

    #[test]
    fn zoomed_view_selects_the_middle_columns() {
        // 101 bins over 100 MHz: 1 MHz per bin. Zoom to the middle 10 MHz.
        let (at_left, per_uv) = u_mapping(101, (100e6, 200e6), (145e6, 155e6)).expect("mapping");
        assert!((at_left - 45.0).abs() < 1e-3);
        assert!((per_uv - 10.0).abs() < 1e-3);
        assert_eq!(column_at(at_left, per_uv, 0.0, 101), Some(45));
        assert_eq!(column_at(at_left, per_uv, 1.0, 101), Some(55));
    }

    #[test]
    fn view_wider_than_the_data_leaves_the_edges_empty() {
        let (at_left, per_uv) = u_mapping(11, (100e6, 110e6), (90e6, 120e6)).expect("mapping");
        assert_eq!(column_at(at_left, per_uv, 0.0, 11), None);
        assert_eq!(column_at(at_left, per_uv, 1.0, 11), None);
        assert_eq!(column_at(at_left, per_uv, 0.5, 11), Some(5));
    }

    #[test]
    fn degenerate_mapping_is_rejected() {
        assert!(u_mapping(0, (1.0, 2.0), (1.0, 2.0)).is_none());
        // Zero-width view.
        assert!(u_mapping(10, (1.0, 2.0), (1.5, 1.5)).is_none());
        // Inverted view.
        assert!(u_mapping(10, (1.0, 2.0), (2.0, 1.0)).is_none());
        // Zero-width data with more than one column.
        assert!(u_mapping(10, (1.0, 1.0), (1.0, 2.0)).is_none());
        assert!(u_mapping(10, (f64::NAN, 1.0), (1.0, 2.0)).is_none());
        // One column covers the whole widget.
        assert_eq!(u_mapping(1, (1.0, 1.0), (0.0, 2.0)), Some((0.0, 0.0)));
    }

    // -- upload alignment ---------------------------------------------------

    #[test]
    fn padded_stride_is_always_aligned() {
        for cols in [0usize, 1, 63, 64, 65, 100, 1000, 2048, 4096] {
            let stride = padded_cols(cols);
            assert!(stride >= cols.max(1), "cols {cols}");
            assert_eq!(stride * 4 % 256, 0, "cols {cols}");
            assert!(stride - cols.max(1) < ALIGN_TEXELS, "cols {cols}");
        }
    }

    #[test]
    fn padded_stride_known_values() {
        assert_eq!(ALIGN_TEXELS, 64);
        assert_eq!(padded_cols(0), 64);
        assert_eq!(padded_cols(1), 64);
        assert_eq!(padded_cols(64), 64);
        assert_eq!(padded_cols(65), 128);
        assert_eq!(padded_cols(2100), 2112);
    }

    // -- row layout ---------------------------------------------------------

    #[test]
    fn exact_row_is_copied_verbatim() {
        let mut dst = [0.0f32; 4];
        fill_columns(&mut dst, &[1.0, 2.0, 3.0, 4.0], 4);
        assert_eq!(dst, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn truncated_row_is_zero_filled() {
        let mut dst = [9.0f32; 4];
        fill_columns(&mut dst, &[1.0, 2.0], 4);
        assert_eq!(dst, [1.0, 2.0, 0.0, 0.0]);
    }

    #[test]
    fn overlong_row_is_truncated() {
        let mut dst = [0.0f32; 3];
        fill_columns(&mut dst, &[1.0, 2.0, 3.0, 4.0, 5.0], 3);
        assert_eq!(dst, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn empty_row_and_empty_destination_are_harmless() {
        let mut dst = [7.0f32; 2];
        fill_columns(&mut dst, &[], 2);
        assert_eq!(dst, [0.0, 0.0]);
        fill_columns(&mut [], &[1.0], 1);
    }

    #[test]
    fn decimation_keeps_both_ends_of_the_span() {
        let row: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        let mut dst = [0.0f32; 100];
        fill_columns(&mut dst, &row, 1000);
        assert_eq!(dst[0], 0.0);
        assert_eq!(dst[99], 990.0);
        assert!(dst.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn source_bin_stays_in_range() {
        for cols in [1usize, 7, 64, 1000] {
            for j in 0..cols {
                assert!(source_bin(j, 500, cols) < 500, "cols {cols} j {j}");
            }
        }
        assert_eq!(source_bin(0, 0, 4), 0);
        assert_eq!(source_bin(3, 4, 0), 0);
    }

    // -- uniforms -----------------------------------------------------------

    #[test]
    fn uniform_block_respects_wgsl_alignment() {
        assert_eq!(std::mem::size_of::<Uniforms>() % 16, 0);
        assert_eq!(std::mem::size_of::<Uniforms>(), 48);
        assert_eq!(std::mem::align_of::<Uniforms>(), 4);
    }

    fn view() -> WaterfallView {
        WaterfallView {
            low: -100.0,
            high: -20.0,
            data_x: (100e6, 200e6),
            view_x: (100e6, 200e6),
            row_interval: 1.0,
        }
    }

    #[test]
    fn uniforms_reduce_the_head_and_clamp_the_fill() {
        let u = uniforms(101, 8, 11, 99, &view(), egui::Color32::BLACK);
        assert_eq!(u.head, 3);
        assert_eq!(u.rows_filled, 8);
        assert_eq!(u.cols, 101);
        assert_eq!(u.inv_span, 1.0 / 80.0);
    }

    #[test]
    fn unmappable_view_reports_no_columns() {
        let mut v = view();
        v.view_x = (150e6, 150e6);
        assert_eq!(uniforms(101, 8, 0, 8, &v, egui::Color32::BLACK).cols, 0);

        // Zero rows must not divide by zero while reducing the head.
        let u = uniforms(101, 0, 7, 7, &view(), egui::Color32::BLACK);
        assert_eq!(u.head, 0);
        assert_eq!(u.rows_filled, 0);
    }

    // -- time axis ----------------------------------------------------------

    #[test]
    fn tick_step_keeps_the_label_count_sane() {
        assert_eq!(nice_time_step(100.0, 6), 30.0);
        assert_eq!(nice_time_step(10.0, 6), 2.0);
        assert_eq!(nice_time_step(6.0, 6), 1.0);
        assert_eq!(nice_time_step(3.0, 6), 0.5);
        assert_eq!(nice_time_step(3600.0, 6), 600.0);
        for total in [0.5, 7.0, 93.0, 1234.0, 99_999.0] {
            let step = nice_time_step(total, 6);
            assert!(step > 0.0 && total / step <= 6.0 + 1e-9, "total {total}");
        }
    }

    #[test]
    fn tick_step_rejects_a_zero_span() {
        assert_eq!(nice_time_step(0.0, 6), 0.0);
        assert_eq!(nice_time_step(-1.0, 6), 0.0);
        assert_eq!(nice_time_step(f64::NAN, 6), 0.0);
        assert_eq!(nice_time_step(100.0, 0), 0.0);
        // Beyond the largest candidate it falls back to an even division.
        assert!((nice_time_step(1e7, 5) - 2e6).abs() < 1.0);
    }

    #[test]
    fn age_labels_read_as_time_into_the_past() {
        assert_eq!(format_age(0.5, true), "-0.5 s");
        assert_eq!(format_age(12.0, false), "-12 s");
        assert_eq!(format_age(63.0, false), "-1:03");
        assert_eq!(format_age(3723.0, false), "-1:02:03");
        assert_eq!(format_age(f64::NAN, false), "0 s");
        assert_eq!(format_age(-1.0, false), "0 s");
    }

    // -- CPU fallback -------------------------------------------------------

    /// A `Waterfall` with no render state, filled through the public API.
    fn cpu_waterfall(bins: usize, rows: usize) -> Waterfall {
        let mut w = Waterfall::new(None);
        assert!(!w.is_gpu());
        w.set_colormap(&GREY, false);
        w.reset(bins, rows);
        w
    }

    #[test]
    fn cpu_image_puts_the_newest_sweep_on_the_top_line() {
        let mut w = cpu_waterfall(4, 3);
        w.push_row(0, &[-100.0; 4]);
        w.push_row(1, &[-60.0; 4]);
        w.push_row(2, &[-20.0; 4]);

        // head = 0 after three appends into a three-row ring.
        let img = w.cpu_image(4, 3, 0, 3, &view(), egui::Color32::BLACK);
        assert_eq!(img.size, [4, 3]);
        assert_eq!(img.pixels[0], egui::Color32::from_rgb(255, 255, 255));
        assert_eq!(img.pixels[4], egui::Color32::from_rgb(128, 128, 128));
        assert_eq!(img.pixels[8], egui::Color32::from_rgb(0, 0, 0));
    }

    #[test]
    fn cpu_image_leaves_unfilled_rows_as_background() {
        let mut w = cpu_waterfall(2, 4);
        w.push_row(0, &[-20.0, -20.0]);

        let bg = egui::Color32::from_rgb(1, 2, 3);
        let img = w.cpu_image(2, 4, 1, 1, &view(), bg);
        assert_eq!(img.pixels[0], egui::Color32::from_rgb(255, 255, 255));
        for p in &img.pixels[2..] {
            assert_eq!(*p, bg, "stale row leaked into the image");
        }
    }

    #[test]
    fn cpu_image_leaves_columns_outside_the_data_as_background() {
        let mut w = cpu_waterfall(3, 1);
        w.push_row(0, &[-20.0, -20.0, -20.0]);

        let mut v = view();
        v.data_x = (100e6, 102e6);
        v.view_x = (98e6, 104e6);
        let bg = egui::Color32::from_rgb(1, 2, 3);
        let img = w.cpu_image(6, 1, 1, 1, &v, bg);
        assert_eq!(img.pixels[0], bg);
        assert_eq!(img.pixels[5], bg);
        assert!(img.pixels[2..4].iter().all(|p| *p != bg));
    }

    #[test]
    fn cpu_image_survives_nan_and_a_zero_window() {
        let mut w = cpu_waterfall(2, 1);
        w.push_row(0, &[f32::NAN, f32::NAN]);

        let mut v = view();
        v.low = -50.0;
        v.high = -50.0;
        let img = w.cpu_image(2, 1, 1, 1, &v, egui::Color32::BLACK);
        assert_eq!(img.pixels[0], egui::Color32::from_rgb(0, 0, 0));
    }

    #[test]
    fn reset_and_push_tolerate_nonsense() {
        let mut w = cpu_waterfall(0, 0);
        // Nothing to write into: must not panic.
        w.push_row(0, &[1.0]);
        w.push_row(99, &[1.0]);
        assert_eq!(w.cols(), 0);

        w.reset(4, 2);
        // Out-of-range row index is dropped.
        w.push_row(5, &[1.0, 2.0, 3.0, 4.0]);
        assert!(w.cpu.data.iter().all(|v| *v == 0.0));
        assert_eq!(w.rows(), 2);
    }

    #[test]
    fn upload_history_reshapes_to_match() {
        let mut w = cpu_waterfall(2, 2);
        let mut h = HistoryBuffer::new(3, 5);
        h.append(&[-20.0, -60.0, -100.0]);

        w.upload_history(&h);
        assert_eq!(w.cols(), 3);
        assert_eq!(w.rows(), 5);

        let img = w.cpu_image(3, 5, h.head(), h.len(), &view(), egui::Color32::BLACK);
        assert_eq!(img.pixels[0], egui::Color32::from_rgb(255, 255, 255));
        assert_eq!(img.pixels[1], egui::Color32::from_rgb(128, 128, 128));
        assert_eq!(img.pixels[2], egui::Color32::from_rgb(0, 0, 0));
    }

    #[test]
    fn upload_history_of_an_empty_buffer_is_harmless() {
        let mut w = cpu_waterfall(4, 4);
        let h = HistoryBuffer::default();
        w.upload_history(&h);
        assert_eq!(w.cols(), 0);
    }
}
