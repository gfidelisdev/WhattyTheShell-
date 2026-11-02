// src/widget.rs
//! Custom GTK4 terminal view widget.
//!
//! Subclasses `gtk4::Widget`, connects keystrokes to the PTY,
//! reads PTY output into the terminal state machine, renders via wgpu into
//! zero-copy Vulkan dmabufs, and displays via GTK snapshot.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use gtk4::gdk;
use gtk4::glib;
use gtk4::graphene;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;

fn unix_fd_add_local<F>(fd: i32, condition: glib::IOCondition, func: F) -> glib::SourceId
where
    F: FnMut(i32, glib::IOCondition) -> glib::ControlFlow + 'static,
{
    unsafe extern "C" {
        fn g_unix_fd_add_full(
            priority: i32,
            fd: i32,
            condition: u32,
            function: Option<unsafe extern "C" fn(i32, u32, *mut std::ffi::c_void) -> i32>,
            user_data: *mut std::ffi::c_void,
            notify: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
        ) -> u32;
    }

    unsafe extern "C" fn trampoline<F: FnMut(i32, glib::IOCondition) -> glib::ControlFlow + 'static>(
        fd: i32,
        condition: u32,
        user_data: *mut std::ffi::c_void,
    ) -> i32 {
        let f = unsafe { &mut *(user_data as *mut F) };
        let cond = glib::IOCondition::from_bits_truncate(condition);
        match f(fd, cond) {
            glib::ControlFlow::Continue => 1,
            glib::ControlFlow::Break => 0,
        }
    }

    unsafe extern "C" fn destroy<F>(user_data: *mut std::ffi::c_void) {
        let _ = unsafe { Box::from_raw(user_data as *mut F) };
    }

    let boxed: Box<F> = Box::new(func);
    unsafe {
        let id = g_unix_fd_add_full(
            0, // G_PRIORITY_DEFAULT
            fd,
            condition.bits(),
            Some(trampoline::<F>),
            Box::into_raw(boxed) as *mut std::ffi::c_void,
            Some(destroy::<F>),
        );
        glib::translate::from_glib(id)
    }
}

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
        pub last_mouse_pos: RefCell<Option<(f64, f64)>>,
        pub drag_start: RefCell<Option<(usize, usize)>>,
        pub custom_title: RefCell<Option<String>>,
        pub dpi_handler_id: RefCell<Option<glib::SignalHandlerId>>,
        pub pty_source_id: RefCell<Option<glib::SourceId>>,
        pub im_context: gtk4::IMMulticontext,
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
            if let Some(inner) = self.inner.borrow_mut().take() {
                inner.im_context.set_client_widget(None::<&gtk4::Widget>);
                if let Some(source_id) = inner.pty_source_id.borrow_mut().take() {
                    source_id.remove();
                }
                if let Some(id) = inner.dpi_handler_id.borrow_mut().take() {
                    if let Some(settings) = gtk4::Settings::default() {
                        settings.disconnect(id);
                    }
                }
                if let Ok(mut pty) = inner.pty.lock() {
                    pty.terminate();
                }
            }
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
                if frames_guard[0].is_some() || frames_guard[1].is_some() {
                    *frames_guard = [None, None];
                    let _ = inner.device.poll(wgpu::PollType::wait_indefinitely());
                }
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
    /// Explicitly terminate the terminal, killing the child process group and releasing all GPU/memory resources.
    pub fn terminate(&self) {
        if let Some(inner) = self.imp().inner.borrow_mut().take() {
            inner.im_context.set_client_widget(None::<&gtk4::Widget>);
            if let Some(source_id) = inner.pty_source_id.borrow_mut().take() {
                source_id.remove();
            }
            if let Some(id) = inner.dpi_handler_id.borrow_mut().take() {
                if let Some(settings) = gtk4::Settings::default() {
                    settings.disconnect(id);
                }
            }
            if let Ok(mut pty) = inner.pty.lock() {
                pty.terminate();
            }
            let device = inner.device.clone();
            drop(inner);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            unsafe {
                libc::malloc_trim(0);
            }
        }
    }

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

        let master_fd = pty.lock().unwrap().raw_fd();

        let im_context = gtk4::IMMulticontext::new();
        im_context.set_client_widget(Some(&widget));
        im_context.focus_in();

        let inner = imp::TerminalViewInner {
            pty,
            terminal,
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
            last_mouse_pos: RefCell::new(None),
            drag_start: RefCell::new(None),
            custom_title: RefCell::new(None),
            dpi_handler_id: RefCell::new(None),
            pty_source_id: RefCell::new(None),
            im_context: im_context.clone(),
        };

        widget.imp().inner.replace(Some(inner));

        // Poll PTY master FD directly on the GTK main loop without spawning per-tab OS threads
        let widget_weak = widget.downgrade();
        let source_id = unix_fd_add_local(
            master_fd,
            glib::IOCondition::IN | glib::IOCondition::HUP | glib::IOCondition::ERR,
            move |_fd, _cond| {
                let widget = match widget_weak.upgrade() {
                    Some(w) => w,
                    None => return glib::ControlFlow::Break,
                };

                let mut exited = false;
                let mut read_any = false;
                let mut buf = [0u8; 8192];

                {
                    let inner_guard = widget.imp().inner.borrow();
                    let inner = match inner_guard.as_ref() {
                        Some(i) => i,
                        None => return glib::ControlFlow::Break,
                    };

                    let mut total_read = 0usize;
                    while total_read < 64 * 1024 {
                        let read_res = match inner.pty.lock() {
                            Ok(pty) => pty.try_read(&mut buf),
                            Err(_) => break,
                        };
                        match read_res {
                            Ok(0) => {
                                exited = true;
                                break;
                            }
                            Ok(n) => {
                                read_any = true;
                                total_read += n;
                                if let Ok(mut term) = inner.terminal.lock() {
                                    term.advance(&buf[..n]);
                                    let responses = term.take_responses();
                                    if !responses.is_empty() {
                                        if let Ok(pty) = inner.pty.lock() {
                                            let _ = pty.write_all(&responses);
                                        }
                                    }
                                }
                            }
                            Err(ref e)
                                if e.kind() == std::io::ErrorKind::WouldBlock
                                    || e.kind() == std::io::ErrorKind::Interrupted =>
                            {
                                break;
                            }
                            Err(_) => {
                                exited = true;
                                break;
                            }
                        }
                    }

                    if exited {
                        *inner.pty_source_id.borrow_mut() = None;
                    }
                }

                if read_any {
                    widget.queue_draw();
                }

                if exited {
                    widget.emit_by_name::<()>("child-exited", &[]);
                    return glib::ControlFlow::Break;
                }

                glib::ControlFlow::Continue
            },
        );

        if let Some(inner) = widget.imp().inner.borrow().as_ref() {
            *inner.pty_source_id.borrow_mut() = Some(source_id);
        }

        // Setup keyboard controller
        let key_controller = gtk4::EventControllerKey::new();
        key_controller.set_im_context(Some(&im_context));

        let widget_for_im = widget.downgrade();
        im_context.connect_commit(move |_, text| {
            if let Some(w) = widget_for_im.upgrade() {
                let mut need_draw = false;
                if let Some(inner) = w.imp().inner.borrow().as_ref() {
                    if let Ok(mut term) = inner.terminal.lock() {
                        if term.display_offset() > 0 {
                            term.scroll_to_bottom();
                            need_draw = true;
                        }
                    }
                    if let Ok(pty) = inner.pty.lock() {
                        let _ = pty.write_all(text.as_bytes());
                    }
                }
                if need_draw {
                    w.queue_draw();
                }
            }
        });

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

            let w = match widget_for_keys.upgrade() {
                Some(w) => w,
                None => return glib::Propagation::Proceed,
            };

            let (term_for_keys, pty_for_keys) = {
                let inner_guard = w.imp().inner.borrow();
                let inner = match inner_guard.as_ref() {
                    Some(i) => i,
                    None => return glib::Propagation::Proceed,
                };
                if keyval == gdk::Key::Control_L || keyval == gdk::Key::Control_R || is_ctrl {
                    *inner.ctrl_pressed.borrow_mut() = true;
                    if let Some((x, y)) = *inner.last_mouse_pos.borrow() {
                        if let Some((row, col)) = w.pixel_to_cell(x, y) {
                            if let Ok(term) = inner.terminal.lock() {
                                if let Some((url, span)) = term.url_at(row, col) {
                                    w.set_cursor_from_name(Some("pointer"));
                                    *inner.hovered_link.borrow_mut() = Some(span);
                                    *inner.hovered_url.borrow_mut() = Some(url);
                                    w.queue_draw();
                                }
                            }
                        }
                    }
                }
                (inner.terminal.clone(), inner.pty.clone())
            };

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
                    let kitty_active = term_for_keys
                        .lock()
                        .map(|t| t.kitty_keyboard_flags() > 0)
                        .unwrap_or(false);

                    if is_ctrl && !is_shift && !is_alt {
                        if kitty_active {
                            bytes.extend_from_slice(b"\x1b[127;5u");
                        } else {
                            // Ctrl+Backspace deletes previous word (standard readline / terminal rubout)
                            bytes.push(0x17); // 0x17 = ^W (unix-word-rubout)
                        }
                    } else if is_alt && !is_shift && !is_ctrl {
                        if kitty_active {
                            bytes.extend_from_slice(b"\x1b[127;3u");
                        } else {
                            bytes.extend_from_slice(b"\x1b\x7f");
                        }
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
        let widget_for_scroll = widget.downgrade();

        scroll_controller.connect_scroll(move |controller, _dx, dy| {
            let state = controller.current_event_state();
            // If Ctrl is held, let it bubble to the window-level opacity/saturation controller
            if state.contains(gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }

            let w = match widget_for_scroll.upgrade() {
                Some(w) => w,
                None => return glib::Propagation::Proceed,
            };
            let (term_for_scroll, pty_for_scroll) = {
                let inner_guard = w.imp().inner.borrow();
                match inner_guard.as_ref() {
                    Some(i) => (i.terminal.clone(), i.pty.clone()),
                    None => return glib::Propagation::Proceed,
                }
            };

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

            w.queue_draw();

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
        drag_controller.connect_drag_update(move |drag_gesture_update, offset_x, offset_y| {
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
        motion_controller.connect_motion(move |motion_ctrl, x, y| {
            if let Some(w) = motion_w.upgrade() {
                if let Some(inner) = w.imp().inner.borrow().as_ref() {
                    *inner.last_mouse_pos.borrow_mut() = Some((x, y));
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

        let motion_leave_w = widget.downgrade();
        motion_controller.connect_leave(move |_| {
            if let Some(w) = motion_leave_w.upgrade() {
                if let Some(inner) = w.imp().inner.borrow().as_ref() {
                    *inner.last_mouse_pos.borrow_mut() = None;
                    if inner.hovered_link.borrow().is_some() {
                        *inner.hovered_link.borrow_mut() = None;
                        *inner.hovered_url.borrow_mut() = None;
                        w.set_cursor_from_name(Some("text"));
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
            let handler_id = settings.connect_notify_local(Some("gtk-xft-dpi"), move |_, _| {
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
            if let Some(inner) = widget.imp().inner.borrow().as_ref() {
                *inner.dpi_handler_id.borrow_mut() = Some(handler_id);
            }
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

        // Release double-buffered DMA-BUF textures when the terminal is unmapped (e.g., background tab)
        // so inactive tabs do not hold ~8-16 MB of GPU/system RAM each.
        widget.connect_unmap(|w| {
            if let Some(inner) = w.imp().inner.borrow().as_ref() {
                let mut frames = inner.frames.borrow_mut();
                if frames[0].is_some() || frames[1].is_some() {
                    *frames = [None, None];
                    drop(frames);
                    let _ = inner.device.poll(wgpu::PollType::wait_indefinitely());
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
        let url = url.trim();
        if url.is_empty() {
            return;
        }

        let root = self.root();
        let window = root.as_ref().and_then(|r| r.downcast_ref::<gtk4::Window>());

        // 1. Web links (HTTP / HTTPS)
        if url.starts_with("http://") || url.starts_with("https://") {
            let launcher = gtk4::UriLauncher::new(url);
            let url_clone = url.to_string();
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
                        let _ = std::process::Command::new("xdg-open")
                            .arg(&url_clone)
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                    }
                }
            });
            return;
        }

        // 2. Filesystem paths & file:// URLs
        let (raw_path, line, col) = parse_path_and_location(url);

        // Expand home directory ~
        let expanded_path = if raw_path.starts_with("~/") || raw_path == "~" {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/home/gfidelis".to_string());
            if raw_path == "~" {
                std::path::PathBuf::from(home)
            } else {
                std::path::PathBuf::from(home).join(&raw_path[2..])
            }
        } else {
            std::path::PathBuf::from(&raw_path)
        };

        // Resolve relative paths with terminal's current working directory
        let final_path = if expanded_path.is_relative() {
            if let Some(cwd) = self.current_working_dir() {
                let joined = cwd.join(&expanded_path);
                if joined.exists() {
                    joined
                } else {
                    std::env::current_dir().map(|d| d.join(&expanded_path)).unwrap_or(joined)
                }
            } else {
                std::env::current_dir().map(|d| d.join(&expanded_path)).unwrap_or(expanded_path)
            }
        } else {
            expanded_path
        };

        // A. If line number is specified, try code --goto first
        if let Some(l) = line {
            let goto_arg = if let Some(c) = col {
                format!("{}:{}:{}", final_path.display(), l, c)
            } else {
                format!("{}:{}", final_path.display(), l)
            };

            if let Ok(_) = std::process::Command::new("code")
                .arg("--goto")
                .arg(&goto_arg)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                return;
            }
        }

        // B. Try opening via xdg-open directly (standard default desktop opener)
        if let Ok(_) = std::process::Command::new("xdg-open")
            .arg(&final_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            return;
        }

        // C. Fallback to Gio FileLauncher
        let uri = format!("file://{}", final_path.display());
        let file = gtk4::gio::File::for_uri(&uri);
        let launcher = gtk4::FileLauncher::new(Some(&file));
        launcher.launch(window, None::<&gtk4::gio::Cancellable>, move |_| {});
    }
}

fn parse_path_and_location(raw: &str) -> (String, Option<usize>, Option<usize>) {
    let s = raw.strip_prefix("file://").unwrap_or(raw);

    // Check #L anchor (e.g. #L123-L145 or #L123)
    if let Some((path, anchor)) = s.split_once('#') {
        let line = if anchor.starts_with('L') || anchor.starts_with('l') {
            let num_str = anchor[1..]
                .split('-')
                .next()
                .unwrap_or("")
                .trim();
            num_str.parse::<usize>().ok()
        } else {
            anchor.parse::<usize>().ok()
        };
        return (path.to_string(), line, None);
    }

    // Check :line:col or :line suffix
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() >= 3 {
        let last = parts[parts.len() - 1];
        let second_last = parts[parts.len() - 2];
        if let (Ok(col), Ok(line)) = (last.parse::<usize>(), second_last.parse::<usize>()) {
            let path = parts[..parts.len() - 2].join(":");
            return (path, Some(line), Some(col));
        }
    }
    if parts.len() >= 2 {
        let last = parts[parts.len() - 1];
        if let Ok(line) = last.parse::<usize>() {
            let path = parts[..parts.len() - 1].join(":");
            return (path, Some(line), None);
        }
    }

    (s.to_string(), None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_view_terminate_reaps_child() {
        if gtk4::is_initialized() {
            if !gtk4::is_initialized_main_thread() {
                return;
            }
        } else if gtk4::init().is_err() {
            return;
        }

        let instance = wgpu::Instance::default();
        let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        })) {
            Ok(a) => a,
            Err(_) => return,
        };
        let (device, queue) = match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())) {
            Ok(dq) => dq,
            Err(_) => return,
        };
        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let view = TerminalView::with_working_dir(device, queue, None, 12.0, None).unwrap();
        let child_pid = {
            let inner_guard = view.imp().inner.borrow();
            let inner = inner_guard.as_ref().unwrap();
            let pty = inner.pty.lock().unwrap();
            pty.child_pid()
        };

        // Ensure child is running initially
        assert!(child_pid > 0);
        let status = unsafe { libc::kill(child_pid as i32, 0) };
        assert_eq!(status, 0);

        // Terminate view
        view.terminate();

        // Ensure inner is cleared
        assert!(view.imp().inner.borrow().is_none());

        // Ensure child process is terminated (kill with 0 returns -1 with ESRCH)
        std::thread::sleep(std::time::Duration::from_millis(50));
        let status = unsafe { libc::kill(child_pid as i32, 0) };
        assert_ne!(status, 0);
    }

    #[derive(Debug, Default, Clone, Copy)]
    struct MemStats {
        vm_size: usize,
        vm_rss: usize,
        rss_anon: usize,
        rss_file: usize,
        rss_shmem: usize,
        threads: usize,
        fds: usize,
    }

    fn read_mem_stats() -> MemStats {
        let mut s = MemStats::default();
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(val) = parts[1].parse::<usize>() {
                        match parts[0] {
                            "VmSize:" => s.vm_size = val,
                            "VmRSS:" => s.vm_rss = val,
                            "RssAnon:" => s.rss_anon = val,
                            "RssFile:" => s.rss_file = val,
                            "RssShmem:" => s.rss_shmem = val,
                            "Threads:" => s.threads = val,
                            _ => {}
                        }
                    }
                }
            }
        }
        if let Ok(dir) = std::fs::read_dir("/proc/self/fd") {
            s.fds = dir.count();
        }
        s
    }

    #[test]
    fn test_open_close_tabs_memory_leak() {
        if gtk4::is_initialized() {
            if !gtk4::is_initialized_main_thread() {
                return;
            }
        } else if gtk4::init().is_err() {
            return;
        }

        let instance = wgpu::Instance::default();
        let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        })) {
            Ok(a) => a,
            Err(_) => return,
        };
        let (device, queue) = match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())) {
            Ok(dq) => dq,
            Err(_) => return,
        };
        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let notebook = gtk4::Notebook::new();

        // Warmup 1 tab and close it to initialize font db, shaders, GTK, etc.
        {
            let warm = TerminalView::with_working_dir(device.clone(), queue.clone(), None, 14.0, None).unwrap();
            warm.sync_scale_and_dimensions(1200, 800);
            if let Some(inner) = warm.imp().inner.borrow().as_ref() {
                let f0 = DmabufFrame::new(&inner.device, 1200, 800).unwrap();
                let f1 = DmabufFrame::new(&inner.device, 1200, 800).unwrap();
                let mut term = inner.terminal.lock().unwrap();
                term.advance(b"user@host:~/Code/rust/WhattyTheShell!$ ls -la\r\ndrwxr-xr-x 10 user user 4096 Nov 2 15:36 .\r\n");
                inner.renderer.borrow_mut().render(
                    &inner.device,
                    &inner.queue,
                    &f0.view,
                    1200,
                    800,
                    term.grid(),
                    &term.state.cursor,
                    true,
                    0,
                    None,
                    None,
                );
                let _ = inner.device.poll(wgpu::PollType::wait_indefinitely());
                let _ = f0.build_gdk_texture(&warm.display(), || {});
                *inner.frames.borrow_mut() = [Some(f0), Some(f1)];
            }
            warm.terminate();
            drop(warm);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        }

        while glib::MainContext::default().iteration(false) {}
        let mem_baseline = read_mem_stats();

        let mut after_cycle_rss = Vec::new();

        for cycle in 1..=3 {
            let mut weak_views = Vec::new();
            let mut weak_tab_boxes = Vec::new();
            let mut weak_popovers = Vec::new();
            for _ in 0..15 {
                let v = TerminalView::with_working_dir(device.clone(), queue.clone(), None, 14.0, None).unwrap();
                v.sync_scale_and_dimensions(1200, 800);
                if let Some(inner) = v.imp().inner.borrow().as_ref() {
                    let f0 = DmabufFrame::new(&inner.device, 1200, 800).unwrap();
                    let f1 = DmabufFrame::new(&inner.device, 1200, 800).unwrap();
                    let mut term = inner.terminal.lock().unwrap();
                    term.advance(b"user@host:~/Code/rust/WhattyTheShell!$ ls -la\r\ndrwxr-xr-x 10 user user 4096 Nov 2 15:36 .\r\n");
                    inner.renderer.borrow_mut().render(
                        &inner.device,
                        &inner.queue,
                        &f0.view,
                        1200,
                        800,
                        term.grid(),
                        &term.state.cursor,
                        true,
                        0,
                        None,
                        None,
                    );
                    let _ = inner.device.poll(wgpu::PollType::wait_indefinitely());
                    let _ = f0.build_gdk_texture(&v.display(), || {});
                    *inner.frames.borrow_mut() = [Some(f0), Some(f1)];
                }

                let tab_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
                let pop = gtk4::Popover::new();
                pop.set_parent(&tab_box);
                notebook.append_page(&v, Some(&tab_box));

                weak_views.push(v.downgrade());
                weak_tab_boxes.push(tab_box.downgrade());
                weak_popovers.push(pop.downgrade());
            }

            while glib::MainContext::default().iteration(false) {}

            while notebook.n_pages() > 0 {
                if let Some(page) = notebook.nth_page(Some(0)) {
                    for tv in crate::split::find_terminals(&page) {
                        tv.terminate();
                    }
                    crate::split::cleanup_tab_label(&notebook, &page);
                }
                notebook.remove_page(Some(0));
            }

            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            std::thread::sleep(std::time::Duration::from_millis(50));
            while glib::MainContext::default().iteration(false) {}
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            unsafe {
                libc::malloc_trim(0);
            }

            let alive_views = weak_views.iter().filter(|w| w.upgrade().is_some()).count();
            let alive_boxes = weak_tab_boxes.iter().filter(|w| w.upgrade().is_some()).count();
            let alive_pops = weak_popovers.iter().filter(|w| w.upgrade().is_some()).count();
            let mem_after = read_mem_stats();
            after_cycle_rss.push(mem_after.vm_rss);

            assert_eq!(alive_views, 0, "TerminalView GObjects leaked in cycle {cycle}!");
            assert_eq!(alive_boxes, 0, "Tab header Box GObjects leaked in cycle {cycle}!");
            assert_eq!(alive_pops, 0, "Tab Popover GObjects leaked in cycle {cycle}!");
            assert!(
                mem_after.fds <= mem_baseline.fds + 2,
                "File descriptors leaked in cycle {cycle}: baseline={}, after={}",
                mem_baseline.fds,
                mem_after.fds
            );
        }

        let growth_c1_to_c3 = after_cycle_rss[2].saturating_sub(after_cycle_rss[0]);
        assert!(
            growth_c1_to_c3 < 4096,
            "Memory continued growing across tab open/close cycles: cycle 1 RSS={} KB, cycle 3 RSS={} KB (+{} KB)",
            after_cycle_rss[0],
            after_cycle_rss[2],
            growth_c1_to_c3
        );
    }
}


