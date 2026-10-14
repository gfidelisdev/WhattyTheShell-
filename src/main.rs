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
        .application_id("com.example.whattytheshell")
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
        let window = ApplicationWindow::builder()
            .application(app)
            .title("WhattyTheShell! – GPU Terminal")
            .default_width(1024)
            .default_height(768)
            .build();

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

        // Create TabGroup (horizontal or vertical as configured)
        let tab_group = TabGroup::new(config_manager.borrow().tab_orientation());

        // Connect page switch to immediately grab focus on the active terminal
        let win_switch = window.clone();
        tab_group.widget().connect_switch_page(move |_, page, _| {
            page.grab_focus();
            gtk4::prelude::RootExt::set_focus(&win_switch, Some(page));
        });

        let new_tab_action = gtk4::gio::SimpleAction::new("new-tab", None);
        let device_clone = device.clone();
        let queue_clone = queue.clone();
        let cfg_for_tab = config_manager.clone();

        let create_tab = move |notebook: &gtk4::Notebook,
                               window: &ApplicationWindow,
                               working_dir: Option<&std::path::Path>,
                               title: Option<&str>| {
            let (font_family, font_size) = {
                let c = cfg_for_tab.borrow();
                (c.font_family(), c.font_size())
            };
            if let Ok(view) = TerminalView::with_working_dir(
                device_clone.clone(),
                queue_clone.clone(),
                font_family,
                font_size,
                working_dir,
            ) {
                // Apply theme palette if configured
                if let Some(ref colors) = cfg_for_tab.borrow().theme_colors() {
                    let palette = crate::render::pipeline::palette_from_vscode_colors(colors);
                    view.set_palette(palette);
                }

                let tab_title = title
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| format!("Terminal {}", notebook.n_pages() + 1));

                let label = gtk4::Label::new(Some(&tab_title));
                let idx = notebook.append_page(&view, Some(&label));
                notebook.set_current_page(Some(idx));

                // Immediately focus terminal view and cursor
                view.grab_focus();
                gtk4::prelude::RootExt::set_focus(window, Some(&view));

                let nb_clone = notebook.clone();
                let win_clone = window.clone();
                let cfg_for_exit = cfg_for_tab.clone();
                view.connect_closure(
                    "child-exited",
                    false,
                    gtk4::glib::closure_local!(move |v: TerminalView| {
                        // If this was the last tab, preserve its working directory so next launch restores it
                        if nb_clone.n_pages() <= 1 {
                            if let Some(cwd) = v.current_working_dir() {
                                cfg_for_exit.borrow_mut().set_open_tabs(vec![crate::config::SavedTab {
                                    title: Some("Terminal 1".into()),
                                    working_dir: Some(cwd),
                                }]);
                            }
                        }

                        if let Some(num) = nb_clone.page_num(&v) {
                            nb_clone.remove_page(Some(num));
                        }
                        if nb_clone.n_pages() == 0 {
                            win_clone.close();
                        } else if let Some(current) = nb_clone.current_page() {
                            if let Some(page) = nb_clone.nth_page(Some(current)) {
                                page.grab_focus();
                                gtk4::prelude::RootExt::set_focus(&win_clone, Some(&page));
                            }
                        }
                    }),
                );
            }
        };

        // Restore saved tabs or create the primary terminal view
        let saved_tabs = config_manager.borrow().open_tabs();
        if saved_tabs.is_empty() {
            create_tab(tab_group.widget(), &window, None, None);
        } else {
            for tab in saved_tabs {
                let cwd = tab.working_dir.as_deref();
                let title = tab.title.as_deref();
                create_tab(tab_group.widget(), &window, cwd, title);
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
                .and_then(|page| page.downcast::<TerminalView>().ok())
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
                    let title = nb_for_close.tab_label_text(&page).map(|s| s.to_string());
                    if let Ok(view) = page.downcast::<TerminalView>() {
                        let cwd = view.current_working_dir();
                        tabs_to_save.push(crate::config::SavedTab {
                            title,
                            working_dir: cwd,
                        });
                    }
                }
            }
            if !tabs_to_save.is_empty() {
                cfg_for_close.borrow_mut().set_open_tabs(tabs_to_save);
            }
            gtk4::glib::Propagation::Proceed
        });

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
        header_bar.pack_start(&add_button);

        let menu_button = gtk4::MenuButton::new();
        menu_button.set_icon_name("open-menu-symbolic");
        let menu = gtk4::gio::Menu::new();
        menu.append(Some("New Tab"), Some("win.new-tab"));
        menu.append(Some("Preferences & Themes"), Some("win.preferences"));
        menu.append(Some("Toggle Vertical Tabs"), Some("win.toggle-tabs"));
        menu.append(Some("Quit"), Some("app.quit"));
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
        if let Some(first_tab) = tab_group.widget().nth_page(Some(0)) {
            first_tab.grab_focus();
            gtk4::prelude::RootExt::set_focus(&window, Some(&first_tab));
        }
    });

    // Run the GTK main loop
    app.run();
}
