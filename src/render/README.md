# render

## Overview
GPU rendering subsystem for terminal grids, handling glyph rasterization into an `R8Unorm` texture atlas (`swash` + `fontdb`), zero-copy Vulkan external memory (`DMA_BUF_EXT`) frame allocation and export to `gdk4::DmabufTextureBuilder`, and two-pass `wgpu` instanced rendering (cell backgrounds/underlines/cursors and gamma-corrected glyphs).

## Interface
- `[mod.rs:atlas]`, `[mod.rs:dmabuf]`, `[mod.rs:pipeline]`: Public submodules (`pub mod`) of `render`.
- `[atlas.rs:GlyphInfo]`: UV coordinates (`uv_min`, `uv_max`), bearing `offset`, and pixel `size` for a rasterized glyph.
- `[atlas.rs:GlyphAtlasInner]`: Holds backing `texture: wgpu::Texture`, font face buffers (`font_datas`, `bold_font_datas`), `font_size`, `swash::scale::ScaleContext`, `(char, u8)` glyph `cache`, and shelf allocator cursor state (`cursor_x`, `cursor_y`, `row_height`). Implements `Drop`.
- `[atlas.rs:GlyphAtlas]`: Cloneable handle holding `view: wgpu::TextureView`, `sampler: wgpu::Sampler`, cell metrics (`cell_width`, `cell_height`, `ascent`, `font_size`), and `Arc<Mutex<GlyphAtlasInner>>`. Methods: `[atlas.rs:GlyphAtlas::new]`, `[atlas.rs:GlyphAtlas::get_or_insert]` (plus `#[cfg(test)]` `[atlas.rs:GlyphAtlas::get_cached]`).
- `[dmabuf.rs:DRM_FORMAT_ABGR8888]`, `[dmabuf.rs:DRM_FORMAT_MOD_LINEAR]`: DRM fourcc (`AB24`) and linear modifier (`0`) constants.
- `[dmabuf.rs:ExportedDmabuf]`: Holds raw `vk_image: vk::Image`, `vk_memory: vk::DeviceMemory`, exported `fd: OwnedFd`, `stride`, `offset`, `modifier`, `width`, and `height`.
- `[dmabuf.rs:DmabufFrame]`: Wraps the Vulkan external-memory image into `texture: wgpu::Texture`, `view: wgpu::TextureView`, and `exported: ExportedDmabuf`. Implements `Drop`. Methods: `[dmabuf.rs:DmabufFrame::new]`, `[dmabuf.rs:DmabufFrame::build_gdk_texture]`.
- `[pipeline.rs:Uniforms]`, `[pipeline.rs:BgInstance]`, `[pipeline.rs:GlyphInstance]`: `#[repr(C)]` `bytemuck::Pod` / `Zeroable` GPU buffer types for `screen_size`, background/underline/cursor quads, and glyph quads.
- `[pipeline.rs:SelectionRange]`: Buffer-coordinate selection span `(start, end)` with `[pipeline.rs:SelectionRange::new]`, `[pipeline.rs:SelectionRange::normalized]`, and `[pipeline.rs:SelectionRange::contains]`.
- `[pipeline.rs:Palette]`: Terminal color configuration (`bg`, `fg`, `cursor`, `selection`, and 16-color `ansi` table; implements `Default`).
- `[pipeline.rs:boost_saturation]`, `[pipeline.rs:hex_to_rgba]`, `[pipeline.rs:palette_from_vscode_colors]`: Color conversion and Rec.709 luminance saturation boost helpers.
- `[pipeline.rs:TerminalRenderer]`: Manages the two-pass `wgpu::RenderPipeline` (`vs_bg`/`fs_bg` and `vs_glyph`/`fs_glyph`), uniform and instance buffers, `palette`, `saturation_boost`, and glyph atlas bind groups. Methods: `[pipeline.rs:TerminalRenderer::new]`, `[pipeline.rs:TerminalRenderer::set_saturation_boost]`, `[pipeline.rs:TerminalRenderer::update_font]`, `[pipeline.rs:TerminalRenderer::set_palette]`, `[pipeline.rs:TerminalRenderer::render]`.

## Invariants
- `[atlas.rs:ATLAS_SIZE]`: Glyph atlas texture is fixed at `1024x1024` (`wgpu::TextureFormat::R8Unorm`, `TextureUsages::TEXTURE_BINDING | COPY_DST`), with `ClampToEdge` addressing and `Linear` min/mag filtering (`[atlas.rs:GlyphAtlas::new]`).
- `[atlas.rs:GlyphAtlas::new]`: Process-global `ATLAS_CACHE` caches at most one `GlyphAtlas` keyed by `(Option<ash::vk::Device>, Option<String>, u32)` (trimmed `font_family` and `font_size.to_bits()`), active only when `device.as_hal::<wgpu::hal::api::Vulkan>()` returns `Some`. General system fallback faces cap at `font_datas.len() >= 20`, returning `Err` if no fonts are found or the primary face fails to load.
- `[atlas.rs:GlyphAtlas::get_or_insert]`: Shelf packing starts at `(1, 1, 0)` with 1px spacing, wraps to a new row when `cursor_x + w + 1 >= ATLAS_SIZE`, and evicts/resets the entire cache (`cache.clear()`, `cursor_x = 1`, `cursor_y = 1`, `row_height = 0`) when `cursor_y + h + 1 >= ATLAS_SIZE`. Bold faces (`flags::BOLD`) prioritize `bold_font_datas`.
- `[dmabuf.rs:DmabufFrame::new]`: Requires a `wgpu::Device` backed by `wgpu::hal::api::Vulkan`; creates a `vk::Image` (`vk::Format::R8G8B8A8_UNORM`, `vk::ImageTiling::LINEAR`, `DMA_BUF_EXT`), selects the first matching memory type with `MemoryDedicatedAllocateInfo`, and wraps it as `wgpu::TextureFormat::Rgba8Unorm` with `DRM_FORMAT_MOD_LINEAR`.
- `[dmabuf.rs:DmabufFrame::build_gdk_texture]`: Duplicates `self.exported.fd` via `libc::dup` before passing it to `gdk::DmabufTextureBuilder::set_fd` (`n_planes = 1`, `DRM_FORMAT_ABGR8888`), closing `dup_fd` in the release callback (or immediately if building fails) so `ExportedDmabuf::fd` remains owned across frames.
- `[pipeline.rs:get_or_create_shared_pipeline]` & `[pipeline.rs:TerminalRenderer::new]`: `SharedPipelineState` is cached globally by `ash::vk::Device` handle; instance buffers (`bg_buffer`, `glyph_buffer`) are allocated for `max_cells = 256 * 100` (`25,600` instances), and `bg_instances` / `glyph_instances` are truncated to `self.max_cells` before `queue.write_buffer`.
- `[pipeline.rs:TerminalRenderer::render]`: Stops row/column emission at `y >= height` or `x >= width`; skips glyph quads for `' '`, `'\0'`, `flags::WIDE_SPACER`, or zero-sized placements; renders cursor only when `cursor_visible && display_offset == 0 && cursor.row < grid.rows && cursor.col < grid.cols`; applies `pow(raw_alpha, 1.0 / 1.6)` gamma correction in `fs_glyph`.

## Side Effects
- `[atlas.rs:GlyphAtlas::new]`: Queries `crate::fonts::shared_font_db()` and `crate::fonts::get_cached_face_data`, allocates GPU `Texture`/`TextureView`/`Sampler`, and populates static `ATLAS_CACHE`.
- `[atlas.rs:GlyphAtlas::get_or_insert]`: Rasterizes glyph bitmaps via `swash::scale::ScaleContext`, mutates the glyph `cache`, and uploads texels via `wgpu::Queue::write_texture`.
- `[atlas.rs:GlyphAtlasInner::drop]` & `[dmabuf.rs:DmabufFrame::drop]`: Explicitly call `self.texture.destroy()`.
- `[dmabuf.rs:DmabufFrame::new]` & `[dmabuf.rs:DmabufFrame::build_gdk_texture]`: Executes unsafe Vulkan FFI calls (`create_image`, `allocate_memory`, `bind_image_memory`, `VK_KHR_external_memory_fd::get_memory_fd`, `texture_from_raw`, `create_texture_from_hal`) and POSIX FD duplication/closure (`libc::dup`, `libc::close`).
- `[pipeline.rs:get_or_create_shared_pipeline]`: Compiles WGSL `SHADER_SRC`, creates shared GPU buffers/layouts/pipelines, and populates static `CACHE`.
- `[pipeline.rs:TerminalRenderer::render]`: Writes uniform and vertex buffers via `wgpu::Queue::write_buffer`, encodes `LoadOp::Clear` background and glyph render passes against `target_view`, and submits work via `wgpu::Queue::submit`.

## Verification
- Run: `cargo test`
