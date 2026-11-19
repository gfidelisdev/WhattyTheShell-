// src/settings_dialog.rs
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use gtk4::prelude::*;
use gtk4::{
    glib, Align, ApplicationWindow, Box as GtkBox, Button, CheckButton, DrawingArea, DropDown,
    Entry, Frame, GestureClick, Image, Label, ListBox, ListBoxRow, Notebook, Orientation,
    PasswordEntry, ProgressBar, Scale, ScrolledWindow, SearchEntry, SpinButton, Stack,
    StackSwitcher, Switch, Window,
};

use crate::render::pipeline::{hex_to_rgba, palette_from_vscode_colors, Palette};
use crate::theme_manager::ThemeManager;
use crate::theme_marketplace::{
    extract_key_swatches, find_builtin_theme, get_cached_theme, popular_presets,
    DownloadProgress, Extension,
};
use crate::widget::TerminalView;

fn for_each_terminal<F: FnMut(&TerminalView)>(nb: &Notebook, mut f: F) {
    for i in 0..nb.n_pages() {
        if let Some(page) = nb.nth_page(Some(i)) {
            for tv in crate::split::find_terminals(&page) {
                f(&tv);
            }
        }
    }
}

fn create_swatches_widget(swatches: &[String], width: i32, height: i32) -> DrawingArea {
    let area = DrawingArea::builder()
        .content_width(width)
        .content_height(height)
        .build();

    let parsed_rgbs: Vec<[f64; 3]> = swatches
        .iter()
        .filter_map(|hex| {
            hex_to_rgba(hex).map(|c| [c[0] as f64, c[1] as f64, c[2] as f64])
        })
        .collect();

    area.set_draw_func(move |_area, cr, w, h| {
        if parsed_rgbs.is_empty() {
            return;
        }
        let n = parsed_rgbs.len() as f64;
        let gap = 3.0;
        let swatch_w = ((w as f64 - gap * (n - 1.0)) / n).max(3.0);
        let swatch_h = h as f64;

        for (i, rgb) in parsed_rgbs.iter().enumerate() {
            let x = i as f64 * (swatch_w + gap);
            cr.set_source_rgb(rgb[0], rgb[1], rgb[2]);
            let _ = cr.rectangle(x, 0.0, swatch_w, swatch_h);
            let _ = cr.fill();

            // Subtle border
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.15);
            let _ = cr.rectangle(x, 0.0, swatch_w, swatch_h);
            let _ = cr.stroke();
        }
    });

    area
}

fn create_terminal_mockup(colors: &HashMap<String, String>) -> GtkBox {
    let bg = colors
        .get("editor.background")
        .or_else(|| colors.get("terminal.background"))
        .cloned()
        .unwrap_or_else(|| "#1e1e1e".into());
    let fg = colors
        .get("editor.foreground")
        .or_else(|| colors.get("terminal.foreground"))
        .cloned()
        .unwrap_or_else(|| "#cccccc".into());
    let red = colors
        .get("terminal.ansiRed")
        .or_else(|| colors.get("terminal.ansiBrightRed"))
        .cloned()
        .unwrap_or_else(|| "#ff5555".into());
    let green = colors
        .get("terminal.ansiGreen")
        .or_else(|| colors.get("terminal.ansiBrightGreen"))
        .cloned()
        .unwrap_or_else(|| "#50fa7b".into());
    let blue = colors
        .get("terminal.ansiBlue")
        .or_else(|| colors.get("terminal.ansiBrightBlue"))
        .cloned()
        .unwrap_or_else(|| "#61afef".into());
    let cyan = colors
        .get("terminal.ansiCyan")
        .or_else(|| colors.get("terminal.ansiBrightCyan"))
        .cloned()
        .unwrap_or_else(|| "#8be9fd".into());
    let cur = colors
        .get("editorCursor.foreground")
        .or_else(|| colors.get("terminalCursor.foreground"))
        .cloned()
        .unwrap_or_else(|| "#ffffff".into());

    let outer_box = GtkBox::new(Orientation::Vertical, 0);

    let frame = Frame::new(None);
    frame.add_css_class("view");

    let content_box = GtkBox::new(Orientation::Vertical, 4);
    content_box.set_margin_top(8);
    content_box.set_margin_bottom(8);
    content_box.set_margin_start(10);
    content_box.set_margin_end(10);

    // Window controls header (red, yellow, green dots)
    let header = GtkBox::new(Orientation::Horizontal, 5);
    header.set_margin_bottom(6);
    for dot_color in ["#ff5f56", "#ffbd2e", "#27c93f"] {
        let dot = DrawingArea::builder().content_width(8).content_height(8).build();
        let c_str = dot_color.to_string();
        dot.set_draw_func(move |_, cr, w, h| {
            if let Some(c) = hex_to_rgba(&c_str) {
                cr.set_source_rgb(c[0] as f64, c[1] as f64, c[2] as f64);
                let _ = cr.arc(w as f64 / 2.0, h as f64 / 2.0, 4.0, 0.0, 2.0 * std::f64::consts::PI);
                let _ = cr.fill();
            }
        });
        header.append(&dot);
    }
    content_box.append(&header);

    // Mock terminal prompt & lines
    let l1 = Label::new(None);
    l1.set_halign(Align::Start);
    l1.set_markup(&format!(
        "<span font_family=\"monospace\"><span foreground=\"{}\"><b>user@whatty</b></span>:<span foreground=\"{}\">~</span>$ git status</span>",
        cyan, blue
    ));
    content_box.append(&l1);

    let l2 = Label::new(None);
    l2.set_halign(Align::Start);
    l2.set_markup(&format!(
        "<span font_family=\"monospace\">On branch <span foreground=\"{}\">main</span></span>",
        green
    ));
    content_box.append(&l2);

    let l3 = Label::new(None);
    l3.set_halign(Align::Start);
    l3.set_markup(&format!(
        "<span font_family=\"monospace\" alpha=\"70%\">Changes to be committed:</span>"
    ));
    content_box.append(&l3);

    let l4 = Label::new(None);
    l4.set_halign(Align::Start);
    l4.set_markup(&format!(
        "<span font_family=\"monospace\">  <span foreground=\"{}\">modified:   src/main.rs</span></span>",
        green
    ));
    content_box.append(&l4);

    let l5 = Label::new(None);
    l5.set_halign(Align::Start);
    l5.set_markup(&format!(
        "<span font_family=\"monospace\">  <span foreground=\"{}\">untracked:  preview.png</span></span>",
        red
    ));
    content_box.append(&l5);

    let l6 = Label::new(None);
    l6.set_halign(Align::Start);
    l6.set_markup(&format!(
        "<span font_family=\"monospace\"><span foreground=\"{}\"><b>user@whatty</b></span>:<span foreground=\"{}\">~</span>$ cargo run<span background=\"{}\"> </span></span>",
        cyan, blue, cur
    ));
    content_box.append(&l6);

    frame.set_child(Some(&content_box));

    // Dynamic CSS provider for this mockup container
    let css_provider = gtk4::CssProvider::new();
    let css = format!(
        ".theme-mockup-frame {{ background-color: {}; color: {}; border: 1px solid rgba(128,128,128,0.25); border-radius: 8px; }}",
        bg, fg
    );
    css_provider.load_from_string(&css);
    frame.add_css_class("theme-mockup-frame");

    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("Default display"),
        &css_provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    outer_box.append(&frame);
    outer_box
}

fn load_icon_preview(icon_name: &str, file_rel_path: &str) -> Image {
    if std::path::Path::new(file_rel_path).exists() {
        let img = Image::from_file(file_rel_path);
        img.set_pixel_size(48);
        img
    } else {
        let img = Image::from_icon_name(icon_name);
        img.set_pixel_size(48);
        img
    }
}

fn update_user_desktop_icon(icon_name: &str) {
    if let Some(data_dir) = dirs_next::data_dir() {
        let desktop_path = data_dir.join("applications/io.github.whattytheshell.desktop");
        if desktop_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&desktop_path) {
                let updated = content
                    .lines()
                    .map(|line| {
                        if line.starts_with("Icon=") {
                            format!("Icon={}", icon_name)
                        } else {
                            line.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n") + "\n";
                let _ = std::fs::write(&desktop_path, updated);
                let _ = std::process::Command::new("update-desktop-database")
                    .arg(data_dir.join("applications"))
                    .output();
            }
        }
    }
}

fn get_active_theme_display(config_manager: &crate::config::ConfigManager) -> (String, Vec<String>) {
    let name = config_manager
        .theme_name()
        .unwrap_or_else(|| "Default Dark".to_string());
    let swatches = if let Some(colors) = config_manager.theme_colors() {
        extract_key_swatches(&colors)
    } else {
        vec![
            "#121212".into(),
            "#e0e0e0".into(),
            "#ff5555".into(),
            "#50fa7b".into(),
            "#61afef".into(),
            "#bd93f9".into(),
        ]
    };
    (name, swatches)
}

pub fn open_settings_dialog(
    parent: &ApplicationWindow,
    notebook: &Notebook,
    theme_manager: &ThemeManager,
    config_manager: std::rc::Rc<std::cell::RefCell<crate::config::ConfigManager>>,
) {
    let dialog = Window::builder()
        .title("Preferences & Themes")
        .transient_for(parent)
        .modal(true)
        .default_width(820)
        .default_height(600)
        .build();

    let root_box = GtkBox::new(Orientation::Vertical, 12);
    root_box.set_margin_top(16);
    root_box.set_margin_bottom(16);
    root_box.set_margin_start(16);
    root_box.set_margin_end(16);

    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);

    let switcher = StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.set_halign(Align::Center);
    root_box.append(&switcher);

    // ==========================================
    // TAB 1: Appearance (Font, Custom Colors & App Icon)
    // ==========================================
    let appearance_box = GtkBox::new(Orientation::Vertical, 16);
    appearance_box.set_margin_top(12);

    // Section 0: Application Icon
    let icon_section = GtkBox::new(Orientation::Vertical, 8);
    let icon_title = Label::new(Some("Application Icon"));
    icon_title.set_halign(Align::Start);
    icon_title.add_css_class("heading");
    icon_section.append(&icon_title);

    let icon_desc = Label::new(Some("Choose the icon style for WhattyTheShell (window and desktop):"));
    icon_desc.set_halign(Align::Start);
    icon_desc.add_css_class("dim-label");
    icon_section.append(&icon_desc);

    let icon_cards_box = GtkBox::new(Orientation::Horizontal, 16);

    let is_classic = config_manager.borrow().app_icon() == "classic";

    // Modern Option Card (Default)
    let modern_card = GtkBox::new(Orientation::Horizontal, 12);
    modern_card.add_css_class("icon-choice-card");
    let modern_img = load_icon_preview("io.github.whattytheshell", "icons/wts.png");
    let modern_info = GtkBox::new(Orientation::Vertical, 2);
    let modern_radio = CheckButton::builder()
        .label("Modern (Default)")
        .active(!is_classic)
        .build();
    let modern_sub = Label::new(Some("New official application icon"));
    modern_sub.set_halign(Align::Start);
    modern_sub.add_css_class("dim-label");
    modern_info.append(&modern_radio);
    modern_info.append(&modern_sub);
    modern_card.append(&modern_img);
    modern_card.append(&modern_info);

    // Classic Option Card (Original)
    let classic_card = GtkBox::new(Orientation::Horizontal, 12);
    classic_card.add_css_class("icon-choice-card");
    let classic_img = load_icon_preview("io.github.whattytheshell-classic", "icons/wts-old.png");
    let classic_info = GtkBox::new(Orientation::Vertical, 2);
    let classic_radio = CheckButton::builder()
        .label("Classic (Original)")
        .group(&modern_radio)
        .active(is_classic)
        .build();
    let classic_sub = Label::new(Some("Retro neon terminal icon"));
    classic_sub.set_halign(Align::Start);
    classic_sub.add_css_class("dim-label");
    classic_info.append(&classic_radio);
    classic_info.append(&classic_sub);
    classic_card.append(&classic_img);
    classic_card.append(&classic_info);

    // Connect toggles
    let parent_for_modern = parent.clone();
    let cfg_for_modern = config_manager.clone();
    modern_radio.connect_toggled(move |btn| {
        if btn.is_active() {
            cfg_for_modern.borrow_mut().set_app_icon("modern".into());
            let icon_name = "io.github.whattytheshell";
            parent_for_modern.set_icon_name(Some(icon_name));
            gtk4::Window::set_default_icon_name(icon_name);
            update_user_desktop_icon(icon_name);
        }
    });

    let parent_for_classic = parent.clone();
    let cfg_for_classic = config_manager.clone();
    classic_radio.connect_toggled(move |btn| {
        if btn.is_active() {
            cfg_for_classic.borrow_mut().set_app_icon("classic".into());
            let icon_name = "io.github.whattytheshell-classic";
            parent_for_classic.set_icon_name(Some(icon_name));
            gtk4::Window::set_default_icon_name(icon_name);
            update_user_desktop_icon(icon_name);
        }
    });

    let m_radio_clone = modern_radio.clone();
    let click_modern = GestureClick::new();
    click_modern.connect_pressed(move |_, _, _, _| {
        m_radio_clone.set_active(true);
    });
    modern_card.add_controller(click_modern);

    let c_radio_clone = classic_radio.clone();
    let click_classic = GestureClick::new();
    click_classic.connect_pressed(move |_, _, _, _| {
        c_radio_clone.set_active(true);
    });
    classic_card.add_controller(click_classic);

    icon_cards_box.append(&modern_card);
    icon_cards_box.append(&classic_card);
    icon_section.append(&icon_cards_box);

    appearance_box.append(&icon_section);

    // Font Section
    let font_section = GtkBox::new(Orientation::Vertical, 8);
    let font_title = Label::new(Some("Terminal Typography & Font"));
    font_title.set_halign(Align::Start);
    font_title.add_css_class("heading");
    font_section.append(&font_title);

    // Filter controls: SearchEntry + "Only Monospaced" Switch
    let font_controls = GtkBox::new(Orientation::Horizontal, 12);
    let font_search = SearchEntry::new();
    font_search.set_hexpand(true);
    font_search.set_placeholder_text(Some("Search fonts (e.g. Fira, Mono, Hack, JetBrains, Ubuntu)..."));

    let mono_label = Label::new(Some("Only Monospaced:"));
    let mono_switch = Switch::new();
    mono_switch.set_active(true);
    mono_switch.set_valign(Align::Center);

    font_controls.append(&font_search);
    font_controls.append(&mono_label);
    font_controls.append(&mono_switch);
    font_section.append(&font_controls);

    // Scrolled list of fonts
    let font_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .min_content_height(170)
        .vexpand(true)
        .build();

    let font_list_box = ListBox::new();
    font_list_box.add_css_class("boxed-list");
    font_scroll.set_child(Some(&font_list_box));
    font_section.append(&font_scroll);

    // Font apply row
    let font_apply_row = GtkBox::new(Orientation::Horizontal, 12);
    let fam_label = Label::new(Some("Selected Font:"));
    let fam_entry = Entry::new();
    fam_entry.set_hexpand(true);
    if let Some(f) = config_manager.borrow().font_family() {
        fam_entry.set_text(&f);
    }

    let size_label = Label::new(Some("Size (pt):"));
    let size_spin = SpinButton::with_range(8.0, 36.0, 1.0);
    size_spin.set_value(config_manager.borrow().font_size() as f64);

    let apply_font_btn = Button::with_label("Apply Font");
    apply_font_btn.add_css_class("suggested-action");

    font_apply_row.append(&fam_label);
    font_apply_row.append(&fam_entry);
    font_apply_row.append(&size_label);
    font_apply_row.append(&size_spin);
    font_apply_row.append(&apply_font_btn);
    font_section.append(&font_apply_row);

    appearance_box.append(&font_section);

    // System fonts discovery & fuzzy population
    let all_system_fonts = Arc::new(crate::fonts::query_system_fonts());
    let all_fonts_clone = all_system_fonts.clone();
    let flb_clone = font_list_box.clone();
    let fam_entry_for_list = fam_entry.clone();

    let populate_fonts = move |query: &str, only_mono: bool| {
        flb_clone.remove_all();
        let filtered = crate::fonts::filter_fonts(&all_fonts_clone, query, only_mono);

        for font_info in filtered.into_iter().take(80) {
            let row = ListBoxRow::new();
            row.set_cursor_from_name(Some("pointer"));
            row.set_activatable(true);
            row.set_selectable(true);

            let row_box = GtkBox::new(Orientation::Horizontal, 12);
            row_box.set_margin_top(6);
            row_box.set_margin_bottom(6);
            row_box.set_margin_start(10);
            row_box.set_margin_end(10);

            let label = Label::new(None);
            label.set_halign(Align::Start);
            label.set_hexpand(true);

            let escaped = glib::markup_escape_text(&font_info.name);
            label.set_markup(&format!(
                "<span font_family=\"{}\">{}</span>",
                escaped, escaped
            ));
            row_box.append(&label);

            if font_info.is_monospaced {
                let badge = Label::new(Some("mono"));
                badge.add_css_class("dim-label");
                row_box.append(&badge);
            }

            row.set_child(Some(&row_box));

            let font_name = font_info.name.clone();
            let entry_cl = fam_entry_for_list.clone();

            let gesture = GestureClick::new();
            let fn_cl = font_name.clone();
            let ent_cl = entry_cl.clone();
            gesture.connect_released(move |_, _, _, _| {
                ent_cl.set_text(&fn_cl);
            });
            row.add_controller(gesture);

            flb_clone.append(&row);
        }
    };

    let pop_fn = Arc::new(populate_fonts);
    let pop_fn_search = pop_fn.clone();
    let mono_sw_search = mono_switch.clone();
    font_search.connect_search_changed(move |entry| {
        let text = entry.text().to_string();
        pop_fn_search(&text, mono_sw_search.is_active());
    });

    let pop_fn_mono = pop_fn.clone();
    let fs_mono = font_search.clone();
    mono_switch.connect_active_notify(move |sw| {
        let text = fs_mono.text().to_string();
        pop_fn_mono(&text, sw.is_active());
    });

    // Populate initial font list with monospace fonts
    pop_fn("", true);

    // Also support keyboard row selection
    {
        let ent_sel = fam_entry.clone();
        font_list_box.connect_row_selected(move |_, maybe_row| {
            if let Some(row) = maybe_row {
                if let Some(row_box) = row.child().and_then(|c| c.downcast::<GtkBox>().ok()) {
                    if let Some(label) = row_box.first_child().and_then(|c| c.downcast::<Label>().ok()) {
                        ent_sel.set_text(&label.text());
                    }
                }
            }
        });
    }

    // Apply font logic
    let nb_for_font = notebook.clone();
    let size_spin_clone = size_spin.clone();
    let fam_entry_clone = fam_entry.clone();
    let cfg_for_font = config_manager.clone();

    let apply_font_action = move || {
        let new_size = size_spin_clone.value() as f32;
        let fam_text = fam_entry_clone.text().to_string();
        let new_family = if fam_text.trim().is_empty() {
            None
        } else {
            Some(fam_text.trim().to_string())
        };

        cfg_for_font.borrow_mut().set_font(new_family.clone(), new_size);

        for_each_terminal(&nb_for_font, |view| {
            view.set_font(new_family.clone(), new_size);
        });
    };

    let af_btn = apply_font_action.clone();
    apply_font_btn.connect_clicked(move |_| {
        af_btn();
    });

    let af_entry = apply_font_action.clone();
    fam_entry.connect_activate(move |_| {
        af_entry();
    });

    // Colors Section
    let color_section = GtkBox::new(Orientation::Vertical, 8);
    let color_title_box = GtkBox::new(Orientation::Horizontal, 8);
    let color_title = Label::new(Some("Custom Colors"));
    color_title.set_halign(Align::Start);
    color_title.add_css_class("heading");
    color_title_box.append(&color_title);

    let (active_theme_str, _) = get_active_theme_display(&config_manager.borrow());
    let active_theme_chip = Label::new(Some(&format!("Active: {}", active_theme_str)));
    active_theme_chip.add_css_class("theme-active-badge");
    color_title_box.append(&active_theme_chip);
    color_section.append(&color_title_box);

    let colors_row = GtkBox::new(Orientation::Horizontal, 12);

    let bg_label = Label::new(Some("Background:"));
    let bg_entry = Entry::new();
    bg_entry.set_text("#1e1e1e");
    bg_entry.set_max_length(9);
    bg_entry.set_width_chars(8);

    let fg_label = Label::new(Some("Foreground:"));
    let fg_entry = Entry::new();
    fg_entry.set_text("#cccccc");
    fg_entry.set_max_length(9);
    fg_entry.set_width_chars(8);

    let cur_label = Label::new(Some("Cursor:"));
    let cur_entry = Entry::new();
    cur_entry.set_text("#ffffff");
    cur_entry.set_max_length(9);
    cur_entry.set_width_chars(8);

    let apply_colors_btn = Button::with_label("Apply Colors");

    colors_row.append(&bg_label);
    colors_row.append(&bg_entry);
    colors_row.append(&fg_label);
    colors_row.append(&fg_entry);
    colors_row.append(&cur_label);
    colors_row.append(&cur_entry);
    colors_row.append(&apply_colors_btn);
    color_section.append(&colors_row);

    let bg_entry_cl = bg_entry.clone();
    let fg_entry_cl = fg_entry.clone();
    let cur_entry_cl = cur_entry.clone();
    let nb_for_colors = notebook.clone();
    let tm_for_colors = std::cell::RefCell::new(theme_manager.clone());
    let win_for_colors = parent.clone();
    let cfg_for_colors = config_manager.clone();

    apply_colors_btn.connect_clicked(move |_| {
        let mut palette = Palette::default();
        let mut map = HashMap::new();

        if let Some(c) = hex_to_rgba(&bg_entry_cl.text()) {
            palette.bg = c;
            map.insert("editor.background".into(), bg_entry_cl.text().to_string());
        }
        if let Some(c) = hex_to_rgba(&fg_entry_cl.text()) {
            palette.fg = c;
            map.insert("editor.foreground".into(), fg_entry_cl.text().to_string());
        }
        if let Some(c) = hex_to_rgba(&cur_entry_cl.text()) {
            palette.cursor = c;
        }

        for_each_terminal(&nb_for_colors, |view| {
            view.set_palette(palette.clone());
        });

        let mut tm = tm_for_colors.borrow_mut();
        tm.apply_palette(&map);
        tm.apply_to_window(&win_for_colors);

        cfg_for_colors.borrow_mut().set_theme(Some("Custom".into()), map);
    });

    appearance_box.append(&color_section);
    let appearance_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .vexpand(true)
        .child(&appearance_box)
        .build();
    stack.add_titled(&appearance_scroll, Some("appearance"), "Appearance");

    // ==========================================
    // TAB 2: Marketplace Themes (2-Column Layout)
    // ==========================================
    let marketplace_box = GtkBox::new(Orientation::Vertical, 10);
    marketplace_box.set_margin_top(12);

    // Active Theme Indicator Banner
    let active_theme_bar = GtkBox::new(Orientation::Horizontal, 10);
    active_theme_bar.add_css_class("active-theme-bar");

    let active_theme_icon = Image::from_icon_name("preferences-desktop-theme-symbolic");
    let active_theme_title = Label::new(Some("Active Theme:"));
    active_theme_title.add_css_class("heading");

    let (active_name, active_swatches) = get_active_theme_display(&config_manager.borrow());
    let active_badge = Label::new(Some(&active_name));
    active_badge.add_css_class("theme-active-badge");

    let active_sw_box = GtkBox::new(Orientation::Horizontal, 0);
    let active_sw = create_swatches_widget(&active_swatches, 90, 18);
    active_sw_box.append(&active_sw);

    active_theme_bar.append(&active_theme_icon);
    active_theme_bar.append(&active_theme_title);
    active_theme_bar.append(&active_badge);
    active_theme_bar.append(&active_sw_box);

    marketplace_box.append(&active_theme_bar);

    let search_entry = SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search themes (e.g. Dracula, One Dark, Nord, Gruvbox)..."));
    marketplace_box.append(&search_entry);

    // Quick tags
    let quick_box = GtkBox::new(Orientation::Horizontal, 6);
    let quick_label = Label::new(Some("Popular:"));
    quick_box.append(&quick_label);

    for popular in ["Dracula", "Nord", "One Dark Pro", "Gruvbox Dark", "Monokai", "Catppuccin Mocha", "Tokyo Night", "Solarized Dark"] {
        let btn = Button::with_label(popular);
        btn.add_css_class("flat");
        let se_clone = search_entry.clone();
        btn.connect_clicked(move |_| {
            se_clone.set_text(popular);
        });
        quick_box.append(&btn);
    }
    marketplace_box.append(&quick_box);

    // Two-Column Split (Left: Theme list, Right: Preview card)
    let columns_box = GtkBox::new(Orientation::Horizontal, 14);
    columns_box.set_vexpand(true);

    // --- LEFT COLUMN: Results List ---
    let left_col = GtkBox::new(Orientation::Vertical, 6);
    left_col.set_hexpand(true);

    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .min_content_height(260)
        .vexpand(true)
        .build();

    let theme_list_box = ListBox::new();
    theme_list_box.add_css_class("boxed-list");
    scroll.set_child(Some(&theme_list_box));
    left_col.append(&scroll);
    columns_box.append(&left_col);

    // --- RIGHT COLUMN: Preview Card ---
    let right_col = GtkBox::new(Orientation::Vertical, 8);
    right_col.set_size_request(350, -1);
    right_col.add_css_class("card");
    right_col.set_margin_top(4);
    right_col.set_margin_bottom(4);
    right_col.set_margin_start(4);
    right_col.set_margin_end(4);

    let preview_header_box = GtkBox::new(Orientation::Vertical, 2);
    let preview_title = Label::new(Some("Select a theme"));
    preview_title.add_css_class("title-3");
    preview_title.set_halign(Align::Start);

    let preview_author = Label::new(Some("Click any theme on the left to preview"));
    preview_author.add_css_class("dim-label");
    preview_author.set_halign(Align::Start);

    preview_header_box.append(&preview_title);
    preview_header_box.append(&preview_author);
    right_col.append(&preview_header_box);

    // Swatches preview section
    let preview_swatches_container = GtkBox::new(Orientation::Vertical, 4);
    right_col.append(&preview_swatches_container);

    // Mockup terminal container
    let preview_mock_container = GtkBox::new(Orientation::Vertical, 0);
    right_col.append(&preview_mock_container);

    // Apply Theme button
    let apply_theme_btn = Button::with_label("Apply Theme");
    apply_theme_btn.add_css_class("suggested-action");
    apply_theme_btn.set_sensitive(false);
    right_col.append(&apply_theme_btn);

    columns_box.append(&right_col);
    marketplace_box.append(&columns_box);

    // Footer with Progress Bar and Status Label
    let footer_box = GtkBox::new(Orientation::Vertical, 4);
    let progress_bar = ProgressBar::new();
    progress_bar.set_show_text(true);
    progress_bar.set_visible(false);
    footer_box.append(&progress_bar);

    let status_label = Label::new(Some(""));
    status_label.set_halign(Align::Start);
    footer_box.append(&status_label);

    marketplace_box.append(&footer_box);

    // State for currently previewed colors and theme name
    let current_preview_colors: Arc<Mutex<Option<HashMap<String, String>>>> =
        Arc::new(Mutex::new(None));
    let current_preview_name: Arc<Mutex<Option<String>>> =
        Arc::new(Mutex::new(None));

    let update_preview_panel = {
        let p_title = preview_title.clone();
        let p_author = preview_author.clone();
        let p_swatches = preview_swatches_container.clone();
        let p_mock = preview_mock_container.clone();
        let p_apply = apply_theme_btn.clone();
        let cur_colors = current_preview_colors.clone();
        let cur_name = current_preview_name.clone();
        let cfg_prev = config_manager.clone();

        move |name: &str, author: &str, colors: &HashMap<String, String>| {
            p_title.set_text(name);

            let active_name = cfg_prev
                .borrow()
                .theme_name()
                .unwrap_or_else(|| "Default Dark".into());
            let is_active = name.eq_ignore_ascii_case(&active_name);

            if is_active {
                p_author.set_text(&format!("by {} • ✓ Current Active Theme", author));
                p_apply.set_label("✓ Current Theme");
                p_apply.set_sensitive(false);
            } else {
                p_author.set_text(&format!("by {}", author));
                p_apply.set_label("Apply Theme");
                p_apply.set_sensitive(true);
            }

            // Store in state
            *cur_colors.lock().unwrap() = Some(colors.clone());
            *cur_name.lock().unwrap() = Some(name.to_string());

            // Update Swatches
            while let Some(child) = p_swatches.first_child() {
                p_swatches.remove(&child);
            }
            let swatches = extract_key_swatches(colors);
            let sw_area = create_swatches_widget(&swatches, 330, 22);
            p_swatches.append(&sw_area);

            // Update Mock Terminal
            while let Some(child) = p_mock.first_child() {
                p_mock.remove(&child);
            }
            let mock = create_terminal_mockup(colors);
            p_mock.append(&mock);
        }
    };

    let populate_popular_list = {
        let tlb = theme_list_box.clone();
        let update_prev = update_preview_panel.clone();
        let cfg_pop = config_manager.clone();

        move || {
            tlb.remove_all();
            let active_name = cfg_pop
                .borrow()
                .theme_name()
                .unwrap_or_else(|| "Default Dark".into());

            for (name, publisher, colors) in popular_presets() {
                let row = ListBoxRow::new();
                row.set_cursor_from_name(Some("pointer"));
                row.set_activatable(true);
                row.set_selectable(true);

                let is_active = name.eq_ignore_ascii_case(&active_name);
                if is_active {
                    row.add_css_class("active-theme-row");
                }

                let row_box = GtkBox::new(Orientation::Horizontal, 10);
                row_box.set_margin_top(6);
                row_box.set_margin_bottom(6);
                row_box.set_margin_start(8);
                row_box.set_margin_end(8);

                let info_box = GtkBox::new(Orientation::Vertical, 2);
                info_box.set_hexpand(true);

                let title_box = GtkBox::new(Orientation::Horizontal, 8);
                let title = Label::new(Some(name));
                title.set_halign(Align::Start);
                title.add_css_class("heading");
                title_box.append(&title);

                if is_active {
                    let badge = Label::new(Some("✓ Active"));
                    badge.add_css_class("theme-active-badge");
                    title_box.append(&badge);
                }

                let author = Label::new(Some(&format!("by {}", publisher)));
                author.set_halign(Align::Start);
                author.add_css_class("dim-label");

                info_box.append(&title_box);
                info_box.append(&author);
                row_box.append(&info_box);

                let swatches = extract_key_swatches(&colors);
                let sw_widget = create_swatches_widget(&swatches, 90, 16);
                row_box.append(&sw_widget);

                row.set_child(Some(&row_box));

                let n_cl = name.to_string();
                let a_cl = publisher.to_string();
                let c_cl = colors.clone();
                let up_cl = update_prev.clone();

                let gesture = GestureClick::new();
                gesture.connect_released(move |_, _, _, _| {
                    up_cl(&n_cl, &a_cl, &c_cl);
                });
                row.add_controller(gesture);

                tlb.append(&row);
            }
        }
    };

    // Populate initial popular presets
    populate_popular_list();

    // Pre-select and preview the current active theme on open
    let (initial_active_name, _) = get_active_theme_display(&config_manager.borrow());
    let found_preset = popular_presets().into_iter().find(|(name, _, _)| {
        name.eq_ignore_ascii_case(&initial_active_name)
    });

    if let Some((name, pub_name, colors)) = found_preset {
        update_preview_panel(name, pub_name, &colors);
    } else if let Some(colors) = config_manager.borrow().theme_colors() {
        update_preview_panel(&initial_active_name, "Current Config", &colors);
    } else if let Some((first_name, first_pub, first_colors)) = popular_presets().into_iter().next() {
        update_preview_panel(first_name, first_pub, &first_colors);
    }

    // Apply Theme Click Handler
    {
        let cur_colors = current_preview_colors.clone();
        let cur_name = current_preview_name.clone();
        let cfg_for_theme = config_manager.clone();
        let tm_cl = theme_manager.clone();
        let win_cl = parent.clone();
        let nb_cl = notebook.clone();
        let dlg_cl = dialog.clone();

        apply_theme_btn.connect_clicked(move |_| {
            if let Some(colors) = cur_colors.lock().unwrap().clone() {
                let mut tm = tm_cl.clone();
                tm.apply_palette(&colors);
                tm.apply_to_window(&win_cl);

                let theme_name = cur_name.lock().unwrap().clone();
                cfg_for_theme.borrow_mut().set_theme(theme_name, colors.clone());

                let palette = palette_from_vscode_colors(&colors);
                for_each_terminal(&nb_cl, |view| {
                    view.set_palette(palette.clone());
                });
                dlg_cl.close();
            }
        });
    }

    // Marketplace Search Handler
    let tlb_clone = theme_list_box.clone();
    let status_label_clone = status_label.clone();
    let progress_bar_clone = progress_bar.clone();
    let update_preview_cl = update_preview_panel.clone();
    let pop_popular = populate_popular_list.clone();
    let cfg_search = config_manager.clone();

    search_entry.connect_search_changed(move |entry| {
        let text = entry.text().to_string();
        if text.trim().is_empty() {
            pop_popular();
            status_label_clone.set_text("");
            progress_bar_clone.set_visible(false);
            return;
        }

        if text.len() >= 2 {
            tlb_clone.remove_all();
            status_label_clone.set_text("Searching Marketplace...");
            progress_bar_clone.set_visible(true);
            progress_bar_clone.set_fraction(0.1);
            progress_bar_clone.set_text(Some("Consultando catálogo..."));

            let lb_cl = tlb_clone.clone();
            let status_cl = status_label_clone.clone();
            let pbar_cl = progress_bar_clone.clone();
            let up_cl = update_preview_cl.clone();
            let cfg_search_inner = cfg_search.clone();

            let (tx, rx): (async_channel::Sender<Vec<Extension>>, _) =
                async_channel::unbounded();

            gtk4::glib::spawn_future_local(async move {
                if let Ok(results) = rx.recv().await {
                    pbar_cl.set_fraction(0.3);
                    lb_cl.remove_all();
                    if results.is_empty() {
                        status_cl.set_text("No themes found. Try another query.");
                        pbar_cl.set_visible(false);
                        return;
                    }
                    status_cl.set_text(&format!("Found {} themes", results.len()));
                    pbar_cl.set_visible(false);

                    for ext in results {
                        let ext_name = ext.displayName.clone().unwrap_or(ext.extensionName.clone());
                        let pub_name = ext.publisher.publisherName.clone();

                        let row = ListBoxRow::new();
                        row.set_cursor_from_name(Some("pointer"));
                        row.set_activatable(true);
                        row.set_selectable(true);

                        let is_active = {
                            let act = cfg_search_inner
                                .borrow()
                                .theme_name()
                                .unwrap_or_else(|| "Default Dark".into());
                            ext_name.eq_ignore_ascii_case(&act)
                        };

                        if is_active {
                            row.add_css_class("active-theme-row");
                        }

                        let row_box = GtkBox::new(Orientation::Horizontal, 10);
                        row_box.set_margin_top(6);
                        row_box.set_margin_bottom(6);
                        row_box.set_margin_start(8);
                        row_box.set_margin_end(8);

                        let info_box = GtkBox::new(Orientation::Vertical, 2);
                        info_box.set_hexpand(true);

                        let title_box = GtkBox::new(Orientation::Horizontal, 8);
                        let title = Label::new(Some(&ext_name));
                        title.set_halign(Align::Start);
                        title.add_css_class("heading");
                        title_box.append(&title);

                        if is_active {
                            let badge = Label::new(Some("✓ Active"));
                            badge.add_css_class("theme-active-badge");
                            title_box.append(&badge);
                        }

                        let author = Label::new(Some(&format!("by {}", pub_name)));
                        author.set_halign(Align::Start);
                        author.add_css_class("dim-label");

                        info_box.append(&title_box);
                        info_box.append(&author);
                        row_box.append(&info_box);

                        // If built-in or cached, show swatches right away in row
                        if let Some(colors) = find_builtin_theme(&ext_name).or_else(|| get_cached_theme(&ext)) {
                            let swatches = extract_key_swatches(&colors);
                            let sw_widget = create_swatches_widget(&swatches, 90, 16);
                            row_box.append(&sw_widget);
                        }

                        row.set_child(Some(&row_box));

                        let ext_arc = Arc::new(ext);
                        let up_cl2 = up_cl.clone();
                        let pbar_cl2 = pbar_cl.clone();
                        let status_cl2 = status_cl.clone();
                        let row_box_cl = row_box.clone();
                        let ext_name_cl = ext_name.clone();
                        let pub_name_cl = pub_name.clone();

                        let gesture = GestureClick::new();
                        gesture.connect_released(move |_, _, _, _| {
                            // Check if already available in preset or cache
                            if let Some(colors) = find_builtin_theme(&ext_name_cl).or_else(|| get_cached_theme(&ext_arc)) {
                                up_cl2(&ext_name_cl, &pub_name_cl, &colors);
                                return;
                            }

                            // Download with progress
                            pbar_cl2.set_visible(true);
                            pbar_cl2.set_fraction(0.1);
                            pbar_cl2.set_text(Some("Baixando pacote..."));
                            status_cl2.set_text(&format!("Baixando {}...", ext_name_cl));

                            let (prog_tx, prog_rx) = async_channel::unbounded();
                            let ext_clone = ext_arc.clone();

                            let up_cl3 = up_cl2.clone();
                            let pbar_cl3 = pbar_cl2.clone();
                            let status_cl3 = status_cl2.clone();
                            let rb_cl = row_box_cl.clone();
                            let name_cl3 = ext_name_cl.clone();
                            let pub_cl3 = pub_name_cl.clone();

                            gtk4::glib::spawn_future_local(async move {
                                while let Ok(msg) = prog_rx.recv().await {
                                    match msg {
                                        DownloadProgress::Progress(frac, text) => {
                                            pbar_cl3.set_fraction(frac);
                                            pbar_cl3.set_text(Some(&text));
                                        }
                                        DownloadProgress::Done(colors) => {
                                            pbar_cl3.set_fraction(1.0);
                                            pbar_cl3.set_text(Some("Tema carregado!"));
                                            status_cl3.set_text(&format!("Tema {} pronto para uso.", name_cl3));

                                            // Add swatches to row if not present
                                            let swatches = extract_key_swatches(&colors);
                                            let sw_widget = create_swatches_widget(&swatches, 90, 16);
                                            rb_cl.append(&sw_widget);

                                            up_cl3(&name_cl3, &pub_cl3, &colors);
                                        }
                                        DownloadProgress::Error(err) => {
                                            pbar_cl3.set_visible(false);
                                            status_cl3.set_text(&format!("Erro ao baixar {}: {}", name_cl3, err));
                                        }
                                    }
                                }
                            });

                            std::thread::spawn(move || {
                                crate::theme_marketplace::download_and_extract_theme_with_progress(
                                    &ext_clone,
                                    prog_tx,
                                );
                            });
                        });
                        row.add_controller(gesture);

                        lb_cl.append(&row);
                    }
                }
            });

            std::thread::spawn(move || {
                let results =
                    crate::theme_marketplace::search_themes(&text).unwrap_or_default();
                let _ = tx.send_blocking(results);
            });
        }
    });

    stack.add_titled(&marketplace_box, Some("themes"), "VS Code Themes");

    // ==========================================
    // TAB 3: Window & Tabs Ergonomics
    // ==========================================
    let tabs_box = GtkBox::new(Orientation::Vertical, 16);
    tabs_box.set_margin_top(12);

    // Section 0: Default Shell
    let shell_section_title = Label::new(Some("Default Shell"));
    shell_section_title.set_halign(Align::Start);
    shell_section_title.add_css_class("heading");
    tabs_box.append(&shell_section_title);

    let shell_row = GtkBox::new(Orientation::Horizontal, 12);
    let shell_label_box = GtkBox::new(Orientation::Vertical, 2);
    shell_label_box.set_hexpand(true);
    let shell_title = Label::new(Some("Shell for new terminals"));
    shell_title.set_halign(Align::Start);
    let shell_desc = Label::new(Some("Choose which installed shell to launch in new tabs and split panes"));
    shell_desc.set_halign(Align::Start);
    shell_desc.add_css_class("dim-label");
    shell_label_box.append(&shell_title);
    shell_label_box.append(&shell_desc);

    let mut shell_labels: Vec<String> = Vec::new();
    let mut shell_values: Vec<Option<String>> = Vec::new();

    let system_default_label = match std::env::var("SHELL") {
        Ok(env_sh) if !env_sh.trim().is_empty() => format!("System Default ({})", env_sh.trim()),
        _ => "System Default ($SHELL)".to_string(),
    };
    shell_labels.push(system_default_label);
    shell_values.push(None);

    for sh in crate::pty::discover_installed_shells() {
        shell_labels.push(format!("{} ({})", sh.name, sh.path));
        shell_values.push(Some(sh.path));
    }

    let current_shell = config_manager.borrow().default_shell();
    if let Some(ref saved) = current_shell {
        if !shell_values.iter().any(|v| v.as_deref() == Some(saved.as_str())) {
            shell_labels.push(format!("{} (custom)", saved));
            shell_values.push(Some(saved.clone()));
        }
    }

    let label_refs: Vec<&str> = shell_labels.iter().map(String::as_str).collect();
    let shell_dropdown = DropDown::from_strings(&label_refs);
    shell_dropdown.set_valign(Align::Center);

    let initial_shell_idx = current_shell
        .as_deref()
        .and_then(|saved| {
            shell_values
                .iter()
                .position(|v| v.as_deref() == Some(saved))
        })
        .unwrap_or(0);
    shell_dropdown.set_selected(initial_shell_idx as u32);

    let cfg_for_shell = config_manager.clone();
    shell_dropdown.connect_selected_notify(move |dd| {
        let idx = dd.selected() as usize;
        let selected_shell = shell_values.get(idx).cloned().flatten();
        cfg_for_shell.borrow_mut().set_default_shell(selected_shell);
    });

    shell_row.append(&shell_label_box);
    shell_row.append(&shell_dropdown);
    tabs_box.append(&shell_row);

    // Section 1: Navigation & Tabs
    let tab_section_title = Label::new(Some("Tabs & Layout"));
    tab_section_title.set_halign(Align::Start);
    tab_section_title.add_css_class("heading");
    tabs_box.append(&tab_section_title);

    let orient_row = GtkBox::new(Orientation::Horizontal, 12);
    let orient_label = Label::new(Some("Vertical Tabs Layout"));
    orient_label.set_hexpand(true);
    orient_label.set_halign(Align::Start);

    let orient_switch = Switch::new();
    let is_vert = notebook.tab_pos() == gtk4::PositionType::Left;
    orient_switch.set_active(is_vert);
    orient_switch.set_valign(Align::Center);

    let nb_for_orient = notebook.clone();
    let cfg_for_orient = config_manager.clone();
    orient_switch.connect_active_notify(move |sw| {
        let (pos, orientation) = if sw.is_active() {
            (gtk4::PositionType::Left, crate::config::TabOrientation::Vertical)
        } else {
            (gtk4::PositionType::Top, crate::config::TabOrientation::Horizontal)
        };
        nb_for_orient.set_tab_pos(pos);
        cfg_for_orient.borrow_mut().set_orientation(orientation);
    });

    orient_row.append(&orient_label);
    orient_row.append(&orient_switch);
    tabs_box.append(&orient_row);

    // Double-click close tab switch
    let dbl_close_row = GtkBox::new(Orientation::Horizontal, 12);
    let dbl_close_label_box = GtkBox::new(Orientation::Vertical, 2);
    dbl_close_label_box.set_hexpand(true);
    let dbl_close_title = Label::new(Some("Close tab with double click"));
    dbl_close_title.set_halign(Align::Start);
    let dbl_close_desc = Label::new(Some("Double-click a tab header to close it; double-click empty space beside tabs to open a new tab"));
    dbl_close_desc.set_halign(Align::Start);
    dbl_close_desc.add_css_class("dim-label");
    dbl_close_label_box.append(&dbl_close_title);
    dbl_close_label_box.append(&dbl_close_desc);

    let dbl_close_switch = Switch::new();
    dbl_close_switch.set_active(config_manager.borrow().double_click_close_tab());
    dbl_close_switch.set_valign(Align::Center);
    let cfg_for_dbl_close = config_manager.clone();
    dbl_close_switch.connect_active_notify(move |sw| {
        cfg_for_dbl_close.borrow_mut().set_double_click_close_tab(sw.is_active());
    });
    dbl_close_row.append(&dbl_close_label_box);
    dbl_close_row.append(&dbl_close_switch);
    tabs_box.append(&dbl_close_row);

    // Section 2: Transparency & Color Saturation
    let win_section_title = Label::new(Some("Window Transparency & Colors"));
    win_section_title.set_halign(Align::Start);
    win_section_title.add_css_class("heading");
    tabs_box.append(&win_section_title);

    // Ctrl + Scroll toggle
    let scroll_opacity_row = GtkBox::new(Orientation::Horizontal, 12);
    let scroll_opacity_label_box = GtkBox::new(Orientation::Vertical, 2);
    scroll_opacity_label_box.set_hexpand(true);
    let scroll_opacity_title = Label::new(Some("Adjust opacity with Ctrl + Mouse Wheel"));
    scroll_opacity_title.set_halign(Align::Start);
    let scroll_opacity_desc = Label::new(Some("Scroll up to increase opacity, scroll down to decrease opacity (20% floor)"));
    scroll_opacity_desc.set_halign(Align::Start);
    scroll_opacity_desc.add_css_class("dim-label");
    scroll_opacity_label_box.append(&scroll_opacity_title);
    scroll_opacity_label_box.append(&scroll_opacity_desc);

    let scroll_opacity_switch = Switch::new();
    scroll_opacity_switch.set_active(config_manager.borrow().ctrl_scroll_opacity());
    scroll_opacity_switch.set_valign(Align::Center);
    let cfg_for_scroll_opacity = config_manager.clone();
    scroll_opacity_switch.connect_active_notify(move |sw| {
        cfg_for_scroll_opacity.borrow_mut().set_ctrl_scroll_opacity(sw.is_active());
    });
    scroll_opacity_row.append(&scroll_opacity_label_box);
    scroll_opacity_row.append(&scroll_opacity_switch);
    tabs_box.append(&scroll_opacity_row);

    // Experimental supersaturation toggle
    let exp_sat_row = GtkBox::new(Orientation::Horizontal, 12);
    let exp_sat_label_box = GtkBox::new(Orientation::Vertical, 2);
    exp_sat_label_box.set_hexpand(true);
    let exp_sat_title = Label::new(Some("Experimental: Exceed 100% opacity to supersaturate colors"));
    exp_sat_title.set_halign(Align::Start);
    let exp_sat_desc = Label::new(Some("Allows opacity adjustment up to 200%, boosting terminal vibrance and color saturation"));
    exp_sat_desc.set_halign(Align::Start);
    exp_sat_desc.add_css_class("dim-label");
    exp_sat_label_box.append(&exp_sat_title);
    exp_sat_label_box.append(&exp_sat_desc);

    let exp_sat_switch = Switch::new();
    exp_sat_switch.set_active(config_manager.borrow().experimental_supersaturation());
    exp_sat_switch.set_valign(Align::Center);
    exp_sat_row.append(&exp_sat_label_box);
    exp_sat_row.append(&exp_sat_switch);
    tabs_box.append(&exp_sat_row);

    // Opacity / Saturation Slider
    let slider_box = GtkBox::new(Orientation::Vertical, 6);
    let slider_header = GtkBox::new(Orientation::Horizontal, 12);
    let slider_title = Label::new(Some("Window Opacity / Vibrance:"));
    slider_title.set_hexpand(true);
    slider_title.set_halign(Align::Start);

    let current_level = {
        let c = config_manager.borrow();
        if c.experimental_supersaturation() && c.color_saturation() > 1.0 {
            c.color_saturation()
        } else {
            c.window_opacity()
        }
    };
    let initial_pct = (current_level * 100.0).round() as i32;
    let value_label = Label::new(Some(&if initial_pct > 100 {
        format!("{}% (Supersaturado)", initial_pct)
    } else {
        format!("{}%", initial_pct)
    }));

    slider_header.append(&slider_title);
    slider_header.append(&value_label);
    slider_box.append(&slider_header);

    let max_val = if config_manager.borrow().experimental_supersaturation() { 200.0 } else { 100.0 };
    let opacity_scale = Scale::with_range(Orientation::Horizontal, 20.0, max_val, 1.0);
    opacity_scale.set_value(initial_pct as f64);
    opacity_scale.set_hexpand(true);

    let win_for_scale = parent.clone();
    let nb_for_scale = notebook.clone();
    let cfg_for_scale = config_manager.clone();
    let val_label_clone = value_label.clone();
    opacity_scale.connect_value_changed(move |scale| {
        let pct = scale.value().round() as i32;
        let factor = (pct as f32) / 100.0;
        let experimental = cfg_for_scale.borrow().experimental_supersaturation();

        if pct > 100 && experimental {
            val_label_clone.set_text(&format!("{}% (Supersaturado)", pct));
            win_for_scale.set_opacity(1.0);
            for_each_terminal(&nb_for_scale, |tv| {
                tv.set_saturation_boost(factor);
            });
            let mut c = cfg_for_scale.borrow_mut();
            c.set_window_opacity(1.0);
            c.set_color_saturation(factor);
        } else {
            val_label_clone.set_text(&format!("{}%", pct));
            let opacity = (factor.min(1.0)).max(0.20);
            win_for_scale.set_opacity(opacity as f64);
            for_each_terminal(&nb_for_scale, |tv| {
                tv.set_saturation_boost(1.0);
            });
            let mut c = cfg_for_scale.borrow_mut();
            c.set_window_opacity(opacity);
            c.set_color_saturation(1.0);
        }
    });

    let scale_for_exp = opacity_scale.clone();
    let win_for_exp = parent.clone();
    let nb_for_exp = notebook.clone();
    let cfg_for_exp = config_manager.clone();
    let val_label_exp = value_label.clone();
    exp_sat_switch.connect_active_notify(move |sw| {
        let active = sw.is_active();
        cfg_for_exp.borrow_mut().set_experimental_supersaturation(active);
        if active {
            scale_for_exp.set_range(20.0, 200.0);
        } else {
            scale_for_exp.set_range(20.0, 100.0);
            if scale_for_exp.value() > 100.0 {
                scale_for_exp.set_value(100.0);
                val_label_exp.set_text("100%");
                win_for_exp.set_opacity(1.0);
                for_each_terminal(&nb_for_exp, |tv| {
                    tv.set_saturation_boost(1.0);
                });
                let mut c = cfg_for_exp.borrow_mut();
                c.set_window_opacity(1.0);
                c.set_color_saturation(1.0);
            }
        }
    });

    slider_box.append(&opacity_scale);
    tabs_box.append(&slider_box);

    stack.add_titled(&tabs_box, Some("layout"), "Window & Tabs");

    // ==========================================
    // TAB 4: SSH Sessions (Profiles & Keyring)
    // ==========================================
    let ssh_box = build_ssh_sessions_tab(parent, &dialog, config_manager.clone());
    stack.add_titled(&ssh_box, Some("ssh_sessions"), "SSH Sessions");

    root_box.append(&stack);
    dialog.set_child(Some(&root_box));
    dialog.present();
}

#[derive(Clone)]
struct SshFormWidgets {
    list_box: ListBox,
    displayed_ids: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    selected_id: std::rc::Rc<std::cell::RefCell<Option<String>>>,
    loaded_auth_kind: std::rc::Rc<std::cell::RefCell<Option<crate::config::SshAuthKind>>>,
    loading: std::rc::Rc<std::cell::Cell<bool>>,
    in_flight: std::rc::Rc<std::cell::Cell<bool>>,
    name_entry: Entry,
    host_entry: Entry,
    user_entry: Entry,
    port_entry: Entry,
    auth_dropdown: DropDown,
    identity_row: GtkBox,
    identity_entry: Entry,
    secret_row: GtkBox,
    secret_label: Label,
    secret_entry: PasswordEntry,
    clear_secret_btn: Button,
    mux_dropdown: DropDown,
    cm_switch: Switch,
    bypass_switch: Switch,
    status_label: Label,
    new_btn: Button,
    save_btn: Button,
    delete_btn: Button,
    connect_btn: Button,
    config_manager: std::rc::Rc<std::cell::RefCell<crate::config::ConfigManager>>,
    parent_window: ApplicationWindow,
    dialog_window: Window,
}

impl SshFormWidgets {
    fn selected_auth_kind(&self) -> crate::config::SshAuthKind {
        match self.auth_dropdown.selected() {
            1 => crate::config::SshAuthKind::IdentityFile,
            2 => crate::config::SshAuthKind::PasswordKeyring,
            _ => crate::config::SshAuthKind::AgentOrDefault,
        }
    }

    fn set_status(&self, msg: &str, is_error: bool) {
        self.status_label.remove_css_class("error");
        if is_error {
            self.status_label.add_css_class("error");
        }
        self.status_label.set_text(msg);
    }

    fn refresh_auth_fields(&self) {
        let auth = self.selected_auth_kind();
        let is_existing = self.selected_id.borrow().is_some();
        let is_identity = auth == crate::config::SshAuthKind::IdentityFile;
        let uses_secret = auth != crate::config::SshAuthKind::AgentOrDefault;

        self.identity_row.set_sensitive(is_identity);
        self.identity_entry.set_sensitive(is_identity);
        self.secret_row.set_sensitive(uses_secret);
        self.secret_entry.set_sensitive(uses_secret);

        let label_text = match auth {
            crate::config::SshAuthKind::IdentityFile => "Key Passphrase (optional, Keyring)",
            crate::config::SshAuthKind::PasswordKeyring => "SSH Password (System Keyring)",
            crate::config::SshAuthKind::AgentOrDefault => "Secret (Not used for Agent/Default)",
        };
        self.secret_label.set_text(label_text);

        let placeholder = if !uses_secret {
            "Not used with SSH Agent / Default Keys"
        } else if is_existing {
            "Leave blank to keep existing secret"
        } else if is_identity {
            "Enter passphrase or leave blank for prompt"
        } else {
            "Enter password or leave blank for prompt"
        };
        self.secret_entry
            .set_property("placeholder-text", placeholder);

        let busy = self.in_flight.get();
        self.clear_secret_btn
            .set_sensitive(!busy && is_existing && uses_secret);
        self.delete_btn.set_sensitive(!busy && is_existing);
    }

    fn set_busy(&self, busy: bool) {
        self.in_flight.set(busy);
        self.new_btn.set_sensitive(!busy);
        self.save_btn.set_sensitive(!busy);
        self.connect_btn.set_sensitive(!busy);
        self.refresh_auth_fields();
    }

    fn load_new_profile(&self) {
        self.loading.set(true);
        self.list_box.unselect_all();
        *self.selected_id.borrow_mut() = None;
        *self.loaded_auth_kind.borrow_mut() = None;

        self.name_entry.set_text("");
        self.host_entry.set_text("");
        self.user_entry.set_text("");
        self.port_entry.set_text("");
        self.auth_dropdown.set_selected(0);
        self.identity_entry.set_text("");
        self.secret_entry.set_text("");
        self.mux_dropdown.set_selected(0);
        self.cm_switch.set_active(false);
        self.bypass_switch.set_active(false);
        self.set_status("", false);

        self.loading.set(false);
        self.refresh_auth_fields();
    }

    fn load_profile(&self, p: &crate::config::SshProfile) {
        self.loading.set(true);
        *self.selected_id.borrow_mut() = Some(p.id.clone());
        *self.loaded_auth_kind.borrow_mut() = Some(p.auth_kind);

        self.name_entry.set_text(&p.name);
        self.host_entry.set_text(&p.host);
        self.user_entry.set_text(p.user.as_deref().unwrap_or(""));
        self.port_entry
            .set_text(&p.port.map(|v| v.to_string()).unwrap_or_default());

        let auth_idx = match p.auth_kind {
            crate::config::SshAuthKind::AgentOrDefault => 0,
            crate::config::SshAuthKind::IdentityFile => 1,
            crate::config::SshAuthKind::PasswordKeyring => 2,
        };
        self.auth_dropdown.set_selected(auth_idx);

        let ident = p
            .identity_file
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.identity_entry.set_text(&ident);
        self.secret_entry.set_text("");

        let first_mux = p
            .multiplexer_priority
            .iter()
            .copied()
            .find(|m| *m != crate::config::MultiplexerKind::Unknown)
            .unwrap_or(crate::config::MultiplexerKind::Tmux);
        let mux_idx = if first_mux == crate::config::MultiplexerKind::Screen {
            1
        } else {
            0
        };
        self.mux_dropdown.set_selected(mux_idx);
        self.cm_switch.set_active(p.use_control_master);
        self.bypass_switch.set_active(p.bypass_multiplexer_probe);
        self.set_status("", false);

        self.loading.set(false);
        self.refresh_auth_fields();
    }

    fn reload_list(&self, select_id: Option<&str>) {
        self.loading.set(true);
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        let profiles = self.config_manager.borrow().ssh_profiles();
        let mut ids = Vec::with_capacity(profiles.len());
        let mut row_to_select: Option<ListBoxRow> = None;

        for p in &profiles {
            ids.push(p.id.clone());

            let row_box = GtkBox::new(Orientation::Vertical, 2);
            row_box.set_margin_top(6);
            row_box.set_margin_bottom(6);
            row_box.set_margin_start(8);
            row_box.set_margin_end(8);

            let title = Label::new(Some(&p.name));
            title.set_halign(Align::Start);
            title.set_ellipsize(gtk4::pango::EllipsizeMode::End);

            let subtitle_text = format_ssh_target(p);
            let subtitle = Label::new(Some(&subtitle_text));
            subtitle.set_halign(Align::Start);
            subtitle.add_css_class("dim-label");
            subtitle.set_ellipsize(gtk4::pango::EllipsizeMode::End);

            row_box.append(&title);
            row_box.append(&subtitle);

            let row = ListBoxRow::new();
            row.set_child(Some(&row_box));
            self.list_box.append(&row);

            if select_id == Some(p.id.as_str()) {
                row_to_select = Some(row);
            }
        }

        *self.displayed_ids.borrow_mut() = ids;
        if let Some(row) = row_to_select {
            self.list_box.select_row(Some(&row));
        } else {
            self.list_box.unselect_all();
        }
        self.loading.set(false);
    }

    fn collect_validated_profile(&self) -> Result<crate::config::SshProfile, String> {
        let name = self.name_entry.text().trim().to_string();
        if name.is_empty() {
            return Err("Profile name is required.".to_string());
        }

        let host = self.host_entry.text().trim().to_string();
        if host.is_empty() {
            return Err("Host / SSH config alias is required.".to_string());
        }
        if host.starts_with('-') || host.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(
                "Host must not start with '-' or contain whitespace/control characters."
                    .to_string(),
            );
        }

        let user_raw = self.user_entry.text().trim().to_string();
        let user = if user_raw.is_empty() {
            None
        } else {
            if user_raw.starts_with('-')
                || user_raw.chars().any(|c| c.is_whitespace() || c.is_control())
            {
                return Err(
                    "User must not start with '-' or contain whitespace/control characters."
                        .to_string(),
                );
            }
            Some(user_raw)
        };

        let port_raw = self.port_entry.text().trim().to_string();
        let port = if port_raw.is_empty() {
            None
        } else {
            let parsed = port_raw
                .parse::<u16>()
                .map_err(|_| "Port must be a number between 1 and 65535.".to_string())?;
            if parsed == 0 {
                return Err("Port must be a number between 1 and 65535.".to_string());
            }
            Some(parsed)
        };

        let auth_kind = self.selected_auth_kind();
        let ident_raw = self.identity_entry.text().trim().to_string();
        let identity_file = if auth_kind == crate::config::SshAuthKind::IdentityFile {
            if ident_raw.is_empty() {
                return Err(
                    "Identity file path is required for Identity File authentication.".to_string(),
                );
            }
            if ident_raw.chars().any(|c| c.is_control()) {
                return Err("Identity file path contains invalid control characters.".to_string());
            }
            Some(std::path::PathBuf::from(ident_raw))
        } else {
            None
        };

        let multiplexer_priority = if self.mux_dropdown.selected() == 1 {
            vec![
                crate::config::MultiplexerKind::Screen,
                crate::config::MultiplexerKind::Tmux,
            ]
        } else {
            crate::config::default_multiplexer_order()
        };

        let id = self
            .selected_id
            .borrow()
            .clone()
            .unwrap_or_else(|| glib::uuid_string_random().to_string());

        Ok(crate::config::SshProfile {
            id,
            name,
            host,
            user,
            port,
            auth_kind,
            identity_file,
            multiplexer_priority,
            bypass_multiplexer_probe: self.bypass_switch.is_active(),
            use_control_master: self.cm_switch.is_active(),
        })
    }

    fn save_and_maybe_connect(&self, connect_after: bool) {
        if self.in_flight.get() {
            return;
        }

        let profile = match self.collect_validated_profile() {
            Ok(p) => p,
            Err(msg) => {
                self.set_status(&msg, true);
                return;
            }
        };

        let secret_text = self.secret_entry.text().to_string();
        let prev_auth = *self.loaded_auth_kind.borrow();
        let is_existing = self.selected_id.borrow().is_some();
        let id = profile.id.clone();

        if profile.auth_kind == crate::config::SshAuthKind::AgentOrDefault {
            let need_delete_stale = is_existing
                && matches!(
                    prev_auth,
                    Some(
                        crate::config::SshAuthKind::PasswordKeyring
                            | crate::config::SshAuthKind::IdentityFile
                    )
                );

            if !need_delete_stale {
                if let Err(e) = self.config_manager.borrow_mut().upsert_ssh_profile(profile) {
                    self.set_status(&format!("Failed to save profile: {e}"), true);
                    return;
                }
                self.finish_saved_profile(&id, crate::config::SshAuthKind::AgentOrDefault, false);
                self.set_status("Profile saved.", false);
                if connect_after {
                    self.trigger_connect(&id);
                }
                return;
            }

            self.set_busy(true);
            self.set_status("Clearing old keyring secret and saving profile...", false);
            let (tx, rx) = async_channel::bounded(1);
            crate::ssh::SecretServiceClient::delete_secret_async(id.clone(), None, tx);
            let form = self.clone();
            glib::MainContext::default().spawn_local(async move {
                let delete_res = rx.recv().await;
                let upsert_res = form.config_manager.borrow_mut().upsert_ssh_profile(profile);
                if let Err(e) = upsert_res {
                    form.set_busy(false);
                    form.set_status(&format!("Failed to save profile: {e}"), true);
                    return;
                }
                form.finish_saved_profile(&id, crate::config::SshAuthKind::AgentOrDefault, true);
                form.set_busy(false);
                match delete_res {
                    Ok(Ok(())) => {
                        form.set_status("Profile saved and old secret removed from keyring.", false);
                        if connect_after {
                            form.trigger_connect(&id);
                        }
                    }
                    Ok(Err(e)) => {
                        form.set_status(
                            &format!("Profile saved, warning clearing old secret: {e}"),
                            true,
                        );
                    }
                    Err(_) => {
                        form.set_status(
                            "Profile saved, warning: keyring worker exited unexpectedly.",
                            true,
                        );
                    }
                }
            });
            return;
        }

        let (target_kind, opposite_kind) = match profile.auth_kind {
            crate::config::SshAuthKind::PasswordKeyring => ("password", Some("passphrase")),
            crate::config::SshAuthKind::IdentityFile => ("passphrase", Some("password")),
            crate::config::SshAuthKind::AgentOrDefault => unreachable!(),
        };

        let need_store = !secret_text.is_empty();
        let need_delete_opposite = is_existing
            && matches!(
                prev_auth,
                Some(
                    crate::config::SshAuthKind::PasswordKeyring
                        | crate::config::SshAuthKind::IdentityFile
                )
            )
            && prev_auth != Some(profile.auth_kind);

        if !need_store && !need_delete_opposite {
            let auth_kind = profile.auth_kind;
            if let Err(e) = self.config_manager.borrow_mut().upsert_ssh_profile(profile) {
                self.set_status(&format!("Failed to save profile: {e}"), true);
                return;
            }
            self.finish_saved_profile(&id, auth_kind, false);
            self.set_status("Profile saved.", false);
            if connect_after {
                self.trigger_connect(&id);
            }
            return;
        }

        self.set_busy(true);
        self.set_status("Saving secret to system keyring...", false);
        let form = self.clone();
        let label = format!("WhattyTheShell SSH: {}", profile.name);
        let auth_kind = profile.auth_kind;
        glib::MainContext::default().spawn_local(async move {
            if need_store {
                let (tx, rx) = async_channel::bounded(1);
                crate::ssh::SecretServiceClient::store_secret_async(
                    id.clone(),
                    label,
                    target_kind.to_string(),
                    secret_text,
                    tx,
                );
                match rx.recv().await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        form.set_busy(false);
                        form.set_status(&format!("Keyring error (not saved): {e}"), true);
                        return;
                    }
                    Err(_) => {
                        form.set_busy(false);
                        form.set_status("Keyring worker exited unexpectedly.", true);
                        return;
                    }
                }
            }

            let mut delete_warning: Option<String> = None;
            if need_delete_opposite && let Some(opp) = opposite_kind {
                let (tx2, rx2) = async_channel::bounded(1);
                crate::ssh::SecretServiceClient::delete_secret_async(
                    id.clone(),
                    Some(opp.to_string()),
                    tx2,
                );
                match rx2.recv().await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        delete_warning = Some(format!(
                            "Saved, warning removing previous secret: {e}"
                        ));
                    }
                    Err(_) => {
                        delete_warning = Some(
                            "Saved, warning: keyring cleanup worker exited.".to_string(),
                        );
                    }
                }
            }

            if let Err(e) = form.config_manager.borrow_mut().upsert_ssh_profile(profile) {
                form.set_busy(false);
                form.set_status(&format!("Failed to save profile config: {e}"), true);
                return;
            }

            form.finish_saved_profile(&id, auth_kind, true);
            form.set_busy(false);
            if let Some(warn) = delete_warning {
                form.set_status(&warn, true);
            } else if need_store {
                form.set_status("Profile and secret saved to system keyring.", false);
            } else {
                form.set_status("Profile saved.", false);
            }
            if connect_after {
                form.trigger_connect(&id);
            }
        });
    }

    fn finish_saved_profile(
        &self,
        id: &str,
        auth_kind: crate::config::SshAuthKind,
        clear_secret_entry: bool,
    ) {
        if clear_secret_entry {
            self.secret_entry.set_text("");
        }
        *self.selected_id.borrow_mut() = Some(id.to_string());
        *self.loaded_auth_kind.borrow_mut() = Some(auth_kind);
        self.reload_list(Some(id));
        self.refresh_auth_fields();
    }

    fn trigger_connect(&self, id: &str) {
        if self.dialog_window.is_visible() {
            let _ = gtk4::prelude::WidgetExt::activate_action(
                &self.parent_window,
                "win.launch-ssh-profile",
                Some(&id.to_variant()),
            );
            self.dialog_window.close();
        }
    }
}

fn format_ssh_target(p: &crate::config::SshProfile) -> String {
    let base = match p.user.as_deref() {
        Some(u) if !u.is_empty() => format!("{u}@{}", p.host),
        _ => p.host.clone(),
    };
    match p.port {
        Some(port) => format!("{base}:{port}"),
        None => base,
    }
}

fn build_labeled_field(title: &str, control: &impl IsA<gtk4::Widget>) -> GtkBox {
    let row = GtkBox::new(Orientation::Vertical, 4);
    let label = Label::new(Some(title));
    label.set_halign(Align::Start);
    row.append(&label);
    row.append(control);
    row
}

fn build_ssh_sessions_tab(
    parent: &ApplicationWindow,
    dialog: &Window,
    config_manager: std::rc::Rc<std::cell::RefCell<crate::config::ConfigManager>>,
) -> GtkBox {
    let root = GtkBox::new(Orientation::Horizontal, 16);
    root.set_margin_top(12);
    root.set_vexpand(true);
    root.set_hexpand(true);

    // Left column: saved profile list + New Profile button
    let left_col = GtkBox::new(Orientation::Vertical, 8);
    left_col.set_width_request(230);

    let list_heading = Label::new(Some("Saved Profiles"));
    list_heading.set_halign(Align::Start);
    list_heading.add_css_class("heading");
    left_col.append(&list_heading);

    let list_box = ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::Single);
    list_box.add_css_class("boxed-list");

    let list_scroll = ScrolledWindow::new();
    list_scroll.set_vexpand(true);
    list_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    list_scroll.set_child(Some(&list_box));
    left_col.append(&list_scroll);

    let new_btn = Button::with_label("+ New Profile");
    left_col.append(&new_btn);
    root.append(&left_col);

    // Right column: editor form
    let form_scroll = ScrolledWindow::new();
    form_scroll.set_hexpand(true);
    form_scroll.set_vexpand(true);
    form_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

    let form_box = GtkBox::new(Orientation::Vertical, 10);
    form_box.set_margin_end(6);

    let name_entry = Entry::new();
    name_entry.set_placeholder_text(Some("e.g., Prod Bastion"));
    form_box.append(&build_labeled_field("Profile Name", &name_entry));

    let host_entry = Entry::new();
    host_entry.set_placeholder_text(Some("hostname, IP, or SSH config alias"));
    form_box.append(&build_labeled_field("Host / SSH Config Alias", &host_entry));

    let user_port_row = GtkBox::new(Orientation::Horizontal, 12);
    let user_entry = Entry::new();
    user_entry.set_placeholder_text(Some("optional"));
    let user_field = build_labeled_field("User (optional)", &user_entry);
    user_field.set_hexpand(true);

    let port_entry = Entry::new();
    port_entry.set_placeholder_text(Some("22"));
    port_entry.set_width_chars(8);
    let port_field = build_labeled_field("Port (optional)", &port_entry);

    user_port_row.append(&user_field);
    user_port_row.append(&port_field);
    form_box.append(&user_port_row);

    let auth_dropdown = DropDown::from_strings(&[
        "SSH Agent / Default Keys",
        "Identity File (Key)",
        "Password (System Keyring)",
    ]);
    form_box.append(&build_labeled_field("Authentication", &auth_dropdown));

    let identity_entry = Entry::new();
    identity_entry.set_placeholder_text(Some("~/.ssh/id_ed25519"));
    let identity_row = build_labeled_field("Identity File Path", &identity_entry);
    form_box.append(&identity_row);

    let secret_label = Label::new(Some("Secret (System Keyring)"));
    secret_label.set_halign(Align::Start);
    let secret_input_row = GtkBox::new(Orientation::Horizontal, 8);
    let secret_entry = PasswordEntry::new();
    secret_entry.set_show_peek_icon(true);
    secret_entry.set_hexpand(true);
    let clear_secret_btn = Button::with_label("Clear stored secret");
    secret_input_row.append(&secret_entry);
    secret_input_row.append(&clear_secret_btn);

    let secret_row = GtkBox::new(Orientation::Vertical, 4);
    secret_row.append(&secret_label);
    secret_row.append(&secret_input_row);
    form_box.append(&secret_row);

    let mux_dropdown = DropDown::from_strings(&["tmux → screen", "screen → tmux"]);
    form_box.append(&build_labeled_field("Multiplexer Priority", &mux_dropdown));

    let cm_row = GtkBox::new(Orientation::Horizontal, 12);
    let cm_label = Label::new(Some("Use ControlMaster (connection sharing)"));
    cm_label.set_halign(Align::Start);
    cm_label.set_hexpand(true);
    let cm_switch = Switch::new();
    cm_switch.set_valign(Align::Center);
    cm_row.append(&cm_label);
    cm_row.append(&cm_switch);
    form_box.append(&cm_row);

    let bypass_row = GtkBox::new(Orientation::Horizontal, 12);
    let bypass_label = Label::new(Some("Bypass multiplexer probe (ephemeral shell)"));
    bypass_label.set_halign(Align::Start);
    bypass_label.set_hexpand(true);
    let bypass_switch = Switch::new();
    bypass_switch.set_valign(Align::Center);
    bypass_row.append(&bypass_label);
    bypass_row.append(&bypass_switch);
    form_box.append(&bypass_row);

    let status_label = Label::new(None);
    status_label.set_halign(Align::Start);
    status_label.set_wrap(true);
    form_box.append(&status_label);

    let action_row = GtkBox::new(Orientation::Horizontal, 8);
    action_row.set_halign(Align::End);
    action_row.set_margin_top(6);
    let delete_btn = Button::with_label("Delete Profile");
    delete_btn.add_css_class("destructive-action");
    let save_btn = Button::with_label("Save Profile");
    let connect_btn = Button::with_label("Connect");
    connect_btn.add_css_class("suggested-action");
    action_row.append(&delete_btn);
    action_row.append(&save_btn);
    action_row.append(&connect_btn);
    form_box.append(&action_row);

    form_scroll.set_child(Some(&form_box));
    root.append(&form_scroll);

    let widgets = SshFormWidgets {
        list_box,
        displayed_ids: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        selected_id: std::rc::Rc::new(std::cell::RefCell::new(None)),
        loaded_auth_kind: std::rc::Rc::new(std::cell::RefCell::new(None)),
        loading: std::rc::Rc::new(std::cell::Cell::new(false)),
        in_flight: std::rc::Rc::new(std::cell::Cell::new(false)),
        name_entry,
        host_entry,
        user_entry,
        port_entry,
        auth_dropdown,
        identity_row,
        identity_entry,
        secret_row,
        secret_label,
        secret_entry,
        clear_secret_btn,
        mux_dropdown,
        cm_switch,
        bypass_switch,
        status_label,
        new_btn,
        save_btn,
        delete_btn,
        connect_btn,
        config_manager,
        parent_window: parent.clone(),
        dialog_window: dialog.clone(),
    };

    // Initial populate
    let initial_profiles = widgets.config_manager.borrow().ssh_profiles();
    if let Some(first) = initial_profiles.first().cloned() {
        widgets.reload_list(Some(&first.id));
        widgets.load_profile(&first);
    } else {
        widgets.reload_list(None);
        widgets.load_new_profile();
    }

    // Signals
    let w_row = widgets.clone();
    widgets.list_box.connect_row_selected(move |_, row_opt| {
        if w_row.loading.get() || w_row.in_flight.get() {
            return;
        }
        let Some(row) = row_opt else { return };
        let idx = row.index();
        if idx < 0 {
            return;
        }
        let id_opt = w_row.displayed_ids.borrow().get(idx as usize).cloned();
        if let Some(id) = id_opt
            && let Some(profile) = w_row.config_manager.borrow().get_ssh_profile(&id)
        {
            w_row.load_profile(&profile);
        }
    });

    let w_new = widgets.clone();
    widgets.new_btn.connect_clicked(move |_| {
        if !w_new.in_flight.get() {
            w_new.load_new_profile();
        }
    });

    let w_auth = widgets.clone();
    widgets.auth_dropdown.connect_selected_notify(move |_| {
        if !w_auth.loading.get() {
            w_auth.refresh_auth_fields();
        }
    });

    let w_save = widgets.clone();
    widgets.save_btn.connect_clicked(move |_| {
        w_save.save_and_maybe_connect(false);
    });

    let w_connect = widgets.clone();
    widgets.connect_btn.connect_clicked(move |_| {
        w_connect.save_and_maybe_connect(true);
    });

    let w_clear = widgets.clone();
    widgets.clear_secret_btn.connect_clicked(move |_| {
        if w_clear.in_flight.get() {
            return;
        }
        let Some(id) = w_clear.selected_id.borrow().clone() else {
            return;
        };
        w_clear.set_busy(true);
        w_clear.set_status("Clearing stored secret from keyring...", false);
        let (tx, rx) = async_channel::bounded(1);
        crate::ssh::SecretServiceClient::delete_secret_async(id, None, tx);
        let form = w_clear.clone();
        glib::MainContext::default().spawn_local(async move {
            let res = rx.recv().await;
            form.set_busy(false);
            match res {
                Ok(Ok(())) => {
                    form.secret_entry.set_text("");
                    form.set_status("Stored secret cleared from keyring.", false);
                }
                Ok(Err(e)) => {
                    form.set_status(&format!("Failed to clear keyring secret: {e}"), true);
                }
                Err(_) => {
                    form.set_status("Keyring worker exited unexpectedly.", true);
                }
            }
        });
    });

    let w_del = widgets.clone();
    widgets.delete_btn.connect_clicked(move |_| {
        if w_del.in_flight.get() {
            return;
        }
        let Some(id) = w_del.selected_id.borrow().clone() else {
            return;
        };
        match w_del.config_manager.borrow_mut().remove_ssh_profile(&id) {
            Ok(_) => {
                w_del.set_busy(true);
                w_del.set_status("Deleting profile and keyring secret...", false);
                let (tx, rx) = async_channel::bounded(1);
                crate::ssh::SecretServiceClient::delete_secret_async(id, None, tx);
                let form = w_del.clone();
                glib::MainContext::default().spawn_local(async move {
                    let res = rx.recv().await;
                    form.set_busy(false);
                    let next = form.config_manager.borrow().ssh_profiles();
                    if let Some(first) = next.first().cloned() {
                        form.reload_list(Some(&first.id));
                        form.load_profile(&first);
                    } else {
                        form.reload_list(None);
                        form.load_new_profile();
                    }
                    match res {
                        Ok(Ok(())) => form.set_status("Profile and secret deleted.", false),
                        Ok(Err(e)) => form.set_status(
                            &format!("Profile deleted, warning clearing secret: {e}"),
                            true,
                        ),
                        Err(_) => {
                            form.set_status("Profile deleted; keyring worker exited.", true)
                        }
                    }
                });
            }
            Err(e) => {
                w_del.set_status(&format!("Failed to delete profile: {e}"), true);
            }
        }
    });

    root
}
