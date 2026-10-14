// src/settings_dialog.rs
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use gtk4::prelude::*;
use gtk4::{
    glib, Align, ApplicationWindow, Box as GtkBox, Button, DrawingArea,
    Entry, Frame, GestureClick, Label, ListBox, ListBoxRow, Notebook, Orientation,
    ProgressBar, Scale, ScrolledWindow, SearchEntry, SpinButton, Stack, StackSwitcher,
    Switch, Window,
};

use crate::render::pipeline::{hex_to_rgba, palette_from_vscode_colors, Palette};
use crate::theme_manager::ThemeManager;
use crate::theme_marketplace::{
    extract_key_swatches, find_builtin_theme, get_cached_theme, popular_presets,
    DownloadProgress, Extension,
};
use crate::widget::TerminalView;

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
    // TAB 1: Appearance (Font & Custom Colors)
    // ==========================================
    let appearance_box = GtkBox::new(Orientation::Vertical, 16);
    appearance_box.set_margin_top(12);

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

        let n_pages = nb_for_font.n_pages();
        for i in 0..n_pages {
            if let Some(page) = nb_for_font.nth_page(Some(i)) {
                if let Ok(view) = page.downcast::<TerminalView>() {
                    view.set_font(new_family.clone(), new_size);
                }
            }
        }
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
    let color_title = Label::new(Some("Custom Colors"));
    color_title.set_halign(Align::Start);
    color_title.add_css_class("heading");
    color_section.append(&color_title);

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

        let n_pages = nb_for_colors.n_pages();
        for i in 0..n_pages {
            if let Some(page) = nb_for_colors.nth_page(Some(i)) {
                if let Ok(view) = page.downcast::<TerminalView>() {
                    view.set_palette(palette.clone());
                }
            }
        }

        let mut tm = tm_for_colors.borrow_mut();
        tm.apply_palette(&map);
        tm.apply_to_window(&win_for_colors);

        cfg_for_colors.borrow_mut().set_theme(Some("Custom".into()), map);
    });

    appearance_box.append(&color_section);
    stack.add_titled(&appearance_box, Some("appearance"), "Appearance");

    // ==========================================
    // TAB 2: Marketplace Themes (2-Column Layout)
    // ==========================================
    let marketplace_box = GtkBox::new(Orientation::Vertical, 10);
    marketplace_box.set_margin_top(12);

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

        move |name: &str, author: &str, colors: &HashMap<String, String>| {
            p_title.set_text(name);
            p_author.set_text(&format!("by {}", author));

            // Store in state and ENABLE the Apply button!
            *cur_colors.lock().unwrap() = Some(colors.clone());
            *cur_name.lock().unwrap() = Some(name.to_string());
            p_apply.set_sensitive(true);

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

        move || {
            tlb.remove_all();
            for (name, publisher, colors) in popular_presets() {
                let row = ListBoxRow::new();
                row.set_cursor_from_name(Some("pointer"));
                row.set_activatable(true);
                row.set_selectable(true);

                let row_box = GtkBox::new(Orientation::Horizontal, 10);
                row_box.set_margin_top(6);
                row_box.set_margin_bottom(6);
                row_box.set_margin_start(8);
                row_box.set_margin_end(8);

                let info_box = GtkBox::new(Orientation::Vertical, 2);
                info_box.set_hexpand(true);

                let title = Label::new(Some(name));
                title.set_halign(Align::Start);
                title.add_css_class("heading");

                let author = Label::new(Some(&format!("by {}", publisher)));
                author.set_halign(Align::Start);
                author.add_css_class("dim-label");

                info_box.append(&title);
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

    // Pre-select and preview Dracula immediately on open
    if let Some((first_name, first_pub, first_colors)) = popular_presets().into_iter().next() {
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
                let n_pages = nb_cl.n_pages();
                for i in 0..n_pages {
                    if let Some(page) = nb_cl.nth_page(Some(i)) {
                        if let Ok(view) = page.downcast::<TerminalView>() {
                            view.set_palette(palette.clone());
                        }
                    }
                }
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

                        let row_box = GtkBox::new(Orientation::Horizontal, 10);
                        row_box.set_margin_top(6);
                        row_box.set_margin_bottom(6);
                        row_box.set_margin_start(8);
                        row_box.set_margin_end(8);

                        let info_box = GtkBox::new(Orientation::Vertical, 2);
                        info_box.set_hexpand(true);
                        let title = Label::new(Some(&ext_name));
                        title.set_halign(Align::Start);
                        title.add_css_class("heading");

                        let author = Label::new(Some(&format!("by {}", pub_name)));
                        author.set_halign(Align::Start);
                        author.add_css_class("dim-label");

                        info_box.append(&title);
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
            for i in 0..nb_for_scale.n_pages() {
                if let Some(page) = nb_for_scale.nth_page(Some(i)) {
                    if let Ok(tv) = page.downcast::<TerminalView>() {
                        tv.set_saturation_boost(factor);
                    }
                }
            }
            let mut c = cfg_for_scale.borrow_mut();
            c.set_window_opacity(1.0);
            c.set_color_saturation(factor);
        } else {
            val_label_clone.set_text(&format!("{}%", pct));
            let opacity = (factor.min(1.0)).max(0.20);
            win_for_scale.set_opacity(opacity as f64);
            for i in 0..nb_for_scale.n_pages() {
                if let Some(page) = nb_for_scale.nth_page(Some(i)) {
                    if let Ok(tv) = page.downcast::<TerminalView>() {
                        tv.set_saturation_boost(1.0);
                    }
                }
            }
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
                for i in 0..nb_for_exp.n_pages() {
                    if let Some(page) = nb_for_exp.nth_page(Some(i)) {
                        if let Ok(tv) = page.downcast::<TerminalView>() {
                            tv.set_saturation_boost(1.0);
                        }
                    }
                }
                let mut c = cfg_for_exp.borrow_mut();
                c.set_window_opacity(1.0);
                c.set_color_saturation(1.0);
            }
        }
    });

    slider_box.append(&opacity_scale);
    tabs_box.append(&slider_box);

    stack.add_titled(&tabs_box, Some("layout"), "Window & Tabs");

    root_box.append(&stack);
    dialog.set_child(Some(&root_box));
    dialog.present();
}
