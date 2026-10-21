//! WhattyTheShell! – GTK‑based GPU accelerated terminal emulator
//!
//! Features:
//! - Full terminal emulation (vte state machine, PTY)
//! - Zero-copy Vulkan dmabuf GPU rendering with wgpu
//! - TabGroup supporting both horizontal and vertical tab orientations
//! - VS Code theme integration
//! - Session state & tab working directory persistence

use std::sync::Arc;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, glib};

mod config;
mod fonts;
mod gpu_renderer;
mod pty;
mod render;
mod settings_dialog;
mod split;
mod tab_group;
mod term;
mod theme_manager;
mod theme_marketplace;
mod widget;

use config::ConfigManager;
use gpu_renderer::GPURenderer;
use tab_group::TabGroup;
use theme_manager::ThemeManager;
use widget::TerminalView;

fn main() {
    // Initialise GTK application
    let app = Application::builder()
        .application_id("io.github.whattytheshell")
        .build();

    // Load user configuration
    let config = ConfigManager::load_or_default();

    // GPU renderer backend detection
    let _gpu = GPURenderer::new(&config);

    // Theme handling – load the currently selected theme
    let theme_manager = ThemeManager::new(&config);

    let config_manager = std::rc::Rc::new(std::cell::RefCell::new(config));

    app.connect_activate(move |app| {
        let config_manager = config_manager.clone();
        let mut theme_manager = theme_manager.clone();

        // Register local icon search paths if running from source tree or unpacked directory
        if let Some(display) = gtk4::gdk::Display::default() {
            let icon_theme = gtk4::IconTheme::for_display(&display);
            if std::path::Path::new("assets/icons").exists() {
                icon_theme.add_search_path("assets/icons");
            }
            if std::path::Path::new("assets/icons/hicolor").exists() {
                icon_theme.add_search_path("assets/icons/hicolor");
            }
        }

        let initial_icon_name = config_manager.borrow().config.app_icon_name();
        gtk4::Window::set_default_icon_name(initial_icon_name);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("WhattyTheShell! – GPU Terminal")
            .icon_name(initial_icon_name)
            .default_width(1024)
            .default_height(768)
            .build();

        // Apply saved window opacity
        window.set_opacity(config_manager.borrow().window_opacity() as f64);

        // Initialise wgpu Vulkan device & queue
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .expect("Failed to find GPU adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("whatty_the_shell_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        }))
        .expect("Failed to create wgpu device");

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        // Apply saved theme colors if configured
        if let Some(ref colors) = config_manager.borrow().theme_colors() {
            theme_manager.apply_palette(colors);
        }
        theme_manager.apply_to_window(&window);

        // Styling for tab context menus, rename popovers, and tab headers
        let popover_css_provider = gtk4::CssProvider::new();
        popover_css_provider.load_from_string("
            /* Notebook and Tab Bar Styling */
            notebook > header {
                background-color: #121212;
                border: none;
                padding: 2px 4px;
            }
            notebook > header.top {
                border-bottom: 1px solid rgba(255, 255, 255, 0.08);
            }
            notebook > header.left {
                border-right: 1px solid rgba(255, 255, 255, 0.08);
            }
            notebook > header tabs tab {
                background-color: #181818;
                color: #9e9e9e;
                font-weight: normal;
                border: 1px solid rgba(255, 255, 255, 0.08);
                border-radius: 6px;
                margin: 2px 3px;
                padding: 4px 10px;
                transition: background-color 0.15s ease, color 0.15s ease;
            }
            notebook > header tabs tab:hover {
                background-color: #252525;
                color: #d0d0d0;
                border-color: rgba(255, 255, 255, 0.15);
            }
            notebook > header tabs tab:checked {
                background-color: #3c3f41;
                color: #ffffff;
                font-weight: bold;
                border: 1px solid rgba(255, 255, 255, 0.28);
                box-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
            }
            notebook > header tabs tab:checked:hover {
                background-color: #45484c;
                color: #ffffff;
            }
            notebook > header tabs tab label {
                color: inherit;
                font-weight: inherit;
            }
            notebook > header tabs tab .tab-close-btn {
                opacity: 0.5;
                background: transparent;
                border: none;
                padding: 2px 4px;
                min-width: 16px;
                min-height: 16px;
                border-radius: 4px;
                margin-left: 6px;
            }
            notebook > header tabs tab:checked .tab-close-btn {
                opacity: 0.9;
                color: #ffffff;
            }
            notebook > header tabs tab .tab-close-btn:hover {
                opacity: 1.0;
                background-color: rgba(255, 255, 255, 0.2);
                color: #ffffff;
            }

            .tab-context-popover {
                background-color: #2b2d30;
                color: #ffffff;
                border: 1px solid rgba(255, 255, 255, 0.25);
                border-radius: 8px;
                padding: 6px;
                box-shadow: 0 4px 14px rgba(0, 0, 0, 0.5);
            }
            .tab-context-popover button {
                background: transparent;
                color: #ffffff;
                font-weight: 500;
                opacity: 1.0;
                border-radius: 6px;
                padding: 7px 12px;
                margin: 1px 0px;
                border: none;
                box-shadow: none;
            }
            .tab-context-popover button:hover {
                background-color: #3574d4;
                color: #ffffff;
            }
            .tab-context-popover button image,
            .tab-context-popover button label {
                opacity: 1.0;
                color: #ffffff;
            }
            .tab-context-popover button:hover image,
            .tab-context-popover button:hover label {
                color: #ffffff;
            }
            .tab-context-popover separator {
                background-color: rgba(255, 255, 255, 0.2);
                margin: 4px 6px;
                min-height: 1px;
            }
            .tab-context-popover entry {
                background-color: #1e1f22;
                color: #ffffff;
                border: 1px solid rgba(255, 255, 255, 0.25);
                border-radius: 6px;
                padding: 6px 10px;
            }
            .tab-context-popover entry:focus {
                border-color: #3574d4;
            }
            .icon-choice-card {
                background-color: rgba(255, 255, 255, 0.04);
                border: 1px solid rgba(255, 255, 255, 0.1);
                border-radius: 8px;
                padding: 10px 14px;
                transition: background-color 0.15s ease, border-color 0.15s ease;
            }
            .icon-choice-card:hover {
                background-color: rgba(255, 255, 255, 0.08);
                border-color: rgba(255, 255, 255, 0.25);
            }
            .active-theme-bar {
                background-color: rgba(255, 255, 255, 0.04);
                border: 1px solid rgba(255, 255, 255, 0.12);
                border-radius: 8px;
                padding: 8px 14px;
            }
            .theme-active-badge {
                background-color: #3574d4;
                color: #ffffff;
                font-size: 11px;
                font-weight: 600;
                padding: 2px 8px;
                border-radius: 10px;
            }
            .active-theme-row {
                background-color: rgba(53, 116, 212, 0.08);
                border-left: 3px solid #3574d4;
            }
        ");
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &popover_css_provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        // Create TabGroup (horizontal or vertical as configured)
        let tab_group = TabGroup::new(config_manager.borrow().tab_orientation());

        // Connect page switch to immediately grab focus on the active terminal
        let win_switch = window.clone();
        tab_group.widget().connect_switch_page(move |_, page, _| {
            page.grab_focus();
            gtk4::prelude::RootExt::set_focus(&win_switch, Some(page));
        });

        let new_tab_action = gtk4::gio::SimpleAction::new("new-tab", None);

        fn create_menu_item(label: &str, icon: &str) -> gtk4::Button {
            let btn = gtk4::Button::new();
            btn.add_css_class("flat");
            let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            let img = gtk4::Image::from_icon_name(icon);
            let lbl = gtk4::Label::new(Some(label));
            lbl.set_halign(gtk4::Align::Start);
            content.append(&img);
            content.append(&lbl);
            btn.set_child(Some(&content));
            btn
        }

        fn page_root_for_view(notebook: &gtk4::Notebook, view: &TerminalView) -> Option<(u32, gtk4::Widget)> {
            if let Some(idx) = notebook.page_num(view) {
                return Some((idx, view.clone().upcast()));
            }
            for i in 0..notebook.n_pages() {
                if let Some(page) = notebook.nth_page(Some(i)) {
                    if split::find_terminals(&page).iter().any(|t| t == view) {
                        return Some((i, page));
                    }
                }
            }
            None
        }

        fn create_terminal_view(
            device: &std::sync::Arc<wgpu::Device>,
            queue: &std::sync::Arc<wgpu::Queue>,
            config_manager: &std::rc::Rc<std::cell::RefCell<ConfigManager>>,
            working_dir: Option<&std::path::Path>,
        ) -> Result<TerminalView, Box<dyn std::error::Error>> {
            let (font_family, font_size) = {
                let c = config_manager.borrow();
                (c.font_family(), c.font_size())
            };
            let view = TerminalView::with_working_dir(
                device.clone(),
                queue.clone(),
                font_family,
                font_size,
                working_dir,
            )?;

            if let Some(ref colors) = config_manager.borrow().theme_colors() {
                let palette = crate::render::pipeline::palette_from_vscode_colors(colors);
                view.set_palette(palette);
            }

            let (initial_sat, experimental) = {
                let c = config_manager.borrow();
                (c.color_saturation(), c.experimental_supersaturation())
            };
            if experimental && initial_sat > 1.0 {
                view.set_saturation_boost(initial_sat);
            }

            Ok(view)
        }

        fn close_tab_page(
            notebook: &gtk4::Notebook,
            window: &ApplicationWindow,
            config_manager: &std::rc::Rc<std::cell::RefCell<ConfigManager>>,
            active_terminal: &std::rc::Rc<std::cell::RefCell<Option<TerminalView>>>,
            page_idx: u32,
        ) {
            if let Some(page) = notebook.nth_page(Some(page_idx)) {
                let terminals = split::find_terminals(&page);
                if let Ok(mut active) = active_terminal.try_borrow_mut() {
                    if let Some(cur) = active.as_ref() {
                        if terminals.iter().any(|t| t == cur) {
                            *active = None;
                        }
                    }
                }

                // If this was the last tab, preserve its working directory so next launch restores it
                if notebook.n_pages() <= 1 {
                    if let Some(tv) = terminals.first() {
                        let cwd = tv.current_working_dir();
                        let custom = tv.custom_title();
                        let is_custom = custom.is_some();
                        config_manager.borrow_mut().set_open_tabs(vec![crate::config::SavedTab {
                            title: custom,
                            working_dir: cwd,
                            custom_title: is_custom,
                        }]);
                    }
                }
            }

            notebook.remove_page(Some(page_idx));
            if notebook.n_pages() == 0 {
                window.close();
            } else if let Some(current) = notebook.current_page() {
                if let Some(page) = notebook.nth_page(Some(current)) {
                    let active_opt = active_terminal.try_borrow().ok().and_then(|g| g.clone());
                    if let Some(focused) = split::find_focused_terminal_with_active(&page, active_opt.as_ref()) {
                        focused.grab_focus();
                        gtk4::prelude::RootExt::set_focus(window, Some(&focused));
                    } else {
                        page.grab_focus();
                        gtk4::prelude::RootExt::set_focus(window, Some(&page));
                    }
                }
            }
        }

        fn close_pane_or_tab(
            notebook: &gtk4::Notebook,
            window: &ApplicationWindow,
            config_manager: &std::rc::Rc<std::cell::RefCell<ConfigManager>>,
            active_terminal: &std::rc::Rc<std::cell::RefCell<Option<TerminalView>>>,
            view: &TerminalView,
        ) {
            if let Ok(mut active) = active_terminal.try_borrow_mut() {
                if active.as_ref() == Some(view) {
                    *active = None;
                }
            }

            // Attempt to collapse enclosing Paned if this is a split pane
            if let Some(sibling) = split::close_pane(view, notebook) {
                if let Some(first_tv) = split::find_terminals(&sibling).first() {
                    if let Ok(mut active) = active_terminal.try_borrow_mut() {
                        *active = Some(first_tv.clone());
                    }
                }
                return;
            }

            // Otherwise, close the whole tab page
            let page_idx = page_root_for_view(notebook, view).map(|(idx, _)| idx);
            if let Some(num) = page_idx {
                close_tab_page(notebook, window, config_manager, active_terminal, num);
            }
        }

        type CreateTabFn = std::rc::Rc<
            dyn Fn(&gtk4::Notebook, &ApplicationWindow, Option<&std::path::Path>, Option<&str>),
        >;

        let create_tab_cell: std::rc::Rc<std::cell::RefCell<Option<CreateTabFn>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));

        let tab_scroll_accum = std::rc::Rc::new(std::cell::Cell::new(0.0f64));
        let last_pointer_pos = std::rc::Rc::new(std::cell::Cell::new(None::<(f64, f64)>));

        let active_terminal: std::rc::Rc<std::cell::RefCell<Option<TerminalView>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));

        let do_split = {
            let device_clone = device.clone();
            let queue_clone = queue.clone();
            let cfg_clone = config_manager.clone();
            let active_terminal_split = active_terminal.clone();
            std::rc::Rc::new(
                move |notebook: &gtk4::Notebook,
                      window: &ApplicationWindow,
                      target: Option<&TerminalView>,
                      orientation: gtk4::Orientation| -> bool {
                    let current_page = match notebook.current_page() {
                        Some(p) => p,
                        None => return false,
                    };
                    let page_widget = match notebook.nth_page(Some(current_page)) {
                        Some(w) => w,
                        None => return false,
                    };

                    let active_opt = active_terminal_split.try_borrow().ok().and_then(|g| g.clone());
                    let target_view = match target {
                        Some(t) => t.clone(),
                        None => match split::find_focused_terminal_with_active(&page_widget, active_opt.as_ref()) {
                            Some(v) => v,
                            None => return false,
                        },
                    };

                    let cwd = target_view.current_working_dir();
                    let new_view = match create_terminal_view(&device_clone, &queue_clone, &cfg_clone, cwd.as_deref()) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("Failed to create split terminal view: {e}");
                            return false;
                        }
                    };

                    let active_term_nv = active_terminal_split.clone();
                    let nv_weak = new_view.downgrade();
                    let split_focus_ctrl = gtk4::EventControllerFocus::new();
                    split_focus_ctrl.connect_enter(move |_| {
                        if let Some(v) = nv_weak.upgrade() {
                            if let Ok(mut slot) = active_term_nv.try_borrow_mut() {
                                *slot = Some(v);
                            }
                        }
                    });
                    new_view.add_controller(split_focus_ctrl);

                    let nb_close = notebook.clone();
                    let win_close = window.clone();
                    let cfg_exit = cfg_clone.clone();
                    let v_exit = new_view.clone();
                    let active_term_on_exit = active_terminal_split.clone();
                    new_view.connect_closure(
                        "child-exited",
                        false,
                        gtk4::glib::closure_local!(move |_v: TerminalView| {
                            close_pane_or_tab(&nb_close, &win_close, &cfg_exit, &active_term_on_exit, &v_exit);
                        }),
                    );

                    let ok = split::split_terminal(&target_view, &new_view, orientation, notebook);
                    if ok {
                        if let Ok(mut slot) = active_terminal_split.try_borrow_mut() {
                            *slot = Some(new_view.clone());
                        }
                        new_view.grab_focus();
                        gtk4::prelude::RootExt::set_focus(window, Some(&new_view));
                    }
                    ok
                },
            )
        };

        let create_tab_cb = {
            let create_tab_cell = create_tab_cell.clone();
            let device_clone = device.clone();
            let queue_clone = queue.clone();
            let cfg_for_tab = config_manager.clone();
            let tab_scroll_accum = tab_scroll_accum.clone();
            let do_split_cb = do_split.clone();
            let active_terminal_for_tab = active_terminal.clone();

            std::rc::Rc::new(
                move |notebook: &gtk4::Notebook,
                      window: &ApplicationWindow,
                      working_dir: Option<&std::path::Path>,
                      title: Option<&str>| {
                    if let Ok(view) = create_terminal_view(
                        &device_clone,
                        &queue_clone,
                        &cfg_for_tab,
                        working_dir,
                    ) {
                        let active_term_initial = active_terminal_for_tab.clone();
                        let v_initial_weak = view.downgrade();
                        let focus_ctrl = gtk4::EventControllerFocus::new();
                        focus_ctrl.connect_enter(move |_| {
                            if let Some(v) = v_initial_weak.upgrade() {
                                if let Ok(mut slot) = active_term_initial.try_borrow_mut() {
                                    *slot = Some(v);
                                }
                            }
                        });
                        view.add_controller(focus_ctrl);
                        if let Ok(mut slot) = active_terminal_for_tab.try_borrow_mut() {
                            *slot = Some(view.clone());
                        }

                        let initial_cwd = view.current_working_dir()
                            .or_else(|| working_dir.map(|p| p.to_path_buf()))
                            .or_else(|| std::env::current_dir().ok());

                        let (tab_title, is_custom) = match title {
                            Some(t) => (truncate_middle(t, 32), true),
                            None => {
                                let formatted = initial_cwd
                                    .as_ref()
                                    .map(|p| format_dir_path(p))
                                    .unwrap_or_else(|| "~".to_string());
                                (formatted, false)
                            }
                        };

                        if is_custom {
                            view.set_custom_title(title.map(|t| t.to_string()));
                        } else {
                            view.set_custom_title(None);
                        }

                        let last_seen_cwd = std::rc::Rc::new(std::cell::RefCell::new(initial_cwd.clone()));

                        let tab_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);

                        let label = gtk4::Label::new(Some(&tab_title));
                        if is_custom {
                            label.set_tooltip_text(title.as_deref().or(Some(&tab_title)));
                        } else if let Some(ref cwd) = initial_cwd {
                            label.set_tooltip_text(Some(&cwd.to_string_lossy()));
                        }
                        tab_box.append(&label);

                        let tab_close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
                        tab_close_btn.add_css_class("flat");
                        tab_close_btn.add_css_class("tab-close-btn");
                        tab_close_btn.set_valign(gtk4::Align::Center);
                        tab_close_btn.set_can_focus(false);
                        tab_close_btn.set_tooltip_text(Some("Fechar Aba"));
                        let nb_for_cb = notebook.clone();
                        let win_for_cb = window.clone();
                        let cfg_for_cb = cfg_for_tab.clone();
                        let tab_box_for_close = tab_box.clone();
                        let active_term_for_tab_close = active_terminal_for_tab.clone();
                        tab_close_btn.connect_clicked(move |_| {
                            if let Some((idx, _)) = split::find_page_by_tab_label(&nb_for_cb, &tab_box_for_close.clone().upcast()) {
                                close_tab_page(&nb_for_cb, &win_for_cb, &cfg_for_cb, &active_term_for_tab_close, idx);
                            }
                        });
                        tab_box.append(&tab_close_btn);

                        // Dedicated popover for tab renaming
                        let rename_popover = gtk4::Popover::new();
                        rename_popover.set_parent(&tab_box);
                        rename_popover.set_has_arrow(true);
                        rename_popover.set_position(gtk4::PositionType::Bottom);
                        rename_popover.add_css_class("tab-context-popover");

                        let rename_box = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
                        rename_box.set_margin_top(8);
                        rename_box.set_margin_bottom(8);
                        rename_box.set_margin_start(10);
                        rename_box.set_margin_end(10);

                        let rename_title = gtk4::Label::new(Some("Renomear Aba"));
                        rename_title.set_halign(gtk4::Align::Start);
                        rename_title.add_css_class("heading");

                        let rename_entry = gtk4::Entry::new();
                        rename_entry.set_width_chars(25);

                        let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                        btn_box.set_halign(gtk4::Align::End);

                        let cancel_btn = gtk4::Button::with_label("Cancelar");
                        let save_btn = gtk4::Button::with_label("Salvar");
                        save_btn.add_css_class("suggested-action");

                        btn_box.append(&cancel_btn);
                        btn_box.append(&save_btn);

                        rename_box.append(&rename_title);
                        rename_box.append(&rename_entry);
                        rename_box.append(&btn_box);

                        rename_popover.set_child(Some(&rename_box));

                        // Helper to trigger tab renaming popover
                        let start_rename = {
                            let rename_popover = rename_popover.clone();
                            let rename_entry = rename_entry.clone();
                            let label = label.clone();
                            let view = view.clone();
                            std::rc::Rc::new(move || {
                                let current_title = view.custom_title().unwrap_or_else(|| label.text().to_string());
                                rename_entry.set_text(&current_title);
                                rename_popover.popup();
                                let e = rename_entry.clone();
                                glib::idle_add_local_once(move || {
                                    e.grab_focus();
                                    e.select_region(0, -1);
                                });
                            })
                        };

                        // Finish editing on Enter or click Salvar
                        {
                            let last_seen_for_commit = last_seen_cwd.clone();
                            let commit_rename = {
                                let rename_popover = rename_popover.clone();
                                let rename_entry = rename_entry.clone();
                                let label = label.clone();
                                let view = view.clone();
                                move || {
                                    let text = rename_entry.text().trim().to_string();
                                    if !text.is_empty() {
                                        view.set_custom_title(Some(text.clone()));
                                        let display_title = truncate_middle(&text, 32);
                                        label.set_text(&display_title);
                                        label.set_tooltip_text(Some(&text));
                                    } else {
                                        view.set_custom_title(None);
                                        *last_seen_for_commit.borrow_mut() = None;
                                        let cwd = view.current_working_dir()
                                            .or_else(|| std::env::current_dir().ok());
                                        if let Some(ref path) = cwd {
                                            *last_seen_for_commit.borrow_mut() = Some(path.clone());
                                            let formatted = format_dir_path(path);
                                            label.set_text(&formatted);
                                            label.set_tooltip_text(Some(&path.to_string_lossy()));
                                        }
                                    }
                                    rename_popover.popdown();
                                    view.grab_focus();
                                }
                            };

                            let commit_for_activate = commit_rename.clone();
                            rename_entry.connect_activate(move |_| {
                                commit_for_activate();
                            });

                            let commit_for_btn = commit_rename;
                            save_btn.connect_clicked(move |_| {
                                commit_for_btn();
                            });
                        }

                        // Cancel editing on Escape or click Cancelar
                        {
                            let rename_popover = rename_popover.clone();
                            let view = view.clone();
                            let cancel_rename = move || {
                                rename_popover.popdown();
                                view.grab_focus();
                            };

                            let cancel_for_btn = cancel_rename.clone();
                            cancel_btn.connect_clicked(move |_| {
                                cancel_for_btn();
                            });

                            let key_controller = gtk4::EventControllerKey::new();
                            let cancel_for_key = cancel_rename;
                            key_controller.connect_key_pressed(move |_, keyval, _, _| {
                                if keyval == gtk4::gdk::Key::Escape {
                                    cancel_for_key();
                                    return glib::Propagation::Stop;
                                }
                                glib::Propagation::Proceed
                            });
                            rename_entry.add_controller(key_controller);
                        }

                        // Popover Context Menu for tab header
                        let popover = gtk4::Popover::new();
                        popover.set_parent(&tab_box);
                        popover.set_has_arrow(true);
                        popover.add_css_class("tab-context-popover");

                        let menu_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
                        menu_box.set_margin_top(4);
                        menu_box.set_margin_bottom(4);
                        menu_box.set_margin_start(4);
                        menu_box.set_margin_end(4);

                        // 1. Renomear Aba...
                        let rename_btn = create_menu_item("Renomear Aba...", "edit-rename-symbolic");
                        let pop_rename = popover.clone();
                        let start_rename_for_btn = start_rename.clone();
                        rename_btn.connect_clicked(move |_| {
                            pop_rename.popdown();
                            start_rename_for_btn();
                        });
                        menu_box.append(&rename_btn);

                        // 2. Restaurar Nome Automático (Caminho da Pasta)
                        let restore_name_btn = create_menu_item("Restaurar Nome Automático", "view-refresh-symbolic");
                        restore_name_btn.set_tooltip_text(Some("Voltar a exibir automaticamente o caminho da pasta"));
                        let pop_restore = popover.clone();
                        let view_for_restore = view.clone();
                        let label_for_restore = label.clone();
                        let last_seen_for_restore = last_seen_cwd.clone();
                        restore_name_btn.connect_clicked(move |_| {
                            pop_restore.popdown();
                            view_for_restore.set_custom_title(None);
                            *last_seen_for_restore.borrow_mut() = None;
                            let cwd = view_for_restore.current_working_dir()
                                .or_else(|| std::env::current_dir().ok());
                            if let Some(ref path) = cwd {
                                *last_seen_for_restore.borrow_mut() = Some(path.clone());
                                let formatted = format_dir_path(path);
                                label_for_restore.set_text(&formatted);
                                label_for_restore.set_tooltip_text(Some(&path.to_string_lossy()));
                            }
                            view_for_restore.grab_focus();
                        });
                        menu_box.append(&restore_name_btn);

                        // 3. Duplicar Aba
                        let dup_btn = create_menu_item("Duplicar Aba", "tab-new-symbolic");
                        let pop_dup = popover.clone();
                        let nb_for_dup = notebook.clone();
                        let win_for_dup = window.clone();
                        let view_for_dup = view.clone();
                        let create_tab_cell_dup = create_tab_cell.clone();
                        let label_for_dup = label.clone();
                        dup_btn.connect_clicked(move |_| {
                            pop_dup.popdown();
                            let cwd = view_for_dup.current_working_dir();
                            let dup_title = format!("{} (cópia)", label_for_dup.text());
                            if let Some(ref cb) = *create_tab_cell_dup.borrow() {
                                cb(&nb_for_dup, &win_for_dup, cwd.as_deref(), Some(&dup_title));
                            }
                        });
                        menu_box.append(&dup_btn);

                        // 3. Mover Aba para Esquerda / Cima
                        let is_vert = matches!(notebook.tab_pos(), gtk4::PositionType::Left | gtk4::PositionType::Right);
                        let move_prev_btn = create_menu_item(
                            if is_vert { "Mover Aba para Cima" } else { "Mover Aba para a Esquerda" },
                            if is_vert { "go-up-symbolic" } else { "go-previous-symbolic" },
                        );
                        let pop_move_prev = popover.clone();
                        let nb_for_move_prev = notebook.clone();
                        let view_for_move_prev = view.clone();
                        move_prev_btn.connect_clicked(move |_| {
                            pop_move_prev.popdown();
                            if let Some((idx, root)) = page_root_for_view(&nb_for_move_prev, &view_for_move_prev) {
                                if idx > 0 {
                                    nb_for_move_prev.reorder_child(&root, Some(idx - 1));
                                    nb_for_move_prev.set_current_page(Some(idx - 1));
                                    view_for_move_prev.grab_focus();
                                }
                            }
                        });
                        menu_box.append(&move_prev_btn);

                        // 4. Mover Aba para Direita / Baixo
                        let move_next_btn = create_menu_item(
                            if is_vert { "Mover Aba para Baixo" } else { "Mover Aba para a Direita" },
                            if is_vert { "go-down-symbolic" } else { "go-next-symbolic" },
                        );
                        let pop_move_next = popover.clone();
                        let nb_for_move_next = notebook.clone();
                        let tab_box_for_move_next = tab_box.clone();
                        move_next_btn.connect_clicked(move |_| {
                            pop_move_next.popdown();
                            if let Some((idx, root)) = split::find_page_by_tab_label(&nb_for_move_next, &tab_box_for_move_next.clone().upcast()) {
                                if idx + 1 < nb_for_move_next.n_pages() {
                                    nb_for_move_next.reorder_child(&root, Some(idx + 1));
                                    nb_for_move_next.set_current_page(Some(idx + 1));
                                    if let Some(target) = split::find_focused_terminal(&root) {
                                        target.grab_focus();
                                    }
                                }
                            }
                        });
                        menu_box.append(&move_next_btn);

                        // 5. Copiar Caminho do Diretório
                        let copy_path_btn = create_menu_item("Copiar Caminho do Diretório", "edit-copy-symbolic");
                        let pop_copy = popover.clone();
                        let nb_for_copy = notebook.clone();
                        let tab_box_for_copy = tab_box.clone();
                        let active_term_for_copy = active_terminal_for_tab.clone();
                        copy_path_btn.connect_clicked(move |_| {
                            pop_copy.popdown();
                            if let Some((_, root)) = split::find_page_by_tab_label(&nb_for_copy, &tab_box_for_copy.clone().upcast()) {
                                let active_opt = active_term_for_copy.try_borrow().ok().and_then(|g| g.clone());
                                if let Some(target) = split::find_focused_terminal_with_active(&root, active_opt.as_ref()) {
                                    if let Some(cwd) = target.current_working_dir() {
                                        if let Some(display) = gtk4::gdk::Display::default() {
                                            let path_str = cwd.to_string_lossy().to_string();
                                            display.clipboard().set_text(&path_str);
                                            display.primary_clipboard().set_text(&path_str);
                                        }
                                    }
                                }
                            }
                        });
                        menu_box.append(&copy_path_btn);

                        // Separador
                        menu_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

                        // Dividir à Direita
                        let split_r_btn = create_menu_item("Dividir à Direita", "view-split-left-right-symbolic");
                        let pop_split_r = popover.clone();
                        let do_split_tab_r = do_split_cb.clone();
                        let nb_tab_split_r = notebook.clone();
                        let win_tab_split_r = window.clone();
                        let tab_box_for_split_r = tab_box.clone();
                        let active_term_split_r = active_terminal_for_tab.clone();
                        split_r_btn.connect_clicked(move |_| {
                            pop_split_r.popdown();
                            if let Some((_, root)) = split::find_page_by_tab_label(&nb_tab_split_r, &tab_box_for_split_r.clone().upcast()) {
                                let active_opt = active_term_split_r.try_borrow().ok().and_then(|g| g.clone());
                                let target = split::find_focused_terminal_with_active(&root, active_opt.as_ref());
                                do_split_tab_r(&nb_tab_split_r, &win_tab_split_r, target.as_ref(), gtk4::Orientation::Horizontal);
                            }
                        });
                        menu_box.append(&split_r_btn);

                        // Dividir Abaixo
                        let split_d_btn = create_menu_item("Dividir Abaixo", "view-split-top-bottom-symbolic");
                        let pop_split_d = popover.clone();
                        let do_split_tab_d = do_split_cb.clone();
                        let nb_tab_split_d = notebook.clone();
                        let win_tab_split_d = window.clone();
                        let tab_box_for_split_d = tab_box.clone();
                        let active_term_split_d = active_terminal_for_tab.clone();
                        split_d_btn.connect_clicked(move |_| {
                            pop_split_d.popdown();
                            if let Some((_, root)) = split::find_page_by_tab_label(&nb_tab_split_d, &tab_box_for_split_d.clone().upcast()) {
                                let active_opt = active_term_split_d.try_borrow().ok().and_then(|g| g.clone());
                                let target = split::find_focused_terminal_with_active(&root, active_opt.as_ref());
                                do_split_tab_d(&nb_tab_split_d, &win_tab_split_d, target.as_ref(), gtk4::Orientation::Vertical);
                            }
                        });
                        menu_box.append(&split_d_btn);

                        // Desfazer Divisão
                        let unsplit_btn = create_menu_item("Desfazer Divisão", "view-restore-symbolic");
                        let pop_unsplit = popover.clone();
                        let nb_unsplit = notebook.clone();
                        let tab_box_for_unsplit = tab_box.clone();
                        let active_term_unsplit = active_terminal_for_tab.clone();
                        unsplit_btn.connect_clicked(move |_| {
                            pop_unsplit.popdown();
                            if let Some((_, root)) = split::find_page_by_tab_label(&nb_unsplit, &tab_box_for_unsplit.clone().upcast()) {
                                let active_opt = active_term_unsplit.try_borrow().ok().and_then(|g| g.clone());
                                if let Some(target) = split::find_focused_terminal_with_active(&root, active_opt.as_ref()) {
                                    split::unsplit_terminal(&target, &nb_unsplit);
                                }
                            }
                        });
                        menu_box.append(&unsplit_btn);

                        // Fechar Painel Ativo
                        let close_pane_btn = create_menu_item("Fechar Painel Ativo", "window-close-symbolic");
                        let pop_close_pane = popover.clone();
                        let nb_close_p = notebook.clone();
                        let win_close_p = window.clone();
                        let cfg_close_p = cfg_for_tab.clone();
                        let tab_box_for_close_p = tab_box.clone();
                        let active_term_close_p = active_terminal_for_tab.clone();
                        close_pane_btn.connect_clicked(move |_| {
                            pop_close_pane.popdown();
                            if let Some((_, root)) = split::find_page_by_tab_label(&nb_close_p, &tab_box_for_close_p.clone().upcast()) {
                                let active_opt = active_term_close_p.try_borrow().ok().and_then(|g| g.clone());
                                if let Some(target) = split::find_focused_terminal_with_active(&root, active_opt.as_ref()) {
                                    close_pane_or_tab(&nb_close_p, &win_close_p, &cfg_close_p, &active_term_close_p, &target);
                                }
                            }
                        });
                        menu_box.append(&close_pane_btn);

                        // Mesclar com Aba Anterior ao Lado
                        let merge_p_btn = create_menu_item("Mesclar com Aba Anterior", "insert-object-symbolic");
                        let pop_merge_p = popover.clone();
                        let nb_merge_p = notebook.clone();
                        let tab_box_for_merge_p = tab_box.clone();
                        merge_p_btn.connect_clicked(move |_| {
                            pop_merge_p.popdown();
                            if let Some((idx, _)) = split::find_page_by_tab_label(&nb_merge_p, &tab_box_for_merge_p.clone().upcast()) {
                                if idx > 0 {
                                    split::merge_tabs(idx - 1, idx, gtk4::Orientation::Horizontal, &nb_merge_p);
                                }
                            }
                        });
                        menu_box.append(&merge_p_btn);

                        // Mesclar com Próxima Aba ao Lado
                        let merge_n_btn = create_menu_item("Mesclar com Próxima Aba", "insert-object-symbolic");
                        let pop_merge_n = popover.clone();
                        let nb_merge_n = notebook.clone();
                        let tab_box_for_merge_n = tab_box.clone();
                        merge_n_btn.connect_clicked(move |_| {
                            pop_merge_n.popdown();
                            if let Some((idx, _)) = split::find_page_by_tab_label(&nb_merge_n, &tab_box_for_merge_n.clone().upcast()) {
                                if idx + 1 < nb_merge_n.n_pages() {
                                    split::merge_tabs(idx + 1, idx, gtk4::Orientation::Horizontal, &nb_merge_n);
                                }
                            }
                        });
                        menu_box.append(&merge_n_btn);

                        // Separador
                        menu_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

                        // 6. Fechar Aba
                        let close_btn = create_menu_item("Fechar Aba", "window-close-symbolic");
                        let pop_close = popover.clone();
                        let nb_for_close_btn = notebook.clone();
                        let win_for_close_btn = window.clone();
                        let cfg_for_close_btn = cfg_for_tab.clone();
                        let tab_box_for_close_btn = tab_box.clone();
                        let active_term_for_close_btn = active_terminal_for_tab.clone();
                        close_btn.connect_clicked(move |_| {
                            pop_close.popdown();
                            if let Some((idx, _)) = split::find_page_by_tab_label(&nb_for_close_btn, &tab_box_for_close_btn.clone().upcast()) {
                                close_tab_page(
                                    &nb_for_close_btn,
                                    &win_for_close_btn,
                                    &cfg_for_close_btn,
                                    &active_term_for_close_btn,
                                    idx,
                                );
                            }
                        });
                        menu_box.append(&close_btn);

                        // 7. Fechar Outras Abas
                        let close_others_btn = create_menu_item("Fechar Outras Abas", "edit-clear-symbolic");
                        let pop_close_others = popover.clone();
                        let nb_for_close_others = notebook.clone();
                        let tab_box_for_close_others = tab_box.clone();
                        let active_term_close_others = active_terminal_for_tab.clone();
                        close_others_btn.connect_clicked(move |_| {
                            pop_close_others.popdown();
                            let mut i = 0;
                            while i < nb_for_close_others.n_pages() {
                                if let Some(page) = nb_for_close_others.nth_page(Some(i)) {
                                    let is_this = nb_for_close_others.tab_label(&page).as_ref() == Some(&tab_box_for_close_others.clone().upcast());
                                    if is_this {
                                        i += 1;
                                    } else {
                                        nb_for_close_others.remove_page(Some(i));
                                    }
                                } else {
                                    break;
                                }
                            }
                            if let Some(cur) = nb_for_close_others.current_page() {
                                if let Some(page) = nb_for_close_others.nth_page(Some(cur)) {
                                    let terms = split::find_terminals(&page);
                                    if let Some(first) = terms.first() {
                                        if let Ok(mut active) = active_term_close_others.try_borrow_mut() {
                                            *active = Some(first.clone());
                                        }
                                        first.grab_focus();
                                    }
                                }
                            }
                        });
                        menu_box.append(&close_others_btn);

                        popover.set_child(Some(&menu_box));

                        // Right Click gesture to open Popover
                        let tab_right_click = gtk4::GestureClick::new();
                        tab_right_click.set_button(gtk4::gdk::BUTTON_SECONDARY);
                        let pop_trigger = popover.clone();
                        let nb_for_right_click = notebook.clone();
                        let tab_box_for_right_click = tab_box.clone();
                        let view_for_right_click = view.clone();
                        let btn_p = move_prev_btn.clone();
                        let btn_n = move_next_btn.clone();
                        let btn_merge_p = merge_p_btn.clone();
                        let btn_merge_n = merge_n_btn.clone();
                        let btn_split_r = split_r_btn.clone();
                        let btn_split_d = split_d_btn.clone();
                        let btn_unsplit = unsplit_btn.clone();
                        let btn_close_pane = close_pane_btn.clone();
                        let btn_restore = restore_name_btn.clone();
                        tab_right_click.connect_pressed(move |gesture, _, _, _| {
                            gesture.set_state(gtk4::EventSequenceState::Claimed);
                            if let Some((idx, root)) = split::find_page_by_tab_label(&nb_for_right_click, &tab_box_for_right_click.clone().upcast()) {
                                btn_p.set_sensitive(idx > 0);
                                btn_n.set_sensitive(idx + 1 < nb_for_right_click.n_pages());
                                btn_merge_p.set_sensitive(idx > 0);
                                btn_merge_n.set_sensitive(idx + 1 < nb_for_right_click.n_pages());
                                let count = split::find_terminals(&root).len();
                                btn_split_r.set_sensitive(count < split::MAX_PANES_PER_TAB);
                                btn_split_d.set_sensitive(count < split::MAX_PANES_PER_TAB);
                                btn_unsplit.set_sensitive(count > 1);
                                btn_close_pane.set_sensitive(count > 1);
                            }
                            let is_custom = view_for_right_click.custom_title().is_some();
                            btn_restore.set_sensitive(is_custom);
                            pop_trigger.popup();
                        });
                        tab_box.add_controller(tab_right_click);

                        // Left Click gesture: Ctrl+Click rename, Double click close, single click select
                        let tab_left_click = gtk4::GestureClick::new();
                        tab_left_click.set_button(gtk4::gdk::BUTTON_PRIMARY);
                        let nb_for_tab_click = notebook.clone();
                        let win_for_tab_click = window.clone();
                        let cfg_for_tab_click = cfg_for_tab.clone();
                        let tab_box_for_tab_click = tab_box.clone();
                        let active_term_tab_click = active_terminal_for_tab.clone();
                        let start_rename_for_click = start_rename.clone();
                        tab_left_click.connect_pressed(move |gesture, n_press, _x, _y| {
                            let state = gesture.current_event_state();
                            if state.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                                gesture.set_state(gtk4::EventSequenceState::Claimed);
                                start_rename_for_click();
                                return;
                            }

                            if n_press == 1 {
                                if let Some((page_idx, page)) = split::find_page_by_tab_label(&nb_for_tab_click, &tab_box_for_tab_click.clone().upcast()) {
                                    nb_for_tab_click.set_current_page(Some(page_idx));
                                    let active_opt = active_term_tab_click.try_borrow().ok().and_then(|g| g.clone());
                                    if let Some(target) = split::find_focused_terminal_with_active(&page, active_opt.as_ref()) {
                                        target.grab_focus();
                                    }
                                }
                            } else if n_press == 2 {
                                if !cfg_for_tab_click.borrow().double_click_close_tab() {
                                    return;
                                }
                                gesture.set_state(gtk4::EventSequenceState::Claimed);
                                if let Some((idx, _)) = split::find_page_by_tab_label(&nb_for_tab_click, &tab_box_for_tab_click.clone().upcast()) {
                                    close_tab_page(
                                        &nb_for_tab_click,
                                        &win_for_tab_click,
                                        &cfg_for_tab_click,
                                        &active_term_tab_click,
                                        idx,
                                    );
                                }
                            }
                        });
                        tab_box.add_controller(tab_left_click);

                        // Smooth lateral / vertical touchpad scroll over tab header
                        let tab_scroll = gtk4::EventControllerScroll::new(
                            gtk4::EventControllerScrollFlags::BOTH_AXES,
                        );
                        let nb_for_tab_scroll = notebook.clone();
                        let accum_for_tab_scroll = tab_scroll_accum.clone();
                        tab_scroll.connect_scroll(move |ctrl, dx, dy| {
                            handle_smooth_tab_scroll(ctrl, dx, dy, &nb_for_tab_scroll, &accum_for_tab_scroll)
                        });
                        let accum_for_tab_end = tab_scroll_accum.clone();
                        tab_scroll.connect_scroll_end(move |_| {
                            accum_for_tab_end.set(0.0);
                        });
                        tab_box.add_controller(tab_scroll);

                        let idx = notebook.append_page(&view, Some(&tab_box));
                        notebook.set_tab_reorderable(&view, true);
                        notebook.set_current_page(Some(idx));

                        // Immediately focus terminal view and cursor
                        view.grab_focus();
                        gtk4::prelude::RootExt::set_focus(window, Some(&view));

                        let nb_clone = notebook.clone();
                        let win_clone = window.clone();
                        let cfg_for_exit = cfg_for_tab.clone();
                        let v_clone = view.clone();
                        let active_term_exit = active_terminal_for_tab.clone();
                        view.connect_closure(
                            "child-exited",
                            false,
                            gtk4::glib::closure_local!(move |_v: TerminalView| {
                                close_pane_or_tab(&nb_clone, &win_clone, &cfg_for_exit, &active_term_exit, &v_clone);
                            }),
                        );

                        // Auto-update tab title to current working dir when not custom titled
                        let label_for_auto = label.clone();
                        let nb_for_auto = notebook.clone();
                        let last_seen_for_auto = last_seen_cwd.clone();
                        let tab_box_for_auto = tab_box.clone();
                        let active_terminal_for_auto = active_terminal_for_tab.clone();
                        glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                            let (_, page) = match split::find_page_by_tab_label(&nb_for_auto, &tab_box_for_auto.clone().upcast()) {
                                Some(res) => res,
                                None => return glib::ControlFlow::Break,
                            };

                            let active_opt = active_terminal_for_auto.try_borrow().ok().and_then(|g| g.clone());
                            let cur_view = split::find_focused_terminal_with_active(&page, active_opt.as_ref())
                                .or_else(|| split::find_terminals(&page).first().cloned());

                            if let Some(v) = cur_view {
                                if v.custom_title().is_none() {
                                    if let Some(cwd) = v.current_working_dir() {
                                        let mut last = last_seen_for_auto.borrow_mut();
                                        if last.as_ref() != Some(&cwd) {
                                            *last = Some(cwd.clone());
                                            let title = format_dir_path(&cwd);
                                            label_for_auto.set_text(&title);
                                            label_for_auto.set_tooltip_text(Some(&cwd.to_string_lossy()));
                                        }
                                    }
                                }
                            }

                            glib::ControlFlow::Continue
                        });
                    }
                },
            )
        };
        *create_tab_cell.borrow_mut() = Some(create_tab_cb.clone());
        let create_tab = create_tab_cb;

        // Restore saved tabs or create the primary terminal view
        let saved_tabs = config_manager.borrow().open_tabs();
        if saved_tabs.is_empty() {
            create_tab(tab_group.widget(), &window, None, None);
        } else {
            for tab in saved_tabs {
                let cwd = tab.working_dir.as_deref();
                let title = if tab.custom_title {
                    tab.title.as_deref()
                } else {
                    None
                };
                create_tab(tab_group.widget(), &window, cwd, title);
            }
        }

        // Restore active tab index if saved
        if let Some(active_idx) = config_manager.borrow().active_tab_index() {
            let n_pages = tab_group.widget().n_pages();
            if (active_idx as u32) < n_pages {
                tab_group.widget().set_current_page(Some(active_idx as u32));
            }
        }

        let nb_clone_for_action = tab_group.widget().clone();
        let win_clone_for_action = window.clone();
        let create_tab_action = create_tab.clone();
        new_tab_action.connect_activate(move |_, _| {
            // New tab opens in the working directory of the currently active tab
            let active_cwd = nb_clone_for_action
                .current_page()
                .and_then(|p| nb_clone_for_action.nth_page(Some(p)))
                .and_then(|page| split::find_focused_terminal(&page))
                .and_then(|view| view.current_working_dir());

            create_tab_action(
                &nb_clone_for_action,
                &win_clone_for_action,
                active_cwd.as_deref(),
                None,
            );
        });
        window.add_action(&new_tab_action);
        app.set_accels_for_action("win.new-tab", &["<Ctrl><Shift>t", "<Ctrl><Shift>Tab"]);

        // Split Panes Actions and Shortcuts
        let split_right_action = gtk4::gio::SimpleAction::new("split-right", None);
        let nb_split_r = tab_group.widget().clone();
        let win_split_r = window.clone();
        let do_split_r = do_split.clone();
        split_right_action.connect_activate(move |_, _| {
            do_split_r(&nb_split_r, &win_split_r, None, gtk4::Orientation::Horizontal);
        });
        window.add_action(&split_right_action);
        app.set_accels_for_action("win.split-right", &["<Ctrl><Shift>e", "<Ctrl><Shift>d"]);

        let split_down_action = gtk4::gio::SimpleAction::new("split-down", None);
        let nb_split_d = tab_group.widget().clone();
        let win_split_d = window.clone();
        let do_split_d = do_split.clone();
        split_down_action.connect_activate(move |_, _| {
            do_split_d(&nb_split_d, &win_split_d, None, gtk4::Orientation::Vertical);
        });
        window.add_action(&split_down_action);
        app.set_accels_for_action("win.split-down", &["<Ctrl><Shift>o", "<Ctrl><Shift>s"]);

        let unsplit_action = gtk4::gio::SimpleAction::new("unsplit", None);
        let nb_unsplit_act = tab_group.widget().clone();
        let active_unsplit_act = active_terminal.clone();
        unsplit_action.connect_activate(move |_, _| {
            let active_opt = active_unsplit_act.try_borrow().ok().and_then(|g| g.clone());
            split::unsplit_active_tab(&nb_unsplit_act, active_opt.as_ref());
        });
        window.add_action(&unsplit_action);
        app.set_accels_for_action("win.unsplit", &["<Ctrl><Shift>u"]);

        let close_pane_action = gtk4::gio::SimpleAction::new("close-pane", None);
        let nb_close_pane = tab_group.widget().clone();
        let win_close_pane = window.clone();
        let cfg_close_pane = config_manager.clone();
        let active_term_close_pane = active_terminal.clone();
        close_pane_action.connect_activate(move |_, _| {
            if let Some(cur) = nb_close_pane.current_page() {
                if let Some(page) = nb_close_pane.nth_page(Some(cur)) {
                    let active_opt = active_term_close_pane.try_borrow().ok().and_then(|g| g.clone());
                    if let Some(focused) = split::find_focused_terminal_with_active(&page, active_opt.as_ref()) {
                        close_pane_or_tab(&nb_close_pane, &win_close_pane, &cfg_close_pane, &active_term_close_pane, &focused);
                    }
                }
            }
        });
        window.add_action(&close_pane_action);
        app.set_accels_for_action("win.close-pane", &["<Ctrl><Shift>w"]);

        // Reorganize tabs actions and shortcuts
        let nb_move_l = tab_group.widget().clone();
        let move_tab_left_action = gtk4::gio::SimpleAction::new("move-tab-left", None);
        move_tab_left_action.connect_activate(move |_, _| {
            if let Some(cur) = nb_move_l.current_page() {
                if cur > 0 {
                    if let Some(page) = nb_move_l.nth_page(Some(cur)) {
                        nb_move_l.reorder_child(&page, Some(cur - 1));
                        nb_move_l.set_current_page(Some(cur - 1));
                        if let Some(focused) = split::find_focused_terminal(&page) {
                            focused.grab_focus();
                        } else {
                            page.grab_focus();
                        }
                    }
                }
            }
        });
        window.add_action(&move_tab_left_action);
        app.set_accels_for_action("win.move-tab-left", &["<Ctrl><Shift>Page_Up", "<Ctrl><Shift>Prior"]);

        let nb_move_r = tab_group.widget().clone();
        let move_tab_right_action = gtk4::gio::SimpleAction::new("move-tab-right", None);
        move_tab_right_action.connect_activate(move |_, _| {
            if let Some(cur) = nb_move_r.current_page() {
                if cur + 1 < nb_move_r.n_pages() {
                    if let Some(page) = nb_move_r.nth_page(Some(cur)) {
                        nb_move_r.reorder_child(&page, Some(cur + 1));
                        nb_move_r.set_current_page(Some(cur + 1));
                        if let Some(focused) = split::find_focused_terminal(&page) {
                            focused.grab_focus();
                        } else {
                            page.grab_focus();
                        }
                    }
                }
            }
        });
        window.add_action(&move_tab_right_action);
        app.set_accels_for_action("win.move-tab-right", &["<Ctrl><Shift>Page_Down", "<Ctrl><Shift>Next"]);

        // Switch tab actions and shortcuts
        let nb_prev_t = tab_group.widget().clone();
        let prev_tab_action = gtk4::gio::SimpleAction::new("prev-tab", None);
        prev_tab_action.connect_activate(move |_, _| {
            let n = nb_prev_t.n_pages();
            if n > 1 {
                let cur = nb_prev_t.current_page().unwrap_or(0);
                let prev = if cur == 0 { n - 1 } else { cur - 1 };
                nb_prev_t.set_current_page(Some(prev));
                if let Some(page) = nb_prev_t.nth_page(Some(prev)) {
                    if let Some(focused) = split::find_focused_terminal(&page) {
                        focused.grab_focus();
                    } else {
                        page.grab_focus();
                    }
                }
            }
        });
        window.add_action(&prev_tab_action);
        app.set_accels_for_action("win.prev-tab", &["<Ctrl>Page_Up", "<Ctrl>Prior"]);

        let nb_next_t = tab_group.widget().clone();
        let next_tab_action = gtk4::gio::SimpleAction::new("next-tab", None);
        next_tab_action.connect_activate(move |_, _| {
            let n = nb_next_t.n_pages();
            if n > 1 {
                let cur = nb_next_t.current_page().unwrap_or(0);
                let next = (cur + 1) % n;
                nb_next_t.set_current_page(Some(next));
                if let Some(page) = nb_next_t.nth_page(Some(next)) {
                    if let Some(focused) = split::find_focused_terminal(&page) {
                        focused.grab_focus();
                    } else {
                        page.grab_focus();
                    }
                }
            }
        });
        window.add_action(&next_tab_action);
        app.set_accels_for_action("win.next-tab", &["<Ctrl>Page_Down", "<Ctrl>Next"]);

        // Preferences / Settings Action
        let preferences_action = gtk4::gio::SimpleAction::new("preferences", None);
        let win_pref = window.clone();
        let nb_pref = tab_group.widget().clone();
        let tm_pref = theme_manager.clone();
        let cfg_pref = config_manager.clone();
        preferences_action.connect_activate(move |_, _| {
            settings_dialog::open_settings_dialog(&win_pref, &nb_pref, &tm_pref, cfg_pref.clone());
        });
        window.add_action(&preferences_action);
        app.set_accels_for_action("win.preferences", &["<Ctrl>comma"]);

        // Save open tabs on window close
        let nb_for_close = tab_group.widget().clone();
        let cfg_for_close = config_manager.clone();
        window.connect_close_request(move |_| {
            let n_pages = nb_for_close.n_pages();
            let mut tabs_to_save = Vec::new();
            for i in 0..n_pages {
                if let Some(page) = nb_for_close.nth_page(Some(i)) {
                    let terminals = split::find_terminals(&page);
                    if let Some(view) = terminals.first() {
                        let cwd = view.current_working_dir();
                        let custom = view.custom_title();
                        let is_custom = custom.is_some();
                        let title = custom.or_else(|| {
                            nb_for_close.tab_label(&page).and_then(|tl| {
                                if let Some(l) = tl.downcast_ref::<gtk4::Label>() {
                                    Some(l.text().to_string())
                                } else if let Some(first) = tl.first_child() {
                                    first.downcast_ref::<gtk4::Label>().map(|l| l.text().to_string())
                                } else {
                                    None
                                }
                            })
                        });
                        tabs_to_save.push(crate::config::SavedTab {
                            title,
                            working_dir: cwd,
                            custom_title: is_custom,
                        });
                    }
                }
            }
            if !tabs_to_save.is_empty() {
                cfg_for_close.borrow_mut().set_open_tabs(tabs_to_save);
            }
            let active_idx = nb_for_close.current_page();
            cfg_for_close.borrow_mut().set_active_tab_index(active_idx.map(|idx| idx as usize));
            gtk4::glib::Propagation::Proceed
        });

        // Window-level scroll controller for Ctrl + Mouse Wheel opacity
        let win_scroll = gtk4::EventControllerScroll::new(
            gtk4::EventControllerScrollFlags::VERTICAL | gtk4::EventControllerScrollFlags::DISCRETE,
        );
        win_scroll.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let win_for_scroll = window.clone();
        let cfg_for_scroll = config_manager.clone();
        let nb_for_scroll = tab_group.widget().clone();

        win_scroll.connect_scroll(move |controller, _, dy| {
            let state = controller.current_event_state();
            if state.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                let (ctrl_scroll, experimental) = {
                    let c = cfg_for_scroll.borrow();
                    (c.ctrl_scroll_opacity(), c.experimental_supersaturation())
                };
                if !ctrl_scroll {
                    return glib::Propagation::Proceed;
                }

                let (current_opacity, current_sat) = {
                    let c = cfg_for_scroll.borrow();
                    (c.window_opacity(), c.color_saturation())
                };

                let current_level = if experimental && current_sat > 1.0 {
                    current_sat
                } else {
                    current_opacity
                };

                let delta = if dy < 0.0 { 0.05f32 } else { -0.05f32 };
                let max_level = if experimental { 2.0f32 } else { 1.0f32 };
                let new_level = (current_level + delta).clamp(0.20f32, max_level);

                if new_level <= 1.0 {
                    win_for_scroll.set_opacity(new_level as f64);
                    for i in 0..nb_for_scroll.n_pages() {
                        if let Some(page) = nb_for_scroll.nth_page(Some(i)) {
                            for tv in split::find_terminals(&page) {
                                tv.set_saturation_boost(1.0);
                            }
                        }
                    }
                    let mut c = cfg_for_scroll.borrow_mut();
                    c.set_window_opacity(new_level);
                    c.set_color_saturation(1.0);
                } else {
                    win_for_scroll.set_opacity(1.0);
                    for i in 0..nb_for_scroll.n_pages() {
                        if let Some(page) = nb_for_scroll.nth_page(Some(i)) {
                            for tv in split::find_terminals(&page) {
                                tv.set_saturation_boost(new_level);
                            }
                        }
                    }
                    let mut c = cfg_for_scroll.borrow_mut();
                    c.set_window_opacity(1.0);
                    c.set_color_saturation(new_level);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(win_scroll);

        // Double click empty space beside tabs to open new tab
        let tab_nb_click = gtk4::GestureClick::new();
        tab_nb_click.set_button(gtk4::gdk::BUTTON_PRIMARY);
        let nb_for_new_tab = tab_group.widget().clone();
        let win_for_new_tab = window.clone();
        let create_tab_for_nb_click = create_tab.clone();
        tab_nb_click.connect_pressed(move |gesture, n_press, x, y| {
            if n_press == 2 {
                let pos = nb_for_new_tab.tab_pos();
                let n_pages = nb_for_new_tab.n_pages();
                if n_pages == 0 {
                    gesture.set_state(gtk4::EventSequenceState::Claimed);
                    create_tab_for_nb_click(&nb_for_new_tab, &win_for_new_tab, None, None);
                    return;
                }

                let mut in_tab = false;
                let mut max_tab_y = 0.0f32;
                let mut max_tab_x = 0.0f32;

                for i in 0..n_pages {
                    if let Some(page) = nb_for_new_tab.nth_page(Some(i)) {
                        if let Some(tab_label) = nb_for_new_tab.tab_label(&page) {
                            let target = tab_label.parent().unwrap_or(tab_label);
                            if let Some(rect) = target.compute_bounds(&nb_for_new_tab) {
                                let rx = rect.x();
                                let ry = rect.y();
                                let rw = rect.width();
                                let rh = rect.height();

                                max_tab_y = max_tab_y.max(ry + rh);
                                max_tab_x = max_tab_x.max(rx + rw);

                                let px = x as f32;
                                let py = y as f32;
                                if px >= rx - 2.0 && px <= rx + rw + 2.0 && py >= ry - 2.0 && py <= ry + rh + 2.0 {
                                    in_tab = true;
                                    break;
                                }
                            }
                        }
                    }
                }

                if !in_tab {
                    let px = x as f32;
                    let py = y as f32;
                    let is_in_tab_bar = match pos {
                        gtk4::PositionType::Top => py >= 0.0 && py <= max_tab_y + 10.0,
                        gtk4::PositionType::Left => px >= 0.0 && px <= max_tab_x + 10.0,
                        gtk4::PositionType::Bottom => {
                            let h = nb_for_new_tab.height() as f32;
                            py >= h - (max_tab_y + 10.0) && py <= h
                        }
                        gtk4::PositionType::Right => {
                            let w = nb_for_new_tab.width() as f32;
                            px >= w - (max_tab_x + 10.0) && px <= w
                        }
                        _ => false,
                    };

                    if is_in_tab_bar {
                        gesture.set_state(gtk4::EventSequenceState::Claimed);
                        let active_cwd = nb_for_new_tab
                            .current_page()
                            .and_then(|p| nb_for_new_tab.nth_page(Some(p)))
                            .and_then(|p| split::find_focused_terminal(&p))
                            .and_then(|v| v.current_working_dir());

                        create_tab_for_nb_click(
                            &nb_for_new_tab,
                            &win_for_new_tab,
                            active_cwd.as_deref(),
                            None,
                        );
                    }
                }
            }
        });
        tab_group.widget().add_controller(tab_nb_click);

        // Smooth touchpad 2-finger scroll over tab bar (lateral & vertical)
        let nb_scroll = gtk4::EventControllerScroll::new(
            gtk4::EventControllerScrollFlags::BOTH_AXES,
        );
        let nb_for_bar_scroll = tab_group.widget().clone();
        let accum_for_bar_scroll = tab_scroll_accum.clone();
        let pos_for_bar_scroll = last_pointer_pos.clone();
        nb_scroll.connect_scroll(move |ctrl, dx, dy| {
            if let Some((x, y)) = pos_for_bar_scroll.get() {
                if is_pointer_in_tab_bar(&nb_for_bar_scroll, x as f32, y as f32) {
                    return handle_smooth_tab_scroll(ctrl, dx, dy, &nb_for_bar_scroll, &accum_for_bar_scroll);
                }
            }
            gtk4::glib::Propagation::Proceed
        });
        let accum_for_bar_end = tab_scroll_accum.clone();
        nb_scroll.connect_scroll_end(move |_| {
            accum_for_bar_end.set(0.0);
        });
        tab_group.widget().add_controller(nb_scroll);

        // Motion tracking to determine exact cursor coordinates over the notebook
        let motion_ctrl = gtk4::EventControllerMotion::new();
        let pos_for_motion = last_pointer_pos.clone();
        motion_ctrl.connect_motion(move |_, x, y| {
            pos_for_motion.set(Some((x, y)));
        });
        let pos_for_leave = last_pointer_pos.clone();
        motion_ctrl.connect_leave(move |_| {
            pos_for_leave.set(None);
        });
        tab_group.widget().add_controller(motion_ctrl);

        // Quit Action
        let quit_action = gtk4::gio::SimpleAction::new("quit", None);
        let window_clone = window.clone();
        quit_action.connect_activate(move |_, _| {
            window_clone.close();
        });
        app.add_action(&quit_action);
        app.set_accels_for_action("app.quit", &["<Ctrl><Shift>q", "<Ctrl>q"]);

        let notebook_for_toggle = tab_group.widget().clone();
        let cfg_for_toggle = config_manager.clone();
        let toggle_tabs_action = gtk4::gio::SimpleAction::new("toggle-tabs", None);
        toggle_tabs_action.connect_activate(move |_, _| {
            let current = notebook_for_toggle.tab_pos();
            let (new_pos, orientation) = if current == gtk4::PositionType::Top {
                (gtk4::PositionType::Left, crate::config::TabOrientation::Vertical)
            } else {
                (gtk4::PositionType::Top, crate::config::TabOrientation::Horizontal)
            };
            notebook_for_toggle.set_tab_pos(new_pos);
            cfg_for_toggle.borrow_mut().set_orientation(orientation);
        });
        window.add_action(&toggle_tabs_action);

        // HeaderBar
        let header_bar = gtk4::HeaderBar::new();
        window.set_titlebar(Some(&header_bar));

        let add_button = gtk4::Button::from_icon_name("list-add-symbolic");
        add_button.set_action_name(Some("win.new-tab"));
        add_button.set_tooltip_text(Some("Nova Aba (Ctrl+Shift+T)"));
        add_button.set_can_focus(false);
        header_bar.pack_start(&add_button);

        let split_h_button = gtk4::Button::from_icon_name("view-split-left-right-symbolic");
        split_h_button.set_action_name(Some("win.split-right"));
        split_h_button.set_tooltip_text(Some("Dividir à Direita (Ctrl+Shift+E)"));
        split_h_button.set_can_focus(false);
        header_bar.pack_start(&split_h_button);

        let split_v_button = gtk4::Button::from_icon_name("view-split-top-bottom-symbolic");
        split_v_button.set_action_name(Some("win.split-down"));
        split_v_button.set_tooltip_text(Some("Dividir Abaixo (Ctrl+Shift+O)"));
        split_v_button.set_can_focus(false);
        header_bar.pack_start(&split_v_button);

        let unsplit_button = gtk4::Button::from_icon_name("view-restore-symbolic");
        unsplit_button.set_action_name(Some("win.unsplit"));
        unsplit_button.set_tooltip_text(Some("Desfazer Divisão (Ctrl+Shift+U)"));
        unsplit_button.set_can_focus(false);
        header_bar.pack_start(&unsplit_button);

        let menu_button = gtk4::MenuButton::new();
        menu_button.set_icon_name("open-menu-symbolic");
        menu_button.set_can_focus(false);
        let menu = gtk4::gio::Menu::new();
        menu.append(Some("Nova Aba"), Some("win.new-tab"));
        menu.append(Some("Dividir à Direita"), Some("win.split-right"));
        menu.append(Some("Dividir Abaixo"), Some("win.split-down"));
        menu.append(Some("Desfazer Divisão"), Some("win.unsplit"));
        menu.append(Some("Fechar Painel / Aba"), Some("win.close-pane"));
        menu.append(Some("Preferências e Temas"), Some("win.preferences"));
        menu.append(Some("Alternar Abas Verticais"), Some("win.toggle-tabs"));
        menu.append(Some("Sair"), Some("app.quit"));
        menu_button.set_menu_model(Some(&menu));
        header_bar.pack_end(&menu_button);

        // Window layout (no double titlebar)
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        let tab_widget = tab_group.widget();
        tab_widget.set_hexpand(true);
        tab_widget.set_vexpand(true);
        container.append(tab_widget);

        window.set_child(Some(&container));

        // Show window and immediately give keyboard focus to the active terminal
        window.present();

        let nb_for_focus = tab_group.widget().clone();
        let win_for_focus = window.clone();
        glib::idle_add_local_once(move || {
            let current_idx = nb_for_focus.current_page().unwrap_or(0);
            if let Some(active_tab) = nb_for_focus.nth_page(Some(current_idx)) {
                if let Some(focused) = split::find_focused_terminal(&active_tab) {
                    focused.grab_focus();
                    gtk4::prelude::RootExt::set_focus(&win_for_focus, Some(&focused));
                } else {
                    active_tab.grab_focus();
                    gtk4::prelude::RootExt::set_focus(&win_for_focus, Some(&active_tab));
                }
            }
        });
    });

    // Run the GTK main loop
    app.run();
}

/// Helper to check if pointer coordinates (px, py) are located within the tab bar area of the notebook.
pub fn is_pointer_in_tab_bar(nb: &gtk4::Notebook, px: f32, py: f32) -> bool {
    let n_pages = nb.n_pages();
    if n_pages == 0 {
        return false;
    }
    let pos = nb.tab_pos();
    let mut max_tab_y = 0.0f32;
    let mut max_tab_x = 0.0f32;

    for i in 0..n_pages {
        if let Some(page) = nb.nth_page(Some(i)) {
            if let Some(tab_label) = nb.tab_label(&page) {
                let target = tab_label.parent().unwrap_or(tab_label);
                if let Some(rect) = target.compute_bounds(nb) {
                    let rx = rect.x();
                    let ry = rect.y();
                    let rw = rect.width();
                    let rh = rect.height();

                    max_tab_y = max_tab_y.max(ry + rh);
                    max_tab_x = max_tab_x.max(rx + rw);
                }
            }
        }
    }

    match pos {
        gtk4::PositionType::Top => py >= 0.0 && py <= (max_tab_y + 12.0).max(42.0),
        gtk4::PositionType::Bottom => {
            let h = nb.height() as f32;
            py >= h - (max_tab_y + 12.0).max(42.0) && py <= h
        }
        gtk4::PositionType::Left => px >= 0.0 && px <= (max_tab_x + 12.0).max(180.0),
        gtk4::PositionType::Right => {
            let w = nb.width() as f32;
            px >= w - (max_tab_x + 12.0).max(180.0) && px <= w
        }
        _ => false,
    }
}

/// Smoothly cycles tabs based on 2-finger touchpad lateral (or vertical) scrolling.
pub fn handle_smooth_tab_scroll(
    controller: &gtk4::EventControllerScroll,
    dx: f64,
    dy: f64,
    nb: &gtk4::Notebook,
    scroll_accum: &std::rc::Rc<std::cell::Cell<f64>>,
) -> gtk4::glib::Propagation {
    let state = controller.current_event_state();
    if state.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
        return gtk4::glib::Propagation::Proceed;
    }

    let delta = if dx.abs() >= dy.abs() { dx } else { dy };
    if delta.abs() < 0.0001 {
        return gtk4::glib::Propagation::Proceed;
    }

    let unit = controller.unit();
    let threshold = match unit {
        gtk4::gdk::ScrollUnit::Wheel => 0.5,
        _ => 20.0, // Smooth touchpad lateral / vertical scroll threshold
    };

    let mut current_accum = scroll_accum.get();
    // If direction changed, reset accumulator immediately for instant responsiveness
    if (current_accum > 0.0 && delta < 0.0) || (current_accum < 0.0 && delta > 0.0) {
        current_accum = 0.0;
    }
    current_accum += delta;

    let n = nb.n_pages();
    if n <= 1 {
        scroll_accum.set(0.0);
        return gtk4::glib::Propagation::Stop;
    }

    if current_accum >= threshold {
        let cur = nb.current_page().unwrap_or(0);
        if cur + 1 < n {
            nb.set_current_page(Some(cur + 1));
            if let Some(page) = nb.nth_page(Some(cur + 1)) {
                page.grab_focus();
            }
        }
        if unit == gtk4::gdk::ScrollUnit::Wheel {
            current_accum = 0.0;
        } else {
            current_accum -= threshold;
            if current_accum > threshold * 1.5 {
                current_accum = threshold * 1.5;
            }
        }
    } else if current_accum <= -threshold {
        let cur = nb.current_page().unwrap_or(0);
        if cur > 0 {
            nb.set_current_page(Some(cur - 1));
            if let Some(page) = nb.nth_page(Some(cur - 1)) {
                page.grab_focus();
            }
        }
        if unit == gtk4::gdk::ScrollUnit::Wheel {
            current_accum = 0.0;
        } else {
            current_accum += threshold;
            if current_accum < -threshold * 1.5 {
                current_accum = -threshold * 1.5;
            }
        }
    }

    scroll_accum.set(current_accum);
    gtk4::glib::Propagation::Stop
}

/// Truncates a string in the middle if it exceeds `max_chars`, inserting `…`.
pub fn truncate_middle(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars || max_chars <= 3 {
        return s.to_string();
    }
    let keep_prefix = (max_chars - 1) / 2;
    let keep_suffix = max_chars - 1 - keep_prefix;
    let prefix: String = s.chars().take(keep_prefix).collect();
    let suffix: String = s.chars().skip(char_count - keep_suffix).collect();
    format!("{prefix}…{suffix}")
}

/// Format directory path for tab title display. Replaces `$HOME` prefix with `~`,
/// and shortens deep paths cleanly so the tab header remains readable and bounded.
pub fn format_dir_path(path: &std::path::Path) -> String {
    let raw = if let Ok(home) = std::env::var("HOME") {
        let home_path = std::path::Path::new(&home);
        if path == home_path {
            return "~".to_string();
        }
        if let Ok(rel) = path.strip_prefix(home_path) {
            let rel_str = rel.display().to_string();
            if rel_str.is_empty() {
                return "~".to_string();
            }
            format!("~/{}", rel_str)
        } else {
            path.to_string_lossy().into_owned()
        }
    } else {
        path.to_string_lossy().into_owned()
    };

    if raw.is_empty() {
        return "~".to_string();
    }

    const MAX_CHARS: usize = 30;
    if raw.chars().count() <= MAX_CHARS {
        return raw;
    }

    // Smart path truncation: preserve prefix (~ or /root) and the deepest subdirectories
    let parts: Vec<&str> = raw.split('/').collect();
    if parts.len() <= 2 {
        return truncate_middle(&raw, MAX_CHARS);
    }

    let prefix = if parts[0].is_empty() {
        if parts.len() > 1 && !parts[1].is_empty() {
            format!("/{}", parts[1])
        } else {
            "/".to_string()
        }
    } else {
        parts[0].to_string()
    };

    let suffix = parts[parts.len() - 1];
    // If prefix + suffix alone exceeds MAX_CHARS, truncate the suffix
    if prefix.chars().count() + suffix.chars().count() + 3 > MAX_CHARS {
        let avail = MAX_CHARS.saturating_sub(prefix.chars().count() + 4);
        let shortened_suffix = if avail >= 4 {
            let keep: String = suffix.chars().take(avail).collect();
            format!("{keep}…")
        } else {
            suffix.to_string()
        };
        return format!("{prefix}/…/{shortened_suffix}");
    }

    let mut end_parts = vec![suffix];
    let mut idx = parts.len().saturating_sub(2);
    while idx > 0 {
        let next_part = parts[idx];
        let current_len: usize = prefix.chars().count()
            + 4 // for "/…/"
            + end_parts.iter().map(|p| p.chars().count() + 1).sum::<usize>()
            + next_part.chars().count();
        if current_len <= MAX_CHARS {
            end_parts.insert(0, next_part);
            idx -= 1;
        } else {
            break;
        }
    }

    format!("{prefix}/…/{}", end_parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_truncate_middle() {
        assert_eq!(truncate_middle("hello", 10), "hello");
        assert_eq!(truncate_middle("hello world", 5), "he…ld");
        assert_eq!(truncate_middle("abcdefghijklmnopqrstuvwxyz", 10), "abcd…vwxyz");
    }

    #[test]
    fn test_format_dir_path() {
        if let Ok(home) = std::env::var("HOME") {
            let home_p = Path::new(&home);
            assert_eq!(format_dir_path(home_p), "~");
            let sub = home_p.join("projects").join("rust");
            assert_eq!(format_dir_path(&sub), "~/projects/rust");

            let space_sub = home_p.join("pasta com espaço");
            assert_eq!(format_dir_path(&space_sub), "~/pasta com espaço");

            let deep_sub = home_p.join("Code").join("rust").join("WhattyTheShell!").join("build").join("desktop").join("release");
            let formatted_deep = format_dir_path(&deep_sub);
            assert!(formatted_deep.chars().count() <= 30);
            assert!(formatted_deep.starts_with("~/…/"));
            assert!(formatted_deep.ends_with("release"));
        }
        let root = Path::new("/etc");
        assert_eq!(format_dir_path(root), "/etc");

        let slash = Path::new("/");
        assert_eq!(format_dir_path(slash), "/");

        let empty = Path::new("");
        assert_eq!(format_dir_path(empty), "~");

        let deep_system = Path::new("/var/log/journal/1234567890abcdef/system.log");
        let formatted_sys = format_dir_path(deep_system);
        assert!(formatted_sys.chars().count() <= 30);
        assert!(formatted_sys.starts_with("/var/…/"));
    }
}
