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
use crate::render::pipeline::TerminalRenderer;
use crate::term::Terminal;

mod imp {
    use super::*;

    pub struct TerminalViewInner {
        pub pty: Arc<Mutex<Pty>>,
        pub terminal: Arc<Mutex<Terminal>>,
        pub renderer: RefCell<TerminalRenderer>,
        pub device: Arc<wgpu::Device>,
        pub queue: Arc<wgpu::Queue>,
        pub current_frame: RefCell<Option<DmabufFrame>>,
        pub logical_width: RefCell<u32>,
        pub logical_height: RefCell<u32>,
        pub scale_factor: RefCell<i32>,
        pub base_font_size: RefCell<f32>,
        pub font_family: RefCell<Option<String>>,
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
                vec![glib::subclass::Signal::builder("child-exited")
                    .run_last()
                    .build()]
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
    }

    impl WidgetImpl for TerminalView {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);

            let scale = self.obj().scale_factor().max(1);
            let w = width.max(1) as u32;
            let h = height.max(1) as u32;

            let mut inner_guard = self.inner.borrow_mut();
            if let Some(inner) = inner_guard.as_mut() {
                let mut cur_w = inner.logical_width.borrow_mut();
                let mut cur_h = inner.logical_height.borrow_mut();
                let mut cur_scale = inner.scale_factor.borrow_mut();

                let scale_changed = *cur_scale != scale;
                let size_changed = *cur_w != w || *cur_h != h;

                if scale_changed {
                    *cur_scale = scale;
                    let effective_size = *inner.base_font_size.borrow() * scale as f32;
                    let fam = inner.font_family.borrow().clone();
                    let _ = inner.renderer.borrow_mut().update_font(
                        &inner.device,
                        &inner.queue,
                        fam.as_deref(),
                        effective_size,
                    );
                }

                if size_changed || scale_changed {
                    *cur_w = w;
                    *cur_h = h;

                    let pixel_w = (w * scale as u32).max(1);
                    let pixel_h = (h * scale as u32).max(1);

                    let renderer = inner.renderer.borrow();
                    let cell_w = renderer.atlas.cell_width;
                    let cell_h = renderer.atlas.cell_height;

                    let cols = ((pixel_w as f32 / cell_w).floor() as usize).max(1);
                    let rows = ((pixel_h as f32 / cell_h).floor() as usize).max(1);

                    // Resize PTY and terminal
                    if let Ok(mut term) = inner.terminal.lock() {
                        term.resize(cols, rows);
                    }
                    if let Ok(pty) = inner.pty.lock() {
                        let _ = pty.resize(cols as u16, rows as u16);
                    }

                    // Invalidate frame to reallocate on next snapshot
                    *inner.current_frame.borrow_mut() = None;
                }
            }
        }

        fn snapshot(&self, snapshot: &gtk4::Snapshot) {
            let inner_guard = self.inner.borrow();
            let inner = match inner_guard.as_ref() {
                Some(i) => i,
                None => return,
            };

            let log_w = *inner.logical_width.borrow();
            let log_h = *inner.logical_height.borrow();
            if log_w == 0 || log_h == 0 {
                return;
            }

            let scale = self.obj().scale_factor().max(1);
            let pixel_w = (log_w * scale as u32).max(1);
            let pixel_h = (log_h * scale as u32).max(1);

            // Ensure we have a valid DmabufFrame matching current dimensions
            let mut frame_guard = inner.current_frame.borrow_mut();
            let needs_new_frame = match frame_guard.as_ref() {
                Some(f) => f.exported.width != pixel_w || f.exported.height != pixel_h,
                None => true,
            };

            if needs_new_frame {
                match DmabufFrame::new(&inner.device, pixel_w, pixel_h) {
                    Ok(frame) => *frame_guard = Some(frame),
                    Err(e) => {
                        eprintln!("Failed to allocate DmabufFrame: {e}");
                        return;
                    }
                }
            }

            let frame = match frame_guard.as_ref() {
                Some(f) => f,
                None => return,
            };

            // Render current terminal grid into the dmabuf texture
            let term = match inner.terminal.lock() {
                Ok(t) => t,
                Err(_) => return,
            };

            let mut renderer = inner.renderer.borrow_mut();
            renderer.render(
                &inner.device,
                &inner.queue,
                &frame.view,
                pixel_w,
                pixel_h,
                term.grid(),
                &term.state.cursor,
                term.state.cursor_visible,
            );

            inner.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();

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

impl TerminalView {
    pub fn with_working_dir(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        font_family: Option<String>,
        font_size: f32,
        cwd: Option<&std::path::Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let widget: Self = glib::Object::builder().build();

        let scale = widget.scale_factor().max(1);
        let effective_size = font_size * scale as f32;

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
            current_frame: RefCell::new(None),
            logical_width: RefCell::new(init_w),
            logical_height: RefCell::new(init_h),
            scale_factor: RefCell::new(scale),
            base_font_size: RefCell::new(font_size),
            font_family: RefCell::new(font_family),
        };

        widget.imp().inner.replace(Some(inner));

        // Setup background PTY reader thread
        let reader = pty.lock().unwrap().reader()?;
        let (sender, receiver) = async_channel::unbounded::<Vec<u8>>();

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
                if data.is_empty() {
                    if let Some(w) = widget_weak.upgrade() {
                        w.emit_by_name::<()>("child-exited", &[]);
                    }
                    break;
                }
                if let Ok(mut term) = term_clone.lock() {
                    term.advance(&data);
                    let responses = term.take_responses();
                    if !responses.is_empty() {
                        if let Ok(pty) = pty_clone.lock() {
                            let _ = pty.write_all(&responses);
                        }
                    }
                }
                if let Some(w) = widget_weak.upgrade() {
                    w.queue_draw();
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
        im_context.connect_commit(move |_, text| {
            if let Ok(pty) = pty_for_im.lock() {
                let _ = pty.write_all(text.as_bytes());
            }
        });

        let pty_for_keys = pty.clone();
        let term_for_keys = terminal.clone();

        key_controller.connect_key_pressed(move |_, keyval, _keycode, state| {
            let mut bytes = Vec::new();
            let is_ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);

            match keyval {
                gdk::Key::Return | gdk::Key::KP_Enter => bytes.extend_from_slice(b"\r"),
                gdk::Key::BackSpace => bytes.extend_from_slice(b"\x7f"),
                gdk::Key::Tab | gdk::Key::ISO_Left_Tab => bytes.extend_from_slice(b"\t"),
                gdk::Key::Escape => bytes.extend_from_slice(b"\x1b"),

                gdk::Key::Up | gdk::Key::KP_Up => {
                    let app = term_for_keys.lock().map(|t| t.state.app_cursor_keys).unwrap_or(false);
                    bytes.extend_from_slice(if app { b"\x1bOA" } else { b"\x1b[A" });
                }
                gdk::Key::Down | gdk::Key::KP_Down => {
                    let app = term_for_keys.lock().map(|t| t.state.app_cursor_keys).unwrap_or(false);
                    bytes.extend_from_slice(if app { b"\x1bOB" } else { b"\x1b[B" });
                }
                gdk::Key::Right | gdk::Key::KP_Right => {
                    let app = term_for_keys.lock().map(|t| t.state.app_cursor_keys).unwrap_or(false);
                    bytes.extend_from_slice(if app { b"\x1bOC" } else { b"\x1b[C" });
                }
                gdk::Key::Left | gdk::Key::KP_Left => {
                    let app = term_for_keys.lock().map(|t| t.state.app_cursor_keys).unwrap_or(false);
                    bytes.extend_from_slice(if app { b"\x1bOD" } else { b"\x1b[D" });
                }

                gdk::Key::Home | gdk::Key::KP_Home => bytes.extend_from_slice(b"\x1b[H"),
                gdk::Key::End | gdk::Key::KP_End => bytes.extend_from_slice(b"\x1b[F"),
                gdk::Key::Page_Up | gdk::Key::KP_Page_Up => bytes.extend_from_slice(b"\x1b[5~"),
                gdk::Key::Page_Down | gdk::Key::KP_Page_Down => bytes.extend_from_slice(b"\x1b[6~"),
                gdk::Key::Delete | gdk::Key::KP_Delete => bytes.extend_from_slice(b"\x1b[3~"),
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
                if let Ok(pty) = pty_for_keys.lock() {
                    let _ = pty.write_all(&bytes);
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
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

        let click_controller = gtk4::GestureClick::new();
        let click_w = widget.downgrade();
        let im_for_click = im_context.clone();
        click_controller.connect_pressed(move |_, _, _, _| {
            if let Some(w) = click_w.upgrade() {
                w.grab_focus();
                im_for_click.focus_in();
            }
        });
        widget.add_controller(click_controller);

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
            let scale = *inner.scale_factor.borrow();
            let effective_size = font_size * scale as f32;

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
                *inner.current_frame.borrow_mut() = None;
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
}

