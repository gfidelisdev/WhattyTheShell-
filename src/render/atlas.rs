// src/render/atlas.rs
//! Glyph rasterization and GPU texture atlas for monospace terminal cells.

use std::collections::HashMap;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;
use swash::FontRef;

const ATLAS_SIZE: u32 = 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct GlyphInfo {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub offset: [f32; 2],
    pub size: [f32; 2],
}

pub struct GlyphAtlas {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub cell_width: f32,
    pub cell_height: f32,
    pub ascent: f32,

    font_datas: Vec<(Vec<u8>, u32)>,
    bold_font_datas: Vec<(Vec<u8>, u32)>,
    pub font_size: f32,

    scale_context: ScaleContext,
    pub cache: HashMap<(char, u8), GlyphInfo>,

    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
}

impl GlyphAtlas {
    pub fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        font_family: Option<&str>,
        font_size: f32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let mut queries = Vec::new();
        let mut bold_queries = Vec::new();

        let custom_fam;
        if let Some(fam) = font_family {
            let trimmed = fam.trim();
            if !trimmed.is_empty() {
                custom_fam = [fontdb::Family::Name(trimmed)];
                queries.push(fontdb::Query {
                    families: &custom_fam,
                    weight: fontdb::Weight::NORMAL,
                    ..fontdb::Query::default()
                });
                bold_queries.push(fontdb::Query {
                    families: &custom_fam,
                    weight: fontdb::Weight::BOLD,
                    ..fontdb::Query::default()
                });
            }
        }

        let mono_fams = [
            fontdb::Family::Name("Noto Sans Mono"),
            fontdb::Family::Name("Liberation Mono"),
            fontdb::Family::Name("Adwaita Mono"),
            fontdb::Family::Name("Source Code Pro"),
            fontdb::Family::Name("DejaVu Sans Mono"),
            fontdb::Family::Name("Nimbus Mono PS"),
            fontdb::Family::Monospace,
        ];

        queries.push(fontdb::Query {
            families: &mono_fams,
            weight: fontdb::Weight::NORMAL,
            ..fontdb::Query::default()
        });
        bold_queries.push(fontdb::Query {
            families: &mono_fams,
            weight: fontdb::Weight::BOLD,
            ..fontdb::Query::default()
        });

        let sans_fams = [
            fontdb::Family::Name("Noto Sans"),
            fontdb::Family::Name("DejaVu Sans"),
            fontdb::Family::SansSerif,
        ];

        queries.push(fontdb::Query {
            families: &[
                fontdb::Family::Name("Noto Sans"),
                fontdb::Family::Name("DejaVu Sans"),
                fontdb::Family::SansSerif,
            ],
            weight: fontdb::Weight::NORMAL,
            ..fontdb::Query::default()
        });
        bold_queries.push(fontdb::Query {
            families: &sans_fams,
            weight: fontdb::Weight::BOLD,
            ..fontdb::Query::default()
        });

        let mut font_datas = Vec::new();
        let mut bold_font_datas = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for query in &queries {
            if let Some(face_id) = db.query(query) {
                if seen.insert(face_id) {
                    db.with_face_data(face_id, |data, index| {
                        font_datas.push((data.to_vec(), index));
                    });
                }
            }
        }

        let mut seen_bold = std::collections::HashSet::new();
        for query in &bold_queries {
            if let Some(face_id) = db.query(query) {
                if seen_bold.insert(face_id) {
                    db.with_face_data(face_id, |data, index| {
                        bold_font_datas.push((data.to_vec(), index));
                    });
                }
            }
        }
        
        // Add all system faces as fallback for full Unicode / symbol coverage
        for face in db.faces() {
            if seen.insert(face.id) {
                db.with_face_data(face.id, |data, index| {
                    font_datas.push((data.to_vec(), index));
                });
                if font_datas.len() >= 40 {
                    break;
                }
            }
        }

        if font_datas.is_empty() {
            return Err("No fonts found on system".into());
        }

        let scale_context = ScaleContext::new();
        let (first_data, first_index) = &font_datas[0];
        let font_ref = FontRef::from_index(first_data, *first_index as usize)
            .ok_or("Failed to load first font")?;

        let metrics = font_ref.metrics(&[]).scale(font_size);
        let ascent = metrics.ascent.ceil().max(1.0);
        let descent = metrics.descent.abs().ceil();
        let cell_height = (ascent + descent + metrics.leading.max(0.0)).ceil().max(1.0);

        // Measure advance of 'M' or average
        let glyph_metrics = font_ref.glyph_metrics(&[]).scale(font_size);
        let m_glyph = font_ref.charmap().map('M');
        let advance = glyph_metrics.advance_width(m_glyph);
        let cell_width = if advance > 0.0 {
            advance.ceil().max(1.0)
        } else {
            (font_size * 0.6).ceil().max(1.0)
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyph_atlas_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Ok(GlyphAtlas {
            texture,
            view,
            sampler,
            cell_width,
            cell_height,
            ascent,
            font_datas,
            bold_font_datas,
            font_size,
            scale_context,
            cache: HashMap::new(),
            cursor_x: 1,
            cursor_y: 1,
            row_height: 0,
        })
    }

    pub fn get_or_insert(
        &mut self,
        queue: &wgpu::Queue,
        c: char,
        flags: u8,
    ) -> GlyphInfo {
        if let Some(info) = self.cache.get(&(c, flags)) {
            return *info;
        }

        let is_bold = (flags & crate::term::grid::flags::BOLD) != 0;
        let mut best_font_ref = None;
        let mut best_glyph_id = 0;

        let font_lists: [&Vec<(Vec<u8>, u32)>; 2] = if is_bold && !self.bold_font_datas.is_empty() {
            [&self.bold_font_datas, &self.font_datas]
        } else {
            [&self.font_datas, &self.bold_font_datas]
        };

        for list in font_lists {
            for (data, index) in list {
                if let Some(font_ref) = FontRef::from_index(data, *index as usize) {
                    let glyph_id = font_ref.charmap().map(c);
                    if glyph_id != 0 || c == ' ' {
                        best_font_ref = Some(font_ref);
                        best_glyph_id = glyph_id;
                        break;
                    }
                }
            }
            if best_font_ref.is_some() {
                break;
            }
        }

        let font_ref = match best_font_ref {
            Some(f) => f,
            None => return GlyphInfo::default(), // Character not found in any fallback
        };

        let mut scaler = self
            .scale_context
            .builder(font_ref)
            .size(self.font_size)
            .hint(true)
            .build();

        let mut render = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(swash::scale::StrikeWith::BestFit),
            Source::Outline,
        ]);
        render.format(Format::Alpha);

        let image = render.render(&mut scaler, best_glyph_id);
        let info = match image {
            Some(img) if img.placement.width > 0 && img.placement.height > 0 => {
                let w = img.placement.width;
                let h = img.placement.height;

                // Check shelf fit
                if self.cursor_x + w + 1 >= ATLAS_SIZE {
                    self.cursor_x = 1;
                    self.cursor_y += self.row_height + 1;
                    self.row_height = 0;
                }

                if self.cursor_y + h + 1 >= ATLAS_SIZE {
                    // Atlas full: fallback (or in future: clear / resize)
                    return GlyphInfo::default();
                }

                let x = self.cursor_x;
                let y = self.cursor_y;

                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &img.data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(w),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );

                self.cursor_x += w + 1;
                self.row_height = self.row_height.max(h);

                GlyphInfo {
                    uv_min: [x as f32 / ATLAS_SIZE as f32, y as f32 / ATLAS_SIZE as f32],
                    uv_max: [
                        (x + w) as f32 / ATLAS_SIZE as f32,
                        (y + h) as f32 / ATLAS_SIZE as f32,
                    ],
                    offset: [img.placement.left as f32, img.placement.top as f32],
                    size: [w as f32, h as f32],
                }
            }
            _ => GlyphInfo::default(),
        };

        self.cache.insert((c, flags), info);
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rasterize_special() {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let queries = [
            fontdb::Query {
                families: &[
                    fontdb::Family::Name("Noto Sans Mono"),
                    fontdb::Family::Name("Liberation Mono"),
                    fontdb::Family::Name("Adwaita Mono"),
                    fontdb::Family::Name("Source Code Pro"),
                    fontdb::Family::Name("DejaVu Sans Mono"),
                ],
                ..fontdb::Query::default()
            },
            fontdb::Query {
                families: &[fontdb::Family::Name("Noto Sans")],
                ..fontdb::Query::default()
            },
        ];

        let mut font_datas = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for query in &queries {
            if let Some(face_id) = db.query(query) {
                if seen.insert(face_id) {
                    db.with_face_data(face_id, |data, index| {
                        font_datas.push((data.to_vec(), index));
                    });
                }
            }
        }
        for face in db.faces() {
            if seen.insert(face.id) {
                db.with_face_data(face.id, |data, index| {
                    font_datas.push((data.to_vec(), index));
                });
                if font_datas.len() >= 25 {
                    break;
                }
            }
        }

        assert!(!font_datas.is_empty(), "No fonts loaded!");

        let mut sc = ScaleContext::new();

        for test_ch in ['a', '~', 'á', 'ç', '^', '`'] {
            let mut found_glyph = 0;
            let mut found_font = None;
            for (data, index) in &font_datas {
                if let Some(font_ref) = FontRef::from_index(data, *index as usize) {
                    let gid = font_ref.charmap().map(test_ch);
                    if gid != 0 {
                        found_glyph = gid;
                        found_font = Some(font_ref);
                        break;
                    }
                }
            }

            assert!(found_glyph != 0, "Glyph for {:?} not found!", test_ch);

            let font_ref = found_font.unwrap();
            let mut scaler = sc.builder(font_ref).size(14.0).hint(true).build();
            let mut render = Render::new(&[
                Source::ColorOutline(0),
                Source::ColorBitmap(swash::scale::StrikeWith::BestFit),
                Source::Outline,
            ]);
            render.format(Format::Alpha);
            let image = render.render(&mut scaler, found_glyph);
            assert!(image.is_some(), "Render returned None for {:?}", test_ch);
            let img = image.unwrap();
            assert!(img.placement.width > 0 && img.placement.height > 0, "Empty placement for {:?}", test_ch);
        }
    }

    #[test]
    fn test_glyph_stem_darkening_and_metrics() {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let q_bold = fontdb::Query {
            families: &[fontdb::Family::Name("Noto Sans Mono"), fontdb::Family::Name("Liberation Mono")],
            weight: fontdb::Weight::BOLD,
            ..fontdb::Query::default()
        };
        let bold_face_id = db.query(&q_bold).expect("bold font not found");
        db.with_face_data(bold_face_id, |bold_data, bold_index| {
            let bold_font_ref = FontRef::from_index(bold_data, bold_index as usize).unwrap();
            println!("Bold font: {:?}", bold_font_ref.attributes());
            let mut sc = ScaleContext::new();
            let gid = bold_font_ref.charmap().map('i');
            let mut scaler = sc.builder(bold_font_ref).size(14.0).hint(true).build();
            let mut render = Render::new(&[
                Source::ColorOutline(0),
                Source::ColorBitmap(swash::scale::StrikeWith::BestFit),
                Source::Outline,
            ]);
            render.format(Format::Alpha);
            let image = render.render(&mut scaler, gid).expect("Render bold 'i' failed");
            assert!(image.placement.width > 0 && image.placement.height > 0);
            let w = image.placement.width as usize;
            let mid_y = (image.placement.height / 2) as usize;
            let row = &image.data[mid_y * w..(mid_y + 1) * w];
            let solid_pixels = row.iter().filter(|&&val| val >= 100).count();
            assert!(solid_pixels >= 2, "Bold 'i' stem must have solid multi-pixel stroke: {:?}", row);
        });
    }
}


