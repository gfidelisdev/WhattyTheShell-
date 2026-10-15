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
        let window = ApplicationWindow::builder()
            .application(app)
            .title("WhattyTheShell! – GPU Terminal")
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

        fn close_tab_page(
            notebook: &gtk4::Notebook,
            window: &ApplicationWindow,
            config_manager: &std::rc::Rc<std::cell::RefCell<ConfigManager>>,
            view: &TerminalView,
        ) {
            // If this was the last tab, preserve its working directory so next launch restores it
            if notebook.n_pages() <= 1 {
                if let Some(cwd) = view.current_working_dir() {
                    config_manager.borrow_mut().set_open_tabs(vec![crate::config::SavedTab {
                        title: Some("Terminal 1".into()),
                        working_dir: Some(cwd),
                    }]);
                }
            }

            if let Some(num) = notebook.page_num(view) {
                notebook.remove_page(Some(num));
            }
            if notebook.n_pages() == 0 {
                window.close();
            } else if let Some(current) = notebook.current_page() {
                if let Some(page) = notebook.nth_page(Some(current)) {
                    page.grab_focus();
                    gtk4::prelude::RootExt::set_focus(window, Some(&page));
                }
            }
        }

        type CreateTabFn = std::rc::Rc<
            dyn Fn(&gtk4::Notebook, &ApplicationWindow, Option<&std::path::Path>, Option<&str>),
        >;

        let create_tab_cell: std::rc::Rc<std::cell::RefCell<Option<CreateTabFn>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));

        let create_tab_cb = {
            let create_tab_cell = create_tab_cell.clone();
            let device_clone = device.clone();
            let queue_clone = queue.clone();
            let cfg_for_tab = config_manager.clone();

            std::rc::Rc::new(
                move |notebook: &gtk4::Notebook,
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

                        // Apply initial saturation boost if configured and experimental is active
                        let (initial_sat, experimental) = {
                            let c = cfg_for_tab.borrow();
                            (c.color_saturation(), c.experimental_supersaturation())
                        };
                        if experimental && initial_sat > 1.0 {
                            view.set_saturation_boost(initial_sat);
                        }

                        let tab_title = title
                            .map(|t| t.to_string())
                            .unwrap_or_else(|| format!("Terminal {}", notebook.n_pages() + 1));

                        let tab_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);

                        let label = gtk4::Label::new(Some(&tab_title));
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
                        let view_for_cb = view.clone();
                        tab_close_btn.connect_clicked(move |_| {
                            close_tab_page(&nb_for_cb, &win_for_cb, &cfg_for_cb, &view_for_cb);
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
                            std::rc::Rc::new(move || {
                                rename_entry.set_text(&label.text());
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
                            let commit_rename = {
                                let rename_popover = rename_popover.clone();
                                let rename_entry = rename_entry.clone();
                                let label = label.clone();
                                let view = view.clone();
                                move || {
                                    let text = rename_entry.text().trim().to_string();
                                    if !text.is_empty() {
                                        label.set_text(&text);
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

                        // 2. Duplicar Aba
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

                        // 3. Copiar Caminho do Diretório
                        let copy_path_btn = create_menu_item("Copiar Caminho do Diretório", "edit-copy-symbolic");
                        let pop_copy = popover.clone();
                        let view_for_copy = view.clone();
                        copy_path_btn.connect_clicked(move |_| {
                            pop_copy.popdown();
                            if let Some(cwd) = view_for_copy.current_working_dir() {
                                if let Some(display) = gtk4::gdk::Display::default() {
                                    let path_str = cwd.to_string_lossy().to_string();
                                    display.clipboard().set_text(&path_str);
                                    display.primary_clipboard().set_text(&path_str);
                                }
                            }
                        });
                        menu_box.append(&copy_path_btn);

                        // Separador
                        menu_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

                        // 4. Fechar Aba
                        let close_btn = create_menu_item("Fechar Aba", "window-close-symbolic");
                        let pop_close = popover.clone();
                        let nb_for_close_btn = notebook.clone();
                        let win_for_close_btn = window.clone();
                        let cfg_for_close_btn = cfg_for_tab.clone();
                        let view_for_close_btn = view.clone();
                        close_btn.connect_clicked(move |_| {
                            pop_close.popdown();
                            close_tab_page(
                                &nb_for_close_btn,
                                &win_for_close_btn,
                                &cfg_for_close_btn,
                                &view_for_close_btn,
                            );
                        });
                        menu_box.append(&close_btn);

                        // 5. Fechar Outras Abas
                        let close_others_btn = create_menu_item("Fechar Outras Abas", "edit-clear-symbolic");
                        let pop_close_others = popover.clone();
                        let nb_for_close_others = notebook.clone();
                        let win_for_close_others = window.clone();
                        let cfg_for_close_others = cfg_for_tab.clone();
                        let view_for_close_others = view.clone();
                        close_others_btn.connect_clicked(move |_| {
                            pop_close_others.popdown();
                            let mut i = 0;
                            while i < nb_for_close_others.n_pages() {
                                if let Some(page) = nb_for_close_others.nth_page(Some(i)) {
                                    if page == view_for_close_others.clone().upcast::<gtk4::Widget>() {
                                        i += 1;
                                    } else {
                                        if let Ok(tv) = page.downcast::<TerminalView>() {
                                            close_tab_page(&nb_for_close_others, &win_for_close_others, &cfg_for_close_others, &tv);
                                        } else {
                                            nb_for_close_others.remove_page(Some(i));
                                        }
                                    }
                                } else {
                                    break;
                                }
                            }
                        });
                        menu_box.append(&close_others_btn);

                        popover.set_child(Some(&menu_box));

                        // Right Click gesture to open Popover
                        let tab_right_click = gtk4::GestureClick::new();
                        tab_right_click.set_button(gtk4::gdk::BUTTON_SECONDARY);
                        let pop_trigger = popover.clone();
                        tab_right_click.connect_pressed(move |gesture, _, _, _| {
                            gesture.set_state(gtk4::EventSequenceState::Claimed);
                            pop_trigger.popup();
                        });
                        tab_box.add_controller(tab_right_click);

                        // Left Click gesture: Ctrl+Click rename, Double click close, single click select
                        let tab_left_click = gtk4::GestureClick::new();
                        tab_left_click.set_button(gtk4::gdk::BUTTON_PRIMARY);
                        let nb_for_tab_click = notebook.clone();
                        let win_for_tab_click = window.clone();
                        let cfg_for_tab_click = cfg_for_tab.clone();
                        let view_for_tab_click = view.clone();
                        let start_rename_for_click = start_rename.clone();
                        tab_left_click.connect_pressed(move |gesture, n_press, _x, _y| {
                            let state = gesture.current_event_state();
                            if state.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                                gesture.set_state(gtk4::EventSequenceState::Claimed);
                                start_rename_for_click();
                                return;
                            }

                            if n_press == 1 {
                                if let Some(page_idx) = nb_for_tab_click.page_num(&view_for_tab_click) {
                                    nb_for_tab_click.set_current_page(Some(page_idx));
                                    view_for_tab_click.grab_focus();
                                }
                            } else if n_press == 2 {
                                if !cfg_for_tab_click.borrow().double_click_close_tab() {
                                    return;
                                }
                                gesture.set_state(gtk4::EventSequenceState::Claimed);
                                close_tab_page(
                                    &nb_for_tab_click,
                                    &win_for_tab_click,
                                    &cfg_for_tab_click,
                                    &view_for_tab_click,
                                );
                            }
                        });
                        tab_box.add_controller(tab_left_click);

                        let idx = notebook.append_page(&view, Some(&tab_box));
                        notebook.set_current_page(Some(idx));

                        // Immediately focus terminal view and cursor
                        view.grab_focus();
                        gtk4::prelude::RootExt::set_focus(window, Some(&view));

                        let nb_clone = notebook.clone();
                        let win_clone = window.clone();
                        let cfg_for_exit = cfg_for_tab.clone();
                        let v_clone = view.clone();
                        view.connect_closure(
                            "child-exited",
                            false,
                            gtk4::glib::closure_local!(move |_v: TerminalView| {
                                close_tab_page(&nb_clone, &win_clone, &cfg_for_exit, &v_clone);
                            }),
                        );
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
                let title = tab.title.as_deref();
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
                    let title = nb_for_close.tab_label(&page).and_then(|tl| {
                        if let Some(l) = tl.downcast_ref::<gtk4::Label>() {
                            Some(l.text().to_string())
                        } else if let Some(first) = tl.first_child() {
                            first.downcast_ref::<gtk4::Label>().map(|l| l.text().to_string())
                        } else {
                            None
                        }
                    });
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
                            if let Ok(tv) = page.downcast::<TerminalView>() {
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
                            if let Ok(tv) = page.downcast::<TerminalView>() {
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
                            .and_then(|p| p.downcast::<TerminalView>().ok())
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

        let nb_for_focus = tab_group.widget().clone();
        let win_for_focus = window.clone();
        glib::idle_add_local_once(move || {
            let current_idx = nb_for_focus.current_page().unwrap_or(0);
            if let Some(active_tab) = nb_for_focus.nth_page(Some(current_idx)) {
                active_tab.grab_focus();
                gtk4::prelude::RootExt::set_focus(&win_for_focus, Some(&active_tab));
            }
        });
    });

    // Run the GTK main loop
    app.run();
}
