// src/render/pipeline.rs
//! wgpu render pipeline for terminal cells (backgrounds and glyph quads).

use bytemuck::{Pod, Zeroable};
use crate::render::atlas::GlyphAtlas;
use crate::term::grid::{flags, Color, Grid};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Uniforms {
    pub screen_size: [f32; 2],
    pub _padding: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct BgInstance {
    pub rect: [f32; 4],  // x, y, width, height in pixels
    pub color: [f32; 4], // RGBA
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GlyphInstance {
    pub rect: [f32; 4],  // x, y, width, height in pixels
    pub uv: [f32; 4],    // u_min, v_min, u_max, v_max
    pub color: [f32; 4], // RGBA
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionRange {
    pub start: (usize, usize), // (row, col)
    pub end: (usize, usize),   // (row, col)
}

impl SelectionRange {
    pub fn new(start: (usize, usize), end: (usize, usize)) -> Self {
        Self { start, end }
    }

    pub fn normalized(&self) -> ((usize, usize), (usize, usize)) {
        if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    pub fn contains(&self, row: usize, col: usize) -> bool {
        let (s, e) = self.normalized();
        if row < s.0 || row > e.0 {
            return false;
        }
        if s.0 == e.0 {
            col >= s.1 && col <= e.1
        } else if row == s.0 {
            col >= s.1
        } else if row == e.0 {
            col <= e.1
        } else {
            true
        }
    }
}

pub fn boost_saturation(rgba: [f32; 4], factor: f32) -> [f32; 4] {
    if (factor - 1.0).abs() < 0.001 {
        return rgba;
    }
    let l = 0.2126 * rgba[0] + 0.7152 * rgba[1] + 0.0722 * rgba[2];
    [
        (l + (rgba[0] - l) * factor).clamp(0.0, 1.0),
        (l + (rgba[1] - l) * factor).clamp(0.0, 1.0),
        (l + (rgba[2] - l) * factor).clamp(0.0, 1.0),
        rgba[3],
    ]
}

#[derive(Clone, Debug)]
pub struct Palette {
    pub bg: [f32; 4],
    pub fg: [f32; 4],
    pub cursor: [f32; 4],
    pub selection: [f32; 4],
    pub ansi: [(u8, u8, u8); 16],
}

impl Default for Palette {
    fn default() -> Self {
        Palette {
            bg: [0.118, 0.118, 0.118, 1.0], // #1e1e1e
            fg: [0.800, 0.800, 0.800, 1.0], // #cccccc
            cursor: [0.900, 0.900, 0.900, 0.75],
            selection: [0.22, 0.45, 0.85, 0.50], // Semi-transparent blue highlight
            ansi: [
                (0, 0, 0),       // 0: Black
                (205, 49, 49),   // 1: Red
                (13, 188, 121),  // 2: Green
                (229, 229, 16),  // 3: Yellow
                (36, 114, 200),  // 4: Blue
                (188, 63, 188),  // 5: Magenta
                (17, 168, 205),  // 6: Cyan
                (229, 229, 229), // 7: White
                (102, 102, 102), // 8: Bright Black (Grey)
                (241, 76, 76),   // 9: Bright Red
                (35, 209, 139),  // 10: Bright Green
                (245, 245, 67),  // 11: Bright Yellow
                (59, 142, 234),  // 12: Bright Blue
                (214, 112, 214), // 13: Bright Magenta
                (41, 184, 219),  // 14: Bright Cyan
                (255, 255, 255), // 15: Bright White
            ],
        }
    }
}

pub fn hex_to_rgba(hex: &str) -> Option<[f32; 4]> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
    } else if hex.len() == 8 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
        Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0])
    } else {
        None
    }
}

pub fn palette_from_vscode_colors(colors: &std::collections::HashMap<String, String>) -> Palette {
    let mut p = Palette::default();
    if let Some(bg) = colors.get("terminal.background").or_else(|| colors.get("editor.background")) {
        if let Some(c) = hex_to_rgba(bg) { p.bg = c; }
    }
    if let Some(fg) = colors.get("terminal.foreground").or_else(|| colors.get("editor.foreground")) {
        if let Some(c) = hex_to_rgba(fg) { p.fg = c; }
    }
    if let Some(cur) = colors.get("terminalCursor.foreground").or_else(|| colors.get("editorCursor.foreground")) {
        if let Some(c) = hex_to_rgba(cur) { p.cursor = c; }
    }
    if let Some(sel) = colors.get("terminal.selectionBackground").or_else(|| colors.get("editor.selectionBackground")) {
        if let Some(c) = hex_to_rgba(sel) { p.selection = c; }
    }
    let ansi_keys = [
        "terminal.ansiBlack", "terminal.ansiRed", "terminal.ansiGreen", "terminal.ansiYellow",
        "terminal.ansiBlue", "terminal.ansiMagenta", "terminal.ansiCyan", "terminal.ansiWhite",
        "terminal.ansiBrightBlack", "terminal.ansiBrightRed", "terminal.ansiBrightGreen", "terminal.ansiBrightYellow",
        "terminal.ansiBrightBlue", "terminal.ansiBrightMagenta", "terminal.ansiBrightCyan", "terminal.ansiBrightWhite",
    ];
    for (i, key) in ansi_keys.iter().enumerate() {
        if let Some(hex) = colors.get(*key) {
            if let Some(rgba) = hex_to_rgba(hex) {
                p.ansi[i] = ((rgba[0] * 255.0) as u8, (rgba[1] * 255.0) as u8, (rgba[2] * 255.0) as u8);
            }
        }
    }
    p
}

pub struct TerminalRenderer {
    pub atlas: GlyphAtlas,
    pub uniform_buffer: wgpu::Buffer,
    pub uniform_bind_group: wgpu::BindGroup,

    pub bg_pipeline: wgpu::RenderPipeline,
    pub glyph_pipeline: wgpu::RenderPipeline,

    pub bg_buffer: wgpu::Buffer,
    pub glyph_buffer: wgpu::Buffer,

    pub atlas_bind_group: wgpu::BindGroup,
    pub atlas_bind_group_layout: wgpu::BindGroupLayout,

    pub palette: Palette,

    pub saturation_boost: f32,

    max_cells: usize,
}

const SHADER_SRC: &str = r#"
struct Uniforms {
    screen_size: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

// --- Background Pass ---
struct BgInstance {
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
};

struct BgVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_bg(
    @builtin(vertex_index) vertex_index: u32,
    instance: BgInstance,
) -> BgVertexOutput {
    var quad = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0)
    );

    let unit = quad[vertex_index];
    let pixel_pos = instance.rect.xy + unit * instance.rect.zw;
    let ndc_x = (pixel_pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (pixel_pos.y / uniforms.screen_size.y) * 2.0;

    var out: BgVertexOutput;
    out.position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.color = instance.color;
    return out;
}

@fragment
fn fs_bg(in: BgVertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}

// --- Glyph Pass ---
@group(1) @binding(0)
var atlas_texture: texture_2d<f32>;
@group(1) @binding(1)
var atlas_sampler: sampler;

struct GlyphInstance {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
};

struct GlyphVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_glyph(
    @builtin(vertex_index) vertex_index: u32,
    instance: GlyphInstance,
) -> GlyphVertexOutput {
    var quad = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0)
    );

    let unit = quad[vertex_index];
    let pixel_pos = instance.rect.xy + unit * instance.rect.zw;
    let ndc_x = (pixel_pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (pixel_pos.y / uniforms.screen_size.y) * 2.0;

    let uv = mix(instance.uv.xy, instance.uv.zw, unit);

    var out: GlyphVertexOutput;
    out.position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = uv;
    out.color = instance.color;
    return out;
}

@fragment
fn fs_glyph(in: GlyphVertexOutput) -> @location(0) vec4<f32> {
    let raw_alpha = textureSample(atlas_texture, atlas_sampler, in.uv).r;
    if (raw_alpha <= 0.001) {
        discard;
    }
    // Perceptual gamma correction for anti-aliasing against dark backgrounds.
    // Compensates for non-linear sRGB blending and prevents thin stems (like 'i' and 'l') from fading.
    let corrected_alpha = pow(raw_alpha, 1.0 / 1.6);
    return vec4<f32>(in.color.rgb, in.color.a * corrected_alpha);
}
"#;

impl TerminalRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font_family: Option<&str>,
        font_size: f32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let atlas = GlyphAtlas::new(device, queue, font_family, font_size)?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terminal_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terminal_uniform_buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("uniform_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniform_bind_group"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let atlas_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("atlas_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
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

        let atlas_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas_bind_group"),
            layout: &atlas_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&atlas.sampler),
                },
            ],
        });

        let bg_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bg_pipeline_layout"),
            bind_group_layouts: &[Some(&uniform_bind_group_layout)],
            immediate_size: 0,
        });

        let bg_instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<BgInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        };

        let bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bg_pipeline"),
            layout: Some(&bg_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_bg"),
                buffers: &[Some(bg_instance_layout)],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_bg"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let glyph_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("glyph_pipeline_layout"),
                bind_group_layouts: &[Some(&uniform_bind_group_layout), Some(&atlas_bind_group_layout)],
                immediate_size: 0,
            });

        let glyph_instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GlyphInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        };

        let glyph_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glyph_pipeline"),
            layout: Some(&glyph_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_glyph"),
                buffers: &[Some(glyph_instance_layout)],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_glyph"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let max_cells = 256 * 100;
        let bg_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bg_instance_buffer"),
            size: (max_cells * std::mem::size_of::<BgInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let glyph_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glyph_instance_buffer"),
            size: (max_cells * std::mem::size_of::<GlyphInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(TerminalRenderer {
            atlas,
            uniform_buffer,
            uniform_bind_group,
            bg_pipeline,
            glyph_pipeline,
            bg_buffer,
            glyph_buffer,
            atlas_bind_group,
            atlas_bind_group_layout,
            palette: Palette::default(),
            saturation_boost: 1.0,
            max_cells,
        })
    }

    pub fn set_saturation_boost(&mut self, boost: f32) {
        self.saturation_boost = boost;
    }

    pub fn update_font(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font_family: Option<&str>,
        font_size: f32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let new_atlas = GlyphAtlas::new(device, queue, font_family, font_size)?;
        let new_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas_bind_group"),
            layout: &self.atlas_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&new_atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&new_atlas.sampler),
                },
            ],
        });
        self.atlas = new_atlas;
        self.atlas_bind_group = new_bind_group;
        Ok(())
    }

    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// Render the terminal grid into the destination texture view.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        grid: &Grid,
        cursor: &crate::term::Cursor,
        cursor_visible: bool,
        display_offset: usize,
        selection: Option<&SelectionRange>,
        hovered_link: Option<&((usize, usize), (usize, usize))>,
    ) {
        let uniforms = Uniforms {
            screen_size: [width as f32, height as f32],
            _padding: [0.0, 0.0],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let cell_w = self.atlas.cell_width;
        let cell_h = self.atlas.cell_height;
        let ascent = self.atlas.ascent;

        let mut bg_instances = Vec::with_capacity(grid.rows * grid.cols);
        let mut glyph_instances = Vec::with_capacity(grid.rows * grid.cols);

        let default_bg = if self.saturation_boost > 1.0 {
            boost_saturation(self.palette.bg, self.saturation_boost)
        } else {
            self.palette.bg
        };
        let default_fg = if self.saturation_boost > 1.0 {
            boost_saturation(self.palette.fg, self.saturation_boost)
        } else {
            self.palette.fg
        };

        // 1. Draw entire background rect
        bg_instances.push(BgInstance {
            rect: [0.0, 0.0, width as f32, height as f32],
            color: default_bg,
        });

        for r in 0..grid.rows {
            let row = match grid.visible_row(r, display_offset) {
                Some(row) => row,
                None => continue,
            };
            let y = r as f32 * cell_h;
            if y >= height as f32 {
                break;
            }

            for (c, cell) in row.iter().enumerate() {
                let x = c as f32 * cell_w;
                if x >= width as f32 {
                    break;
                }

                let is_selected = selection.map(|s| s.contains(r, c)).unwrap_or(false);
                let is_link = hovered_link.map(|(s, e)| {
                    if r < s.0 || r > e.0 {
                        false
                    } else if s.0 == e.0 {
                        c >= s.1 && c <= e.1
                    } else if r == s.0 {
                        c >= s.1
                    } else if r == e.0 {
                        c <= e.1
                    } else {
                        true
                    }
                }).unwrap_or(false);

                let is_inverse = (cell.flags & flags::INVERSE) != 0;
                let (mut fg_color, mut bg_color) = if is_inverse {
                    (
                        resolve_color(cell.bg, default_bg, default_fg, &self.palette.ansi, false),
                        resolve_color(cell.fg, default_bg, default_fg, &self.palette.ansi, true),
                    )
                } else {
                    (
                        resolve_color(cell.fg, default_fg, default_bg, &self.palette.ansi, false),
                        resolve_color(cell.bg, default_fg, default_bg, &self.palette.ansi, true),
                    )
                };

                if self.saturation_boost > 1.0 {
                    fg_color = boost_saturation(fg_color, self.saturation_boost);
                    bg_color = boost_saturation(bg_color, self.saturation_boost);
                }

                // Non-default background cell
                if cell.bg != Color::Default || is_inverse {
                    bg_instances.push(BgInstance {
                        rect: [x, y, cell_w, cell_h],
                        color: bg_color,
                    });
                }

                // Selection highlight
                if is_selected {
                    let sel_color = if self.saturation_boost > 1.0 {
                        boost_saturation(self.palette.selection, self.saturation_boost)
                    } else {
                        self.palette.selection
                    };
                    bg_instances.push(BgInstance {
                        rect: [x, y, cell_w, cell_h],
                        color: sel_color,
                    });
                }

                // Glyph
                if cell.c != ' ' && cell.c != '\0' {
                    let mut cell_flags = cell.flags;
                    if is_link {
                        cell_flags |= flags::UNDERLINE;
                    }
                    let info = self.atlas.get_or_insert(queue, cell.c, cell_flags);
                    if info.size[0] > 0.0 && info.size[1] > 0.0 {
                        let glyph_x = x + info.offset[0];
                        let glyph_y = y + (ascent - info.offset[1]);

                        glyph_instances.push(GlyphInstance {
                            rect: [glyph_x, glyph_y, info.size[0], info.size[1]],
                            uv: [info.uv_min[0], info.uv_min[1], info.uv_max[0], info.uv_max[1]],
                            color: fg_color,
                        });
                    }
                }
            }
        }

        // Draw cursor if visible and looking at live prompt (display_offset == 0)
        if cursor_visible && display_offset == 0 && cursor.row < grid.rows && cursor.col < grid.cols {
            let cx = cursor.col as f32 * cell_w;
            let cy = cursor.row as f32 * cell_h;
            bg_instances.push(BgInstance {
                rect: [cx, cy, cell_w, cell_h],
                color: self.palette.cursor,
            });
        }

        if bg_instances.len() > self.max_cells {
            bg_instances.truncate(self.max_cells);
        }
        if glyph_instances.len() > self.max_cells {
            glyph_instances.truncate(self.max_cells);
        }

        queue.write_buffer(
            &self.bg_buffer,
            0,
            bytemuck::cast_slice(&bg_instances),
        );
        queue.write_buffer(
            &self.glyph_buffer,
            0,
            bytemuck::cast_slice(&glyph_instances),
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("terminal_render_encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terminal_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: default_bg[0] as f64,
                            g: default_bg[1] as f64,
                            b: default_bg[2] as f64,
                            a: default_bg[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // Pass 1: Backgrounds
            pass.set_pipeline(&self.bg_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_vertex_buffer(0, self.bg_buffer.slice(..));
            pass.draw(0..6, 0..bg_instances.len() as u32);

            // Pass 2: Glyphs
            if !glyph_instances.is_empty() {
                pass.set_pipeline(&self.glyph_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_bind_group(1, &self.atlas_bind_group, &[]);
                pass.set_vertex_buffer(0, self.glyph_buffer.slice(..));
                pass.draw(0..6, 0..glyph_instances.len() as u32);
            }
        }

        queue.submit(Some(encoder.finish()));
    }
}

fn resolve_color(
    color: Color,
    default_fg: [f32; 4],
    default_bg: [f32; 4],
    ansi: &[(u8, u8, u8); 16],
    is_bg: bool,
) -> [f32; 4] {
    match color {
        Color::Default => if is_bg { default_bg } else { default_fg },
        Color::Rgb(r, g, b) => [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0],
        Color::Indexed(idx) => {
            let (r, g, b) = ansi_to_rgb(idx, ansi);
            [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
        }
    }
}

/// Standard 16 ANSI colours + 256 colour palette resolution.
fn ansi_to_rgb(idx: u8, ansi: &[(u8, u8, u8); 16]) -> (u8, u8, u8) {
    if (idx as usize) < ansi.len() {
        return ansi[idx as usize];
    }

    if idx >= 232 {
        // Greyscale ramp (232..=255)
        let v = (idx - 232) * 10 + 8;
        return (v, v, v);
    }

    // 6x6x6 color cube (16..=231)
    let idx = idx - 16;
    let r = (idx / 36) % 6;
    let g = (idx / 6) % 6;
    let b = idx % 6;
    let step = |c: u8| if c == 0 { 0 } else { c * 40 + 55 };
    (step(r), step(g), step(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::Terminal;

    #[test]
    fn renders_terminal_grid_to_texture() {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .expect("Failed to find Vulkan adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("renderer_test_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        }))
        .expect("Failed to create wgpu device");

        let mut renderer = TerminalRenderer::new(&device, &queue, None, 14.0)
            .expect("Failed to create terminal renderer");

        assert!(renderer.atlas.cell_width > 0.0);
        assert!(renderer.atlas.cell_height > 0.0);

        let mut term = Terminal::new(80, 24);
        term.advance("cd ~ / á ç ^ ` \x1b[31;42mHello \x1b[1;34mWorld!\x1b[0m".as_bytes());

        let target_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test_target"),
            size: wgpu::Extent3d {
                width: 800,
                height: 600,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target_texture.create_view(&wgpu::TextureViewDescriptor::default());

        renderer.render(
            &device,
            &queue,
            &target_view,
            800,
            600,
            term.grid(),
            &term.state.cursor,
            true,
            0,
            None,
            None,
        );

        let tilde_info = renderer.atlas.cache.get(&('~', 0)).expect("tilde not cached");
        assert!(tilde_info.size[0] > 0.0 && tilde_info.size[1] > 0.0, "tilde size is 0: {:?}", tilde_info);

        let a_acute_info = renderer.atlas.cache.get(&('á', 0)).expect("á not cached");
        assert!(a_acute_info.size[0] > 0.0 && a_acute_info.size[1] > 0.0, "á size is 0: {:?}", a_acute_info);

        // Read back texture pixels to verify ~ is actually drawn with visible pixels
        let align_w = ((800 * 4 + 255) / 256) * 256;
        let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback_buf"),
            size: (align_w * 600) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(align_w),
                    rows_per_image: Some(600),
                },
            },
            wgpu::Extent3d {
                width: 800,
                height: 600,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = readback_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| { tx.send(res).unwrap(); });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();

        let data_view = slice.get_mapped_range().unwrap();
        let data: &[u8] = &data_view;
        // Check cell 3 (where ~ is located)
        let cell_w = renderer.atlas.cell_width as u32;
        let cell_h = renderer.atlas.cell_height as u32;
        let start_x = 3 * cell_w;
        let mut non_bg_pixels = 0;
        for py in 0..cell_h {
            for px in start_x..start_x + cell_w {
                let idx = (py * align_w + px * 4) as usize;
                let r = data[idx];
                let g = data[idx + 1];
                let b = data[idx + 2];
                // default bg is ~30, 30, 30 (#1e1e1e)
                if r > 50 || g > 50 || b > 50 {
                    non_bg_pixels += 1;
                }
            }
        }
        println!("Non-background pixels in ~ cell: {}", non_bg_pixels);
        assert!(non_bg_pixels > 0, "No visible pixels rendered for tilde!");
    }

    #[test]
    fn test_update_font_replaces_atlas_and_renders_cleanly() {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        })).expect("Failed to get wgpu adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("test_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        })).expect("Failed to create wgpu device");

        let mut renderer = TerminalRenderer::new(&device, &queue, None, 14.0).unwrap();

        let target_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test_target"),
            size: wgpu::Extent3d { width: 400, height: 300, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut term = Terminal::new(40, 15);
        term.advance("Original Text".as_bytes());

        // First render pass
        renderer.render(&device, &queue, &target_view, 400, 300, term.grid(), &term.state.cursor, true, 0, None, None);

        // Update font family and size
        renderer.update_font(&device, &queue, Some("Liberation Mono"), 18.0).unwrap();

        term.advance("\nUpdated Font Text".as_bytes());

        // Second render pass with updated font
        renderer.render(&device, &queue, &target_view, 400, 300, term.grid(), &term.state.cursor, true, 0, None, None);

        assert_eq!(renderer.atlas.font_size, 18.0);
    }

    #[test]
    fn test_selection_range_contains_and_normalization() {
        let sel = SelectionRange {
            start: (2, 5),
            end: (1, 10),
        };
        let (s, e) = sel.normalized();
        assert_eq!(s, (1, 10));
        assert_eq!(e, (2, 5));

        assert!(sel.contains(1, 10));
        assert!(sel.contains(1, 15));
        assert!(sel.contains(2, 0));
        assert!(sel.contains(2, 5));
        assert!(!sel.contains(1, 9));
        assert!(!sel.contains(2, 6));
        assert!(!sel.contains(0, 5));
        assert!(!sel.contains(3, 0));
    }

    #[test]
    fn test_boost_saturation() {
        let original = [0.2, 0.4, 0.8, 1.0];
        let boosted = boost_saturation(original, 1.5);
        assert_eq!(boosted[3], 1.0); // alpha unchanged
        // Saturation factor 1.5 increases distance from luminance
        assert!(boosted[2] > original[2]);
        assert!(boosted[0] <= original[0]);
    }
}

