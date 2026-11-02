---
title: "Complete terminal tab GObject, PTY, and Vulkan memory lifecycle"
author: Guilherme
date: 2026-11-02
branch: main
id: l-51429ad2
scope: repo
status: active
replaces: l-16594797
src: agent
touches:
  - src/widget.rs
  - src/main.rs
  - src/split.rs
  - src/pty.rs
  - src/render/dmabuf.rs
  - src/render/pipeline.rs
  - src/render/atlas.rs
---

Calling tv.terminate() and notebook.remove_page() alone did not free closed tabs because:
1. GObject reference cycles inside TerminalView: im_context.set_client_widget(Some(&widget)) holds a strong reference to TerminalView while widget's key controller holds im_context, and drag_controller / motion_controller closures cloned their own controller handles. Because TerminalView never disposed (ref_count stayed 2), its controller closures stayed alive and held strong Arc<Mutex<Terminal>> and Arc<Mutex<Pty>> clones, preventing Terminal grids/scrollback and Pty master FDs from ever dropping. Always call im_context.set_client_widget(None) in terminate()/dispose() and access inner.terminal / inner.pty via weak widget upgrades inside controller closures.
2. Tab header and Popover GObject cycles in main.rs: Popovers parented to tab_box via set_parent(&tab_box) must be unparent()ed before removing the page (via split::cleanup_tab_label), and all tab header / menu closures must capture weak references (.downgrade()) to view, tab_box, popover, rename_popover, label, and buttons.
3. Per-tab OS thread glibc arenas: Spawning a pty-reader std::thread per tab caused glibc to allocate a 64 MB mmap heap arena per tab that remained mapped after thread exit. Polling the PTY master FD directly on the GTK main loop via g_unix_fd_add_full with an 8 KB stack buffer eliminates per-tab threads and arenas.
4. GPU / DMA-BUF memory on integrated GPUs: DmabufFrame must call self.texture.destroy() in Drop followed by device.poll(PollType::wait_indefinitely()), background unmapped tabs should drop their double-buffered DmabufFrames on connect_unmap, and TerminalRenderer pipeline state, vertex buffers, and GlyphAtlas textures should be shared across tabs (keyed by ash::vk::Device handle rather than wgpu::Device PartialEq, which only compares local wgpu-core DeviceId).
