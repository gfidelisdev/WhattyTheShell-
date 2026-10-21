// src/widget.rs
//! Custom GTK4 terminal view widget.
//!
//! Subclasses `gtk4::Widget`, connects keystrokes to the PTY,
//! reads PTY output into the terminal state machine, renders via wgpu into
//! zero-copy Vulkan dmabufs, and displays via GTK snapshot.

use std::cell::RefCell;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread;

use gtk4::gdk;
use gtk4::glib;
use gtk4::graphene;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;

use crate::pty::Pty;
use crate::render::dmabuf::DmabufFrame;
use crate::render::pipeline::{SelectionRange, TerminalRenderer};
use crate::term::Terminal;

mod imp {
    use super::*;

    pub struct TerminalViewInner {
        pub pty: Arc<Mutex<Pty>>,
        pub terminal: Arc<Mutex<Terminal>>,
        pub renderer: RefCell<TerminalRenderer>,
        pub device: Arc<wgpu::Device>,
        pub queue: Arc<wgpu::Queue>,
        pub frames: RefCell<[Option<DmabufFrame>; 2]>,
        pub frame_index: std::cell::Cell<usize>,
        pub logical_width: RefCell<u32>,
        pub logical_height: RefCell<u32>,
        pub scale_factor: RefCell<i32>,
        pub dpi_scale: RefCell<f32>,
        pub base_font_size: RefCell<f32>,
        pub font_family: RefCell<Option<String>>,
        pub selection: RefCell<Option<SelectionRange>>,
        pub hovered_link: RefCell<Option<((usize, usize), (usize, usize))>>,
        pub hovered_url: RefCell<Option<String>>,
        pub ctrl_pressed: RefCell<bool>,
        pub drag_start: RefCell<Option<(usize, usize)>>,
        pub custom_title: RefCell<Option<String>>,
    }

    #[derive(Default)]
    pub struct TerminalView {
        pub inner: RefCell<Option<TerminalViewInner>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TerminalView {
        const NAME: &'static str = "WhattyTerminalView";
        type Type = super::TerminalView;
        type ParentType = gtk4::Widget;
    }

    impl ObjectImpl for TerminalView {
        fn signals() -> &'static [glib::subclass::Signal] {
            use std::sync::OnceLock;
            static SIGNALS: OnceLock<Vec<glib::subclass::Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    glib::subclass::Signal::builder("child-exited")
                        .run_last()
                        .build(),
                ]
            })
        }

        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_focusable(true);
            obj.set_can_focus(true);
            obj.set_focus_on_click(true);
            obj.set_hexpand(true);
            obj.set_vexpand(true);
        }

        fn dispose(&self) {
            let _ = self.inner.borrow_mut().take();
        }
    }

    impl WidgetImpl for TerminalView {
        fn measure(&self, orientation: gtk4::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk4::Orientation::Horizontal => (80, 640, -1, -1),
                gtk4::Orientation::Vertical => (50, 480, -1, -1),
                _ => (80, 480, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);

            let w = width.max(1) as u32;
            let h = height.max(1) as u32;
            self.obj().sync_scale_and_dimensions(w, h);
        }

        fn snapshot(&self, snapshot: &gtk4::Snapshot) {
            let obj = self.obj();
            let (log_w, log_h) = {
                let inner_guard = self.inner.borrow();
                let inner = match inner_guard.as_ref() {
                    Some(i) => i,
                    None => return,
                };
                let w = *inner.logical_width.borrow();
                let h = *inner.logical_height.borrow();
                (w, h)
            };

            if log_w == 0 || log_h == 0 {
                return;
            }

            // Ensure scale factor and dimensions are synchronized before allocating frame or rendering
            obj.sync_scale_and_dimensions(log_w, log_h);

            let inner_guard = self.inner.borrow();
            let inner = match inner_guard.as_ref() {
                Some(i) => i,
                None => return,
            };

            let scale = obj.scale_factor().max(1);
            let pixel_w = (log_w * scale as u32).max(1);
            let pixel_h = (log_h * scale as u32).max(1);

            // Double-buffering: allocate 2 DmabufFrames matching current dimensions
            let mut frames_guard = inner.frames.borrow_mut();
            let needs_new_frames = match &frames_guard[0] {
                Some(f) => f.exported.width != pixel_w || f.exported.height != pixel_h,
                None => true,
            };

            if needs_new_frames {
                let f0 = match DmabufFrame::new(&inner.device, pixel_w, pixel_h) {
                    Ok(frame) => frame,
                    Err(e) => {
                        eprintln!("Failed to allocate DmabufFrame 0: {e}");
                        return;
                    }
                };
                let f1 = match DmabufFrame::new(&inner.device, pixel_w, pixel_h) {
                    Ok(frame) => frame,
                    Err(e) => {
                        eprintln!("Failed to allocate DmabufFrame 1: {e}");
                        return;
                    }
                };
                *frames_guard = [Some(f0), Some(f1)];
                inner.frame_index.set(0);
            }

            // Ping-pong between frames: render into the back-buffer
            let next_idx = (inner.frame_index.get() + 1) % 2;
            let frame = match &frames_guard[next_idx] {
                Some(f) => f,
                None => return,
            };

            // Render current terminal grid into the dmabuf texture
            let term = match inner.terminal.lock() {
                Ok(t) => t,
                Err(_) => return,
            };

            let mut renderer = inner.renderer.borrow_mut();
            let selection = inner.selection.borrow();
            let hovered_link = inner.hovered_link.borrow();

            let display_offset = term.display_offset();

            renderer.render(
                &inner.device,
                &inner.queue,
                &frame.view,
                pixel_w,
                pixel_h,
                term.grid(),
                &term.state.cursor,
                term.state.cursor_visible,
                display_offset,
                selection.as_ref(),
                hovered_link.as_ref(),
            );

            // Block until GPU finishes drawing into this frame before handing to GTK
            let _ = inner.device.poll(wgpu::PollType::wait_indefinitely());

            inner.frame_index.set(next_idx);

            // Build GdkTexture from dmabuf
            let display = self.obj().display();

            match frame.build_gdk_texture(&display, || {}) {
                Ok(texture) => {
                    let rect = graphene::Rect::new(0.0, 0.0, log_w as f32, log_h as f32);
                    snapshot.append_texture(&texture, &rect);
                }
                Err(e) => {
                    eprintln!("Failed to build GdkTexture from dmabuf: {e}");
                }
            }
        }
    }
}

glib::wrapper! {
    pub struct TerminalView(ObjectSubclass<imp::TerminalView>)
        @extends gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

fn current_dpi_scale() -> f32 {
    gtk4::Settings::default()
        .map(|s| {
            let xft = s.property::<i32>("gtk-xft-dpi");
            if xft > 0 {
                (xft as f32 / 1024.0) / 96.0
            } else {
                1.0
            }
        })
        .unwrap_or(1.0)
}

impl TerminalView {
    pub fn sync_scale_and_dimensions(&self, log_w: u32, log_h: u32) -> bool {
        let inner_guard = self.imp().inner.borrow();
        let inner = match inner_guard.as_ref() {
            Some(i) => i,
            None => return false,
        };

        let scale = self.scale_factor().max(1);
        let dpi = current_dpi_scale();

        let mut cur_w = inner.logical_width.borrow_mut();
        let mut cur_h = inner.logical_height.borrow_mut();
        let mut cur_scale = inner.scale_factor.borrow_mut();
        let mut cur_dpi = inner.dpi_scale.borrow_mut();

        let scale_changed = *cur_scale != scale || (*cur_dpi - dpi).abs() > 0.001;
        let size_changed = *cur_w != log_w || *cur_h != log_h;

        if !scale_changed && !size_changed {
            return false;
        }

        if scale_changed {
            *cur_scale = scale;
            *cur_dpi = dpi;
            let effective_size = *inner.base_font_size.borrow() * scale as f32 * dpi;
            let fam = inner.font_family.borrow().clone();
            let _ = inner.renderer.borrow_mut().update_font(
                &inner.device,
                &inner.queue,
                fam.as_deref(),
                effective_size,
            );
        }

        *cur_w = log_w;
        *cur_h = log_h;

        let pixel_w = (log_w * scale as u32).max(1);
        let pixel_h = (log_h * scale as u32).max(1);

        let renderer = inner.renderer.borrow();
        let cell_w = renderer.atlas.cell_width;
        let cell_h = renderer.atlas.cell_height;

        let cols = ((pixel_w as f32 / cell_w).floor() as usize).max(1);
        let rows = ((pixel_h as f32 / cell_h).floor() as usize).max(1);

        if let Ok(mut term) = inner.terminal.lock() {
            term.resize(cols, rows);
        }
        if let Ok(pty) = inner.pty.lock() {
            let _ = pty.resize(cols as u16, rows as u16);
        }

        *inner.frames.borrow_mut() = [None, None];
        true
    }

    pub fn with_working_dir(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        font_family: Option<String>,
        font_size: f32,
        cwd: Option<&std::path::Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let widget: Self = glib::Object::builder().build();

        let scale = widget.scale_factor().max(1);
        let dpi = current_dpi_scale();
        let effective_size = font_size * scale as f32 * dpi;

        let renderer = TerminalRenderer::new(&device, &queue, font_family.as_deref(), effective_size)?;
        let cell_w = renderer.atlas.cell_width;
        let cell_h = renderer.atlas.cell_height;

        let init_cols = 80;
        let init_rows = 24;

        let pty = Arc::new(Mutex::new(Pty::spawn_shell(
            init_cols as u16,
            init_rows as u16,
            cwd,
        )?));

        let terminal = Arc::new(Mutex::new(Terminal::new(init_cols, init_rows)));

        let init_w = ((init_cols as f32 * cell_w) / scale as f32).ceil() as u32;
        let init_h = ((init_rows as f32 * cell_h) / scale as f32).ceil() as u32;

        let inner = imp::TerminalViewInner {
            pty: pty.clone(),
            terminal: terminal.clone(),
            renderer: RefCell::new(renderer),
            device,
            queue,
            frames: RefCell::new([None, None]),
            frame_index: std::cell::Cell::new(0),
            logical_width: RefCell::new(init_w),
            logical_height: RefCell::new(init_h),
            scale_factor: RefCell::new(scale),
            dpi_scale: RefCell::new(dpi),
            base_font_size: RefCell::new(font_size),
            font_family: RefCell::new(font_family),
            selection: RefCell::new(None),
            hovered_link: RefCell::new(None),
            hovered_url: RefCell::new(None),
            ctrl_pressed: RefCell::new(false),
            drag_start: RefCell::new(None),
            custom_title: RefCell::new(None),
        };

        widget.imp().inner.replace(Some(inner));

        // Setup background PTY reader thread with a bounded channel for backpressure
        let reader = pty.lock().unwrap().reader()?;
        let (sender, receiver) = async_channel::bounded::<Vec<u8>>(128);

        thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut reader = reader;
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            let _ = sender.send_blocking(Vec::new());
                            break;
                        }
                        Ok(n) => {
                            if sender.send_blocking(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(_) => {
                            let _ = sender.send_blocking(Vec::new());
                            break;
                        }
                    }
                }
            })?;

        // Handle PTY input on the main GTK loop
        let term_clone = terminal.clone();
        let pty_clone = pty.clone();
        let widget_weak = widget.downgrade();

        glib::spawn_future_local(async move {
            while let Ok(data) = receiver.recv().await {
                let widget = match widget_weak.upgrade() {
                    Some(w) => w,
                    None => break,
                };

                if data.is_empty() {
                    widget.emit_by_name::<()>("child-exited", &[]);
                    break;
                }

                // Batch multiple chunks up to 64KB per iteration to minimize locking and redraw calls
                let mut batch = data;
                let mut had_more = false;
                while batch.len() < 64 * 1024 {
                    match receiver.try_recv() {
                        Ok(more) if !more.is_empty() => {
                            batch.extend_from_slice(&more);
                            had_more = true;
                        }
                        _ => break,
                    }
                }

                if let Ok(mut term) = term_clone.lock() {
                    term.advance(&batch);
                    let responses = term.take_responses();
                    if !responses.is_empty() {
                        if let Ok(pty) = pty_clone.lock() {
                            let _ = pty.write_all(&responses);
                        }
                    }
                }
                widget.queue_draw();

                // If high throughput was received, cooperatively yield to the GTK main loop
                // to keep UI input, frame rendering, and window responsiveness smooth
                if had_more || batch.len() >= 32 * 1024 {
                    glib::timeout_future(std::time::Duration::from_millis(0)).await;
                }
            }
        });

        // Setup keyboard controller
        let key_controller = gtk4::EventControllerKey::new();
        
        let im_context = gtk4::IMMulticontext::new();
        im_context.set_client_widget(Some(&widget));
        im_context.focus_in();
        key_controller.set_im_context(Some(&im_context));
        
        let pty_for_im = pty.clone();
        let term_for_im = terminal.clone();
        let widget_for_im = widget.downgrade();
        im_context.connect_commit(move |_, text| {
            if let Ok(mut term) = term_for_im.lock() {
                if term.display_offset() > 0 {
                    term.scroll_to_bottom();
                    if let Some(w) = widget_for_im.upgrade() {
                        w.queue_draw();
                    }
                }
            }
            if let Ok(pty) = pty_for_im.lock() {
                let _ = pty.write_all(text.as_bytes());
            }
        });

        let pty_for_keys = pty.clone();
        let term_for_keys = terminal.clone();

        // Setup Actions for Context Menu
        let action_group = gtk4::gio::SimpleActionGroup::new();
        let copy_act = gtk4::gio::SimpleAction::new("copy", None);
        let view_copy = widget.downgrade();
        copy_act.connect_activate(move |_, _| {
            if let Some(w) = view_copy.upgrade() {
                w.copy_selection();
            }
        });
        action_group.add_action(&copy_act);

        let paste_act = gtk4::gio::SimpleAction::new("paste", None);
        let view_paste = widget.downgrade();
        paste_act.connect_activate(move |_, _| {
            if let Some(w) = view_paste.upgrade() {
                w.paste_clipboard();
            }
        });
        action_group.add_action(&paste_act);

        widget.insert_action_group("term", Some(&action_group));

        // Keyboard shortcuts and input
        let widget_for_keys = widget.downgrade();
        key_controller.connect_key_pressed(move |_, keyval, _keycode, state| {
            let is_ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            let is_shift = state.contains(gdk::ModifierType::SHIFT_MASK);
            let is_alt = state.contains(gdk::ModifierType::ALT_MASK);

            if let Some(w) = widget_for_keys.upgrade() {
                if let Some(inner) = w.imp().inner.borrow().as_ref() {
                    if keyval == gdk::Key::Control_L || keyval == gdk::Key::Control_R || is_ctrl {
                        *inner.ctrl_pressed.borrow_mut() = true;
                    }
                }

                // Ctrl+Shift shortcuts (Copy, Paste, Move Tab Left/Right)
                if is_ctrl && is_shift {
                    match keyval {
                        gdk::Key::C | gdk::Key::c => {
                            w.copy_selection();
                            return glib::Propagation::Stop;
                        }
                        gdk::Key::V | gdk::Key::v => {
                            w.paste_clipboard();
                            return glib::Propagation::Stop;
                        }
                        gdk::Key::Page_Up | gdk::Key::KP_Page_Up => {
                            if w.activate_action("win.move-tab-left", None).is_ok() {
                                return glib::Propagation::Stop;
                            }
                        }
                        gdk::Key::Page_Down | gdk::Key::KP_Page_Down => {
                            if w.activate_action("win.move-tab-right", None).is_ok() {
                                return glib::Propagation::Stop;
                            }
                        }
                        _ => {}
                    }
                }

                // Ctrl shortcuts (Previous/Next Tab)
                if is_ctrl && !is_shift {
                    match keyval {
                        gdk::Key::Page_Up | gdk::Key::KP_Page_Up => {
                            if w.activate_action("win.prev-tab", None).is_ok() {
                                return glib::Propagation::Stop;
                            }
                        }
                        gdk::Key::Page_Down | gdk::Key::KP_Page_Down => {
                            if w.activate_action("win.next-tab", None).is_ok() {
                                return glib::Propagation::Stop;
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Shift+PageUp, Shift+PageDown, Shift+Home, Shift+End for scrollback navigation
            if is_shift && !is_ctrl {
                if let Ok(mut term) = term_for_keys.lock() {
                    if !term.is_alt_active() {
                        let page_size = (term.grid().rows.saturating_sub(2)).max(1) as isize;
                        match keyval {
                            gdk::Key::Page_Up => {
                                term.scroll_display(page_size);
                                drop(term);
                                if let Some(w) = widget_for_keys.upgrade() {
                                    w.queue_draw();
                                }
                                return glib::Propagation::Stop;
                            }
                            gdk::Key::Page_Down => {
                                term.scroll_display(-page_size);
                                drop(term);
                                if let Some(w) = widget_for_keys.upgrade() {
                                    w.queue_draw();
                                }
                                return glib::Propagation::Stop;
                            }
                            gdk::Key::Home => {
                                term.scroll_to_top();
                                drop(term);
                                if let Some(w) = widget_for_keys.upgrade() {
                                    w.queue_draw();
                                }
                                return glib::Propagation::Stop;
                            }
                            gdk::Key::End => {
                                term.scroll_to_bottom();
                                drop(term);
                                if let Some(w) = widget_for_keys.upgrade() {
                                    w.queue_draw();
                                }
                                return glib::Propagation::Stop;
                            }
                            _ => {}
                        }
                    }
                }
            }

            let mut bytes = Vec::new();

            match keyval {
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    let kitty_active = term_for_keys
                        .lock()
                        .map(|t| t.kitty_keyboard_flags() > 0)
                        .unwrap_or(false);

                    if is_shift && !is_ctrl && !is_alt {
                        // Shift + Enter sends CSI 13;2u (Kitty Keyboard Protocol / CSI u)
                        bytes.extend_from_slice(b"\x1b[13;2u");
                    } else if is_ctrl && !is_shift && !is_alt {
                        if kitty_active {
                            bytes.extend_from_slice(b"\x1b[13;5u");
                        } else {
                            bytes.extend_from_slice(b"\n");
                        }
                    } else if is_alt && !is_shift && !is_ctrl {
                        if kitty_active {
                            bytes.extend_from_slice(b"\x1b[13;3u");
                        } else {
                            bytes.extend_from_slice(b"\x1b\r");
                        }
                    } else if is_ctrl && is_shift && !is_alt {
                        bytes.extend_from_slice(b"\x1b[13;6u");
                    } else if is_alt && is_shift && !is_ctrl {
                        bytes.extend_from_slice(b"\x1b[13;4u");
                    } else {
                        bytes.extend_from_slice(b"\r");
                    }
                }
                gdk::Key::BackSpace => {
                    if is_ctrl && !is_shift && !is_alt {
                        bytes.push(0x08);
                    } else if is_alt && !is_shift && !is_ctrl {
                        bytes.extend_from_slice(b"\x1b\x7f");
                    } else {
                        bytes.extend_from_slice(b"\x7f");
                    }
                }
                gdk::Key::Tab | gdk::Key::ISO_Left_Tab => bytes.extend_from_slice(b"\t"),
                gdk::Key::Escape => bytes.extend_from_slice(b"\x1b"),

                gdk::Key::Up | gdk::Key::KP_Up => {
                    let app = term_for_keys.lock().map(|t| t.state.app_cursor_keys).unwrap_or(false);
                    if is_ctrl && !is_shift && !is_alt {
                        bytes.extend_from_slice(b"\x1b[1;5A");
                    } else if is_shift && !is_ctrl && !is_alt {
                        bytes.extend_from_slice(b"\x1b[1;2A");
                    } else if is_alt && !is_shift && !is_ctrl {
                        bytes.extend_from_slice(b"\x1b[1;3A");
                    } else {
                        bytes.extend_from_slice(if app { b"\x1bOA" } else { b"\x1b[A" });
                    }
                }
                gdk::Key::Down | gdk::Key::KP_Down => {
                    let app = term_for_keys.lock().map(|t| t.state.app_cursor_keys).unwrap_or(false);
                    if is_ctrl && !is_shift && !is_alt {
                        bytes.extend_from_slice(b"\x1b[1;5B");
                    } else if is_shift && !is_ctrl && !is_alt {
                        bytes.extend_from_slice(b"\x1b[1;2B");
                    } else if is_alt && !is_shift && !is_ctrl {
                        bytes.extend_from_slice(b"\x1b[1;3B");
                    } else {
                        bytes.extend_from_slice(if app { b"\x1bOB" } else { b"\x1b[B" });
                    }
                }
                gdk::Key::Right | gdk::Key::KP_Right => {
                    let app = term_for_keys.lock().map(|t| t.state.app_cursor_keys).unwrap_or(false);
                    if is_ctrl && !is_shift && !is_alt {
                        bytes.extend_from_slice(b"\x1b[1;5C");
                    } else if is_shift && !is_ctrl && !is_alt {
                        bytes.extend_from_slice(b"\x1b[1;2C");
                    } else if is_alt && !is_shift && !is_ctrl {
                        bytes.extend_from_slice(b"\x1b[1;3C");
                    } else {
                        bytes.extend_from_slice(if app { b"\x1bOC" } else { b"\x1b[C" });
                    }
                }
                gdk::Key::Left | gdk::Key::KP_Left => {
                    let app = term_for_keys.lock().map(|t| t.state.app_cursor_keys).unwrap_or(false);
                    if is_ctrl && !is_shift && !is_alt {
                        bytes.extend_from_slice(b"\x1b[1;5D");
                    } else if is_shift && !is_ctrl && !is_alt {
                        bytes.extend_from_slice(b"\x1b[1;2D");
                    } else if is_alt && !is_shift && !is_ctrl {
                        bytes.extend_from_slice(b"\x1b[1;3D");
                    } else {
                        bytes.extend_from_slice(if app { b"\x1bOD" } else { b"\x1b[D" });
                    }
                }

                gdk::Key::Home | gdk::Key::KP_Home => bytes.extend_from_slice(b"\x1b[H"),
                gdk::Key::End | gdk::Key::KP_End => bytes.extend_from_slice(b"\x1b[F"),
                gdk::Key::Page_Up | gdk::Key::KP_Page_Up => bytes.extend_from_slice(b"\x1b[5~"),
                gdk::Key::Page_Down | gdk::Key::KP_Page_Down => bytes.extend_from_slice(b"\x1b[6~"),
                gdk::Key::Delete | gdk::Key::KP_Delete => {
                    if is_ctrl && !is_shift && !is_alt {
                        bytes.extend_from_slice(b"\x1b[3;5~");
                    } else if is_shift && !is_ctrl && !is_alt {
                        bytes.extend_from_slice(b"\x1b[3;2~");
                    } else if is_alt && !is_shift && !is_ctrl {
                        bytes.extend_from_slice(b"\x1b[3;3~");
                    } else {
                        bytes.extend_from_slice(b"\x1b[3~");
                    }
                }
                gdk::Key::Insert | gdk::Key::KP_Insert => bytes.extend_from_slice(b"\x1b[2~"),

                _ => {
                    if let Some(ch) = keyval.to_unicode() {
                        if is_ctrl {
                            let b = ch as u8;
                            if b.is_ascii_alphabetic() {
                                bytes.push(b.to_ascii_uppercase() - b'@');
                            } else if ch == ' ' || ch == '@' {
                                bytes.push(0);
                            } else if ch == '[' {
                                bytes.push(27);
                            } else if ch == '\\' {
                                bytes.push(28);
                            } else if ch == ']' {
                                bytes.push(29);
                            } else if ch == '^' {
                                bytes.push(30);
                            } else if ch == '_' {
                                bytes.push(31);
                            }
                        }
                    }
                }
            }

            if !bytes.is_empty() {
                if let Ok(mut term) = term_for_keys.lock() {
                    if term.display_offset() > 0 {
                        term.scroll_to_bottom();
                        if let Some(w) = widget_for_keys.upgrade() {
                            w.queue_draw();
                        }
                    }
                }
                if let Ok(pty) = pty_for_keys.lock() {
                    let _ = pty.write_all(&bytes);
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });

        let widget_for_key_rel = widget.downgrade();
        key_controller.connect_key_released(move |_, keyval, _keycode, state| {
            if keyval == gdk::Key::Control_L || keyval == gdk::Key::Control_R || !state.contains(gdk::ModifierType::CONTROL_MASK) {
                if let Some(w) = widget_for_key_rel.upgrade() {
                    if let Some(inner) = w.imp().inner.borrow().as_ref() {
                        *inner.ctrl_pressed.borrow_mut() = false;
                        if inner.hovered_link.borrow().is_some() {
                            *inner.hovered_link.borrow_mut() = None;
                            *inner.hovered_url.borrow_mut() = None;
                            w.set_cursor_from_name(Some("text"));
                            w.queue_draw();
                        }
                    }
                }
            }
        });

        widget.add_controller(key_controller);

        let focus_controller = gtk4::EventControllerFocus::new();
        let im_for_focus = im_context.clone();
        focus_controller.connect_enter(move |_| {
            im_for_focus.focus_in();
        });
        let im_for_leave = im_context.clone();
        focus_controller.connect_leave(move |_| {
            im_for_leave.focus_out();
        });
        widget.add_controller(focus_controller);

        // Mouse scroll controller for scrollback & alternate screen scrolling
        let scroll_controller = gtk4::EventControllerScroll::new(
            gtk4::EventControllerScrollFlags::VERTICAL | gtk4::EventControllerScrollFlags::DISCRETE,
        );
        let term_for_scroll = terminal.clone();
        let pty_for_scroll = pty.clone();
        let widget_for_scroll = widget.downgrade();

        scroll_controller.connect_scroll(move |controller, _dx, dy| {
            let state = controller.current_event_state();
            // If Ctrl is held, let it bubble to the window-level opacity/saturation controller
            if state.contains(gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }

            let mut term = match term_for_scroll.lock() {
                Ok(t) => t,
                Err(_) => return glib::Propagation::Proceed,
            };

            // If alternate screen is active (vim, less, htop, nano):
            // Send Up/Down arrow sequences to the child process
            if term.is_alt_active() {
                let app = term.state.app_cursor_keys;
                let code = if dy < 0.0 {
                    if app { b"\x1bOA" } else { b"\x1b[A" }
                } else if dy > 0.0 {
                    if app { b"\x1bOB" } else { b"\x1b[B" }
                } else {
                    return glib::Propagation::Stop;
                };

                if let Ok(pty) = pty_for_scroll.lock() {
                    for _ in 0..3 {
                        let _ = pty.write_all(code);
                    }
                }
                return glib::Propagation::Stop;
            }

            // Normal screen: scroll through history (dy < 0 = scroll up / older lines)
            let lines = if dy < 0.0 { 3 } else { -3 };
            term.scroll_display(lines);
            drop(term);

            if let Some(w) = widget_for_scroll.upgrade() {
                w.queue_draw();
            }

            glib::Propagation::Stop
        });
        widget.add_controller(scroll_controller);

        // Mouse Drag selection controller
        let drag_controller = gtk4::GestureDrag::new();
        drag_controller.set_button(gdk::BUTTON_PRIMARY);
        let drag_w = widget.downgrade();
        drag_controller.connect_drag_begin(move |_, x, y| {
            if let Some(w) = drag_w.upgrade() {
                if let Some(inner) = w.imp().inner.borrow().as_ref() {
                    if let Some(pos) = w.pixel_to_cell(x, y) {
                        *inner.drag_start.borrow_mut() = Some(pos);
                        *inner.selection.borrow_mut() = None;
                        w.queue_draw();
                    }
                }
            }
        });

        let drag_update_w = widget.downgrade();
        let drag_gesture_update = drag_controller.clone();
        drag_controller.connect_drag_update(move |_, offset_x, offset_y| {
            if let Some(w) = drag_update_w.upgrade() {
                if let Some(inner) = w.imp().inner.borrow().as_ref() {
                    if let Some(start_pos) = *inner.drag_start.borrow() {
                        let (start_x, start_y) = drag_gesture_update.start_point().unwrap_or((0.0, 0.0));
                        let cur_x = start_x + offset_x;
                        let cur_y = start_y + offset_y;
                        if let Some(cur_pos) = w.pixel_to_cell(cur_x, cur_y) {
                            *inner.selection.borrow_mut() = Some(SelectionRange::new(start_pos, cur_pos));
                            w.queue_draw();
                        }
                    }
                }
            }
        });

        let drag_end_w = widget.downgrade();
        drag_controller.connect_drag_end(move |_, _, _| {
            if let Some(w) = drag_end_w.upgrade() {
                if let Some(inner) = w.imp().inner.borrow().as_ref() {
                    if let Some(sel) = *inner.selection.borrow() {
                        if let Ok(term) = inner.terminal.lock() {
                            let text = term.extract_text(sel.start, sel.end);
                            if !text.is_empty() {
                                w.display().primary_clipboard().set_text(&text);
                            }
                        }
                    }
                    *inner.drag_start.borrow_mut() = None;
                    w.queue_draw();
                }
            }
        });
        widget.add_controller(drag_controller);

        // Click controller (single, double, triple, middle, right click)
        let click_controller = gtk4::GestureClick::new();
        click_controller.set_button(0); // Listen to all buttons
        let click_w = widget.downgrade();
        let im_for_click = im_context.clone();
        click_controller.connect_pressed(move |gesture, n_press, x, y| {
            let button = gesture.current_button();
            if let Some(w) = click_w.upgrade() {
                w.grab_focus();
                im_for_click.focus_in();

                if button == gdk::BUTTON_PRIMARY {
                    if let Some(inner) = w.imp().inner.borrow().as_ref() {
                        let is_ctrl = *inner.ctrl_pressed.borrow()
                            || gesture.current_event_state().contains(gdk::ModifierType::CONTROL_MASK);
                        if is_ctrl && n_press == 1 {
                            let clicked_url = if let Some((row, col)) = w.pixel_to_cell(x, y) {
                                inner.terminal.lock().ok().and_then(|term| term.url_at(row, col).map(|(u, _)| u))
                            } else {
                                None
                            };
                            let target_url = clicked_url.or_else(|| inner.hovered_url.borrow().clone());
                            if let Some(url) = target_url {
                                w.open_url(&url);
                                return;
                            }
                        }

                        if n_press == 2 {
                            if let Some((row, col)) = w.pixel_to_cell(x, y) {
                                if let Ok(term) = inner.terminal.lock() {
                                    if let Some((start, end)) = term.word_at(row, col) {
                                        *inner.selection.borrow_mut() = Some(SelectionRange::new(start, end));
                                        let text = term.extract_text(start, end);
                                        if !text.is_empty() {
                                            w.display().primary_clipboard().set_text(&text);
                                        }
                                        w.queue_draw();
                                    }
                                }
                            }
                        } else if n_press >= 3 {
                            if let Some((row, _)) = w.pixel_to_cell(x, y) {
                                if let Ok(term) = inner.terminal.lock() {
                                    let cols = term.grid().cols;
                                    if cols > 0 {
                                        *inner.selection.borrow_mut() = Some(SelectionRange::new((row, 0), (row, cols - 1)));
                                        let text = term.extract_text((row, 0), (row, cols - 1));
                                        if !text.is_empty() {
                                            w.display().primary_clipboard().set_text(&text);
                                        }
                                        w.queue_draw();
                                    }
                                }
                            }
                        } else if n_press == 1 && !is_ctrl {
                            if inner.drag_start.borrow().is_none() {
                                *inner.selection.borrow_mut() = None;
                                w.queue_draw();
                            }
                        }
                    }
                } else if button == gdk::BUTTON_MIDDLE {
                    w.paste_primary();
                } else if button == gdk::BUTTON_SECONDARY {
                    w.show_context_menu(x, y);
                }
            }
        });
        widget.add_controller(click_controller);

        // Motion controller for hover link detection
        let motion_controller = gtk4::EventControllerMotion::new();
        let motion_w = widget.downgrade();
        let motion_ctrl = motion_controller.clone();
        motion_controller.connect_motion(move |_, x, y| {
            if let Some(w) = motion_w.upgrade() {
                if let Some(inner) = w.imp().inner.borrow().as_ref() {
                    let is_ctrl = *inner.ctrl_pressed.borrow()
                        || motion_ctrl.current_event_state().contains(gdk::ModifierType::CONTROL_MASK);
                    if is_ctrl {
                        if let Some((row, col)) = w.pixel_to_cell(x, y) {
                            if let Ok(term) = inner.terminal.lock() {
                                if let Some((url, span)) = term.url_at(row, col) {
                                    w.set_cursor_from_name(Some("pointer"));
                                    *inner.hovered_link.borrow_mut() = Some(span);
                                    *inner.hovered_url.borrow_mut() = Some(url);
                                    w.queue_draw();
                                    return;
                                }
                            }
                        }
                    }
                    w.set_cursor_from_name(Some("text"));
                    if inner.hovered_link.borrow().is_some() {
                        *inner.hovered_link.borrow_mut() = None;
                        *inner.hovered_url.borrow_mut() = None;
                        w.queue_draw();
                    }
                }
            }
        });
        widget.add_controller(motion_controller);

        let widget_for_scale = widget.downgrade();
        widget.connect_notify_local(Some("scale-factor"), move |_, _| {
            if let Some(w) = widget_for_scale.upgrade() {
                let log_w = w.imp().inner.borrow().as_ref().map(|i| *i.logical_width.borrow()).unwrap_or(0);
                let log_h = w.imp().inner.borrow().as_ref().map(|i| *i.logical_height.borrow()).unwrap_or(0);
                if log_w > 0 && log_h > 0 {
                    if w.sync_scale_and_dimensions(log_w, log_h) {
                        w.queue_draw();
                    }
                }
            }
        });

        if let Some(settings) = gtk4::Settings::default() {
            let widget_for_dpi = widget.downgrade();
            settings.connect_notify_local(Some("gtk-xft-dpi"), move |_, _| {
                if let Some(w) = widget_for_dpi.upgrade() {
                    let log_w = w.imp().inner.borrow().as_ref().map(|i| *i.logical_width.borrow()).unwrap_or(0);
                    let log_h = w.imp().inner.borrow().as_ref().map(|i| *i.logical_height.borrow()).unwrap_or(0);
                    if log_w > 0 && log_h > 0 {
                        if w.sync_scale_and_dimensions(log_w, log_h) {
                            w.queue_draw();
                        }
                    }
                }
            });
        }

        let widget_for_realize = widget.downgrade();
        widget.connect_realize(move |_| {
            if let Some(w) = widget_for_realize.upgrade() {
                let log_w = w.imp().inner.borrow().as_ref().map(|i| *i.logical_width.borrow()).unwrap_or(0);
                let log_h = w.imp().inner.borrow().as_ref().map(|i| *i.logical_height.borrow()).unwrap_or(0);
                if log_w > 0 && log_h > 0 {
                    if w.sync_scale_and_dimensions(log_w, log_h) {
                        w.queue_draw();
                    }
                }
            }
        });

        Ok(widget)
    }

    pub fn set_palette(&self, palette: crate::render::pipeline::Palette) {
        if let Some(inner) = self.imp().inner.borrow().as_ref() {
            inner.renderer.borrow_mut().set_palette(palette);
            self.queue_draw();
        }
    }

    pub fn set_font(&self, font_family: Option<String>, font_size: f32) {
        let inner_guard = self.imp().inner.borrow();
        if let Some(inner) = inner_guard.as_ref() {
            *inner.font_family.borrow_mut() = font_family.clone();
            *inner.base_font_size.borrow_mut() = font_size;
            let scale = self.scale_factor().max(1);
            let dpi = current_dpi_scale();
            let effective_size = font_size * scale as f32 * dpi;
            *inner.scale_factor.borrow_mut() = scale;
            *inner.dpi_scale.borrow_mut() = dpi;

            let mut renderer = inner.renderer.borrow_mut();
            if renderer
                .update_font(
                    &inner.device,
                    &inner.queue,
                    font_family.as_deref(),
                    effective_size,
                )
                .is_ok()
            {
                let cell_w = renderer.atlas.cell_width;
                let cell_h = renderer.atlas.cell_height;
                drop(renderer);

                let log_w = *inner.logical_width.borrow();
                let log_h = *inner.logical_height.borrow();
                let pixel_w = (log_w * scale as u32).max(1);
                let pixel_h = (log_h * scale as u32).max(1);
                let cols = ((pixel_w as f32 / cell_w).floor() as usize).max(1);
                let rows = ((pixel_h as f32 / cell_h).floor() as usize).max(1);
                if let Ok(mut term) = inner.terminal.lock() {
                    term.resize(cols, rows);
                }
                if let Ok(pty) = inner.pty.lock() {
                    let _ = pty.resize(cols as u16, rows as u16);
                }
                *inner.frames.borrow_mut() = [None, None];
                self.queue_draw();
            }
        }
    }

    pub fn set_font_size(&self, font_size: f32) {
        let fam = self.imp().inner.borrow().as_ref().and_then(|i| i.font_family.borrow().clone());
        self.set_font(fam, font_size);
    }

    pub fn set_font_family(&self, font_family: Option<String>) {
        let size = self.imp().inner.borrow().as_ref().map(|i| *i.base_font_size.borrow()).unwrap_or(14.0);
        self.set_font(font_family, size);
    }

    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        font_family: Option<String>,
        font_size: f32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_working_dir(device, queue, font_family, font_size, None)
    }

    pub fn current_working_dir(&self) -> Option<std::path::PathBuf> {
        let inner_guard = self.imp().inner.borrow();
        let inner = inner_guard.as_ref()?;
        let pty = inner.pty.lock().ok()?;
        pty.current_working_dir()
    }

    pub fn custom_title(&self) -> Option<String> {
        let inner_guard = self.imp().inner.borrow();
        let inner = inner_guard.as_ref()?;
        let title = inner.custom_title.borrow().clone();
        title
    }

    pub fn set_custom_title(&self, title: Option<String>) {
        let inner_guard = self.imp().inner.borrow();
        if let Some(inner) = inner_guard.as_ref() {
            *inner.custom_title.borrow_mut() = title;
        }
    }

    pub fn pixel_to_cell(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let inner_guard = self.imp().inner.borrow();
        let inner = inner_guard.as_ref()?;
        let scale = *inner.scale_factor.borrow() as f64;
        let renderer = inner.renderer.borrow();
        let cell_w = renderer.atlas.cell_width as f64;
        let cell_h = renderer.atlas.cell_height as f64;
        if cell_w <= 0.0 || cell_h <= 0.0 {
            return None;
        }
        let px = x * scale;
        let py = y * scale;
        let col = (px / cell_w).floor().max(0.0) as usize;
        let row = (py / cell_h).floor().max(0.0) as usize;
        let term = inner.terminal.lock().ok()?;
        let grid = term.grid();
        let col = col.min(grid.cols.saturating_sub(1));
        let row = row.min(grid.rows.saturating_sub(1));
        Some((row, col))
    }

    pub fn copy_selection(&self) {
        let inner_guard = self.imp().inner.borrow();
        if let Some(inner) = inner_guard.as_ref() {
            if let Some(sel) = *inner.selection.borrow() {
                if let Ok(term) = inner.terminal.lock() {
                    let text = term.extract_text(sel.start, sel.end);
                    if !text.is_empty() {
                        self.display().clipboard().set_text(&text);
                        self.display().primary_clipboard().set_text(&text);
                    }
                }
            }
        }
    }

    pub fn paste_clipboard(&self) {
        let pty_clone = self.imp().inner.borrow().as_ref().map(|i| (i.pty.clone(), i.terminal.clone()));
        if let Some((pty, term)) = pty_clone {
            self.display().clipboard().read_text_async(None::<&gtk4::gio::Cancellable>, move |res| {
                if let Ok(Some(text)) = res {
                    let bracketed = term.lock().map(|t| t.state.bracketed_paste).unwrap_or(false);
                    let payload = if bracketed {
                        format!("\x1b[200~{}\x1b[201~", text)
                    } else {
                        text.to_string()
                    };
                    if let Ok(p) = pty.lock() {
                        let _ = p.write_all(payload.as_bytes());
                    }
                }
            });
        }
    }

    pub fn paste_primary(&self) {
        let pty_clone = self.imp().inner.borrow().as_ref().map(|i| (i.pty.clone(), i.terminal.clone()));
        if let Some((pty, term)) = pty_clone {
            self.display().primary_clipboard().read_text_async(None::<&gtk4::gio::Cancellable>, move |res| {
                if let Ok(Some(text)) = res {
                    let bracketed = term.lock().map(|t| t.state.bracketed_paste).unwrap_or(false);
                    let payload = if bracketed {
                        format!("\x1b[200~{}\x1b[201~", text)
                    } else {
                        text.to_string()
                    };
                    if let Ok(p) = pty.lock() {
                        let _ = p.write_all(payload.as_bytes());
                    }
                }
            });
        }
    }

    pub fn show_context_menu(&self, x: f64, y: f64) {
        let menu = gtk4::gio::Menu::new();
        menu.append(Some("Copiar (Ctrl+Shift+C)"), Some("term.copy"));
        menu.append(Some("Colar (Ctrl+Shift+V)"), Some("term.paste"));

        let split_section = gtk4::gio::Menu::new();
        split_section.append(Some("Dividir à Direita (Ctrl+Shift+E)"), Some("win.split-right"));
        split_section.append(Some("Dividir Abaixo (Ctrl+Shift+O)"), Some("win.split-down"));
        split_section.append(Some("Fechar Painel (Ctrl+Shift+W)"), Some("win.close-pane"));
        menu.append_section(None, &split_section);

        let popover = gtk4::PopoverMenu::from_model(Some(&menu));
        popover.set_parent(self);
        popover.set_has_arrow(false);
        let rect = gdk::Rectangle::new(x as i32, y as i32, 1, 1);
        popover.set_pointing_to(Some(&rect));
        popover.popup();
    }

    pub fn set_saturation_boost(&self, boost: f32) {
        if let Some(inner) = self.imp().inner.borrow().as_ref() {
            inner.renderer.borrow_mut().set_saturation_boost(boost);
            self.queue_draw();
        }
    }

    pub fn open_url(&self, url: &str) {
        let url = url.trim().to_string();
        if url.is_empty() {
            return;
        }

        let root = self.root();
        let window = root.as_ref().and_then(|r| r.downcast_ref::<gtk4::Window>());

        if url.starts_with("file://") {
            let clean_uri = url.split('#').next().unwrap_or(&url);
            let file = gtk4::gio::File::for_uri(clean_uri);
            let launcher = gtk4::FileLauncher::new(Some(&file));
            let clean_uri_clone = clean_uri.to_string();
            let file_clone = file.clone();

            launcher.launch(window, None::<&gtk4::gio::Cancellable>, move |res| {
                if let Err(err) = res {
                    eprintln!("[Whatty] FileLauncher failed ({}), attempting Gio launch...", err);
                    let display = gdk::Display::default();
                    let context = display.as_ref().map(|d| d.app_launch_context());
                    let gio_res = gtk4::gio::AppInfo::launch_default_for_uri(
                        &clean_uri_clone,
                        context.as_ref(),
                    );
                    if let Err(gio_err) = gio_res {
                        eprintln!("[Whatty] Gio launch failed ({}), falling back to xdg-open...", gio_err);
                        let path_arg = file_clone
                            .path()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_else(|| clean_uri_clone.clone());

                        if let Err(cmd_err) = std::process::Command::new("xdg-open")
                            .arg(&path_arg)
                            .spawn()
                        {
                            eprintln!("[Whatty] Failed to open file via xdg-open: {}", cmd_err);
                        }
                    }
                }
            });
            return;
        }

        // 1. Try GtkUriLauncher (GTK 4.10+) with parent window
        let launcher = gtk4::UriLauncher::new(&url);
        let url_clone = url.clone();
        launcher.launch(window, None::<&gtk4::gio::Cancellable>, move |res| {
            if let Err(err) = res {
                eprintln!("[Whatty] UriLauncher failed ({}), attempting Gio launch...", err);
                let display = gdk::Display::default();
                let context = display.as_ref().map(|d| d.app_launch_context());
                let gio_res = gtk4::gio::AppInfo::launch_default_for_uri(
                    &url_clone,
                    context.as_ref(),
                );
                if let Err(gio_err) = gio_res {
                    eprintln!("[Whatty] Gio launch failed ({}), falling back to xdg-open...", gio_err);
                    if let Err(cmd_err) = std::process::Command::new("xdg-open")
                        .arg(&url_clone)
                        .spawn()
                    {
                        eprintln!("[Whatty] Failed to open URL via xdg-open: {}", cmd_err);
                    }
                }
            }
        });
    }
}

