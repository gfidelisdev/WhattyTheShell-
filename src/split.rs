// src/split.rs
//! Split terminal pane management within notebook tab pages.
//! Supports up to 4 terminal panes per tab page in arbitrary horizontal
//! and vertical configurations (using nested `gtk4::Paned`).

use gtk4::prelude::*;
use gtk4::{Orientation, Paned, Widget};
use crate::widget::TerminalView;

pub const MAX_PANES_PER_TAB: usize = 4;

/// Recursively find all [`TerminalView`] instances contained inside `root`.
pub fn find_terminals(root: &Widget) -> Vec<TerminalView> {
    let mut views = Vec::new();
    if let Ok(tv) = root.clone().downcast::<TerminalView>() {
        views.push(tv);
        return views;
    }
    if let Ok(paned) = root.clone().downcast::<Paned>() {
        if let Some(start) = paned.start_child() {
            views.extend(find_terminals(&start));
        }
        if let Some(end) = paned.end_child() {
            views.extend(find_terminals(&end));
        }
        return views;
    }
    let mut child = root.first_child();
    while let Some(c) = child {
        views.extend(find_terminals(&c));
        child = c.next_sibling();
    }
    views
}

/// Find the currently focused [`TerminalView`] inside `root`, falling back
/// to the first terminal view if none has focus.
pub fn find_focused_terminal(root: &Widget) -> Option<TerminalView> {
    find_focused_terminal_with_active(root, None)
}

/// Find the currently focused [`TerminalView`] inside `root`, prioritizing
/// GTK focus, then falling back to `last_active` if it belongs to this root,
/// and finally to the first terminal in `root`.
pub fn find_focused_terminal_with_active(
    root: &Widget,
    last_active: Option<&TerminalView>,
) -> Option<TerminalView> {
    let terminals = find_terminals(root);
    if terminals.is_empty() {
        return None;
    }
    for t in &terminals {
        if t.has_focus() || t.is_focus() {
            return Some(t.clone());
        }
    }
    if let Some(last) = last_active {
        if terminals.iter().any(|t| t == last) {
            return Some(last.clone());
        }
    }
    terminals.first().cloned()
}

/// Find the root page widget in `notebook` containing `view`, along with its page index.
pub fn find_page_root(notebook: &gtk4::Notebook, view: &TerminalView) -> Option<(u32, Widget)> {
    if let Some(idx) = notebook.page_num(view) {
        return Some((idx, view.clone().upcast()));
    }
    let mut current = view.clone().upcast::<Widget>();
    while let Some(parent) = current.parent() {
        if let Some(idx) = notebook.page_num(&parent) {
            return Some((idx, parent));
        }
        current = parent;
    }
    None
}

/// Find the page root and index in `notebook` corresponding to the tab label widget `tab_label`.
pub fn find_page_by_tab_label(notebook: &gtk4::Notebook, tab_label: &Widget) -> Option<(u32, Widget)> {
    for i in 0..notebook.n_pages() {
        if let Some(page) = notebook.nth_page(Some(i)) {
            if let Some(label) = notebook.tab_label(&page) {
                if &label == tab_label {
                    return Some((i, page));
                }
            }
        }
    }
    None
}

/// Split `target_view` by inserting `new_widget` alongside it within `orientation`.
/// Returns `true` if the split succeeded, or `false` if `target_view` cannot be split
/// (e.g. tab page would exceed [`MAX_PANES_PER_TAB`] panes).
pub fn split_terminal_with_widget(
    target_view: &TerminalView,
    new_widget: &Widget,
    orientation: Orientation,
    notebook: &gtk4::Notebook,
) -> bool {
    let (page_idx, page_root) = match find_page_root(notebook, target_view) {
        Some(res) => res,
        None => return false,
    };

    let current_panes = find_terminals(&page_root).len();
    let new_panes = find_terminals(new_widget).len();
    if current_panes + new_panes > MAX_PANES_PER_TAB {
        return false;
    }

    let paned = Paned::new(orientation);
    paned.set_hexpand(true);
    paned.set_vexpand(true);
    paned.set_resize_start_child(true);
    paned.set_resize_end_child(true);
    paned.set_shrink_start_child(true);
    paned.set_shrink_end_child(true);
    paned.set_wide_handle(true);

    let initial_pos = match orientation {
        Orientation::Horizontal => (target_view.width() / 2).max(80),
        Orientation::Vertical => (target_view.height() / 2).max(50),
        _ => 200,
    };
    paned.set_position(initial_pos);

    if page_root == target_view.clone().upcast::<Widget>() {
        let tab_label = notebook.tab_label(target_view);
        notebook.remove_page(Some(page_idx));
        paned.set_start_child(Some(target_view));
        paned.set_end_child(Some(new_widget));
        notebook.insert_page(&paned, tab_label.as_ref(), Some(page_idx));
        notebook.set_tab_reorderable(&paned, true);
        notebook.set_current_page(Some(page_idx));
        true
    } else {
        let target_widget = target_view.clone().upcast::<Widget>();
        let parent = match target_widget.parent() {
            Some(p) => p,
            None => return false,
        };
        if let Ok(parent_paned) = parent.downcast::<Paned>() {
            let is_start = parent_paned.start_child() == Some(target_widget);
            if is_start {
                parent_paned.set_start_child(None::<&Widget>);
                paned.set_start_child(Some(target_view));
                paned.set_end_child(Some(new_widget));
                parent_paned.set_start_child(Some(&paned));
                true
            } else {
                parent_paned.set_end_child(None::<&Widget>);
                paned.set_start_child(Some(target_view));
                paned.set_end_child(Some(new_widget));
                parent_paned.set_end_child(Some(&paned));
                true
            }
        } else {
            false
        }
    }
}

/// Split `target_view` by inserting a new [`TerminalView`] alongside it within `orientation`.
pub fn split_terminal(
    target_view: &TerminalView,
    new_view: &TerminalView,
    orientation: Orientation,
    notebook: &gtk4::Notebook,
) -> bool {
    split_terminal_with_widget(target_view, &new_view.clone().upcast(), orientation, notebook)
}

/// Closes a terminal pane in a tab page.
/// If `view_to_close` is the only terminal in the tab, returns `None` (caller should close the whole tab page).
/// Otherwise, collapses the enclosing `Paned` so the sibling takes its place and returns `Some(sibling)`.
pub fn close_pane(
    view_to_close: &TerminalView,
    notebook: &gtk4::Notebook,
) -> Option<Widget> {
    let target_widget = view_to_close.clone().upcast::<Widget>();
    let parent = target_widget.parent()?;

    let parent_paned = match parent.downcast::<Paned>() {
        Ok(p) => p,
        Err(_) => return None, // Direct child of notebook; tab page has only 1 terminal
    };

    let is_start = parent_paned.start_child() == Some(target_widget);
    let sibling = if is_start {
        parent_paned.end_child()?
    } else {
        parent_paned.start_child()?
    };

    parent_paned.set_start_child(None::<&Widget>);
    parent_paned.set_end_child(None::<&Widget>);

    if let Some(idx) = notebook.page_num(&parent_paned) {
        let tab_label = notebook.tab_label(&parent_paned);
        notebook.remove_page(Some(idx));
        notebook.insert_page(&sibling, tab_label.as_ref(), Some(idx));
        notebook.set_tab_reorderable(&sibling, true);
        notebook.set_current_page(Some(idx));
        sibling.grab_focus();
        Some(sibling)
    } else {
        let grandparent = parent_paned.parent()?;
        if let Ok(grandparent_paned) = grandparent.downcast::<Paned>() {
            if grandparent_paned.start_child() == Some(parent_paned.clone().upcast()) {
                grandparent_paned.set_start_child(Some(&sibling));
            } else {
                grandparent_paned.set_end_child(Some(&sibling));
            }
            sibling.grab_focus();
            Some(sibling)
        } else {
            None
        }
    }
}

/// Unsplit the tab containing `view`: retains `view` as the sole terminal
/// for that tab page, removing all sibling panes and intermediate Paned widgets.
/// Returns `true` if any panes were collapsed, or `false` if `view` was already the only terminal.
pub fn unsplit_terminal(
    view: &TerminalView,
    notebook: &gtk4::Notebook,
) -> bool {
    let (page_idx, page_root) = match find_page_root(notebook, view) {
        Some(res) => res,
        None => return false,
    };

    let target_widget = view.clone().upcast::<Widget>();
    if page_root == target_widget {
        return false;
    }

    if let Some(parent) = target_widget.parent() {
        if let Ok(paned) = parent.downcast::<Paned>() {
            if paned.start_child() == Some(target_widget.clone()) {
                paned.set_start_child(None::<&Widget>);
            } else {
                paned.set_end_child(None::<&Widget>);
            }
        }
    }

    let tab_label = notebook.tab_label(&page_root);
    notebook.remove_page(Some(page_idx));
    notebook.insert_page(view, tab_label.as_ref(), Some(page_idx));
    notebook.set_tab_reorderable(view, true);
    notebook.set_current_page(Some(page_idx));
    view.grab_focus();
    true
}

/// Unsplits the currently active tab page, retaining its focused or last-active terminal.
pub fn unsplit_active_tab(
    notebook: &gtk4::Notebook,
    last_active: Option<&TerminalView>,
) -> bool {
    let cur = match notebook.current_page() {
        Some(c) => c,
        None => return false,
    };
    let page = match notebook.nth_page(Some(cur)) {
        Some(p) => p,
        None => return false,
    };
    let view = match find_focused_terminal_with_active(&page, last_active) {
        Some(v) => v,
        None => return false,
    };
    unsplit_terminal(&view, notebook)
}

/// Merges `source_tab_idx` into `target_tab_idx` alongside `target_view` (or the focused terminal of `target_tab_idx`).
/// Returns `true` if successful.
pub fn merge_tabs(
    source_tab_idx: u32,
    target_tab_idx: u32,
    orientation: Orientation,
    notebook: &gtk4::Notebook,
) -> bool {
    if source_tab_idx == target_tab_idx {
        return false;
    }
    let source_page = match notebook.nth_page(Some(source_tab_idx)) {
        Some(p) => p,
        None => return false,
    };
    let target_page = match notebook.nth_page(Some(target_tab_idx)) {
        Some(p) => p,
        None => return false,
    };

    let target_terminals = find_terminals(&target_page);
    let source_terminals = find_terminals(&source_page);
    if target_terminals.is_empty() || source_terminals.is_empty() {
        return false;
    }
    if target_terminals.len() + source_terminals.len() > MAX_PANES_PER_TAB {
        return false;
    }

    let target_view = find_focused_terminal(&target_page).unwrap_or_else(|| target_terminals[0].clone());

    // Remove source page from notebook
    notebook.remove_page(Some(source_tab_idx));

    // Now insert source_page into target_view's position
    let ok = split_terminal_with_widget(&target_view, &source_page, orientation, notebook);
    if ok {
        if let Some(first_source_view) = source_terminals.first() {
            first_source_view.grab_focus();
        }
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::Notebook;
    use std::sync::Arc;

    fn setup_device_queue() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        })).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Test Split Device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        })).ok()?;
        Some((Arc::new(device), Arc::new(queue)))
    }

    fn create_test_view(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> TerminalView {
        TerminalView::new(device, queue, Some("monospace".to_string()), 12.0).unwrap()
    }

    #[test]
    fn test_split_and_close_panes() {
        if gtk4::is_initialized() {
            if !gtk4::is_initialized_main_thread() {
                eprintln!("GTK already initialized on another thread, skipping");
                return;
            }
        } else if gtk4::init().is_err() {
            return;
        }
        let (dev, queue) = match setup_device_queue() {
            Some(dq) => dq,
            None => return,
        };

        let notebook = Notebook::new();
        let t1 = create_test_view(dev.clone(), queue.clone());
        let label1 = gtk4::Label::new(Some("Tab 1"));
        notebook.append_page(&t1, Some(&label1));

        let page0 = notebook.nth_page(Some(0)).unwrap();
        assert_eq!(find_terminals(&page0).len(), 1);

        // Split horizontally -> 2 terminals
        let t2 = create_test_view(dev.clone(), queue.clone());
        let ok = split_terminal(&t1, &t2, Orientation::Horizontal, &notebook);
        assert!(ok);
        let page0 = notebook.nth_page(Some(0)).unwrap();
        assert_eq!(find_terminals(&page0).len(), 2);

        // Split vertically on t1 -> 3 terminals
        let t3 = create_test_view(dev.clone(), queue.clone());
        let ok = split_terminal(&t1, &t3, Orientation::Vertical, &notebook);
        assert!(ok);
        let page0 = notebook.nth_page(Some(0)).unwrap();
        assert_eq!(find_terminals(&page0).len(), 3);

        // Split vertically on t2 -> 4 terminals (2x2 grid configuration)
        let t4 = create_test_view(dev.clone(), queue.clone());
        let ok = split_terminal(&t2, &t4, Orientation::Vertical, &notebook);
        assert!(ok);
        let page0 = notebook.nth_page(Some(0)).unwrap();
        assert_eq!(find_terminals(&page0).len(), 4);

        // Attempt 5th terminal -> should fail (capped at MAX_PANES_PER_TAB = 4)
        let t5 = create_test_view(dev.clone(), queue.clone());
        let ok = split_terminal(&t1, &t5, Orientation::Horizontal, &notebook);
        assert!(!ok);
        let page0 = notebook.nth_page(Some(0)).unwrap();
        assert_eq!(find_terminals(&page0).len(), 4);

        // Close pane t1 -> collapses paned, 3 terminals remain
        let closed = close_pane(&t1, &notebook);
        assert!(closed.is_some());
        let page0 = notebook.nth_page(Some(0)).unwrap();
        assert_eq!(find_terminals(&page0).len(), 3);

        // Close pane t3 -> 2 terminals remain
        let closed = close_pane(&t3, &notebook);
        assert!(closed.is_some());
        let page0 = notebook.nth_page(Some(0)).unwrap();
        assert_eq!(find_terminals(&page0).len(), 2);

        // Close pane t4 -> 1 terminal remains
        let closed = close_pane(&t4, &notebook);
        assert!(closed.is_some());
        let page0 = notebook.nth_page(Some(0)).unwrap();
        assert_eq!(find_terminals(&page0).len(), 1);

        // Close the last remaining terminal t2 -> returns None (tab itself should be closed)
        let closed = close_pane(&t2, &notebook);
        assert!(closed.is_none());
    }

    #[test]
    fn test_split_4_horizontal_and_unsplit() {
        if gtk4::is_initialized() {
            if !gtk4::is_initialized_main_thread() {
                return;
            }
        } else if gtk4::init().is_err() {
            return;
        }
        let (dev, queue) = match setup_device_queue() {
            Some(dq) => dq,
            None => return,
        };

        let notebook = Notebook::new();
        let t1 = create_test_view(dev.clone(), queue.clone());
        let label1 = gtk4::Label::new(Some("Tab 1"));
        notebook.append_page(&t1, Some(&label1));

        let t2 = create_test_view(dev.clone(), queue.clone());
        assert!(split_terminal(&t1, &t2, Orientation::Horizontal, &notebook));

        let t3 = create_test_view(dev.clone(), queue.clone());
        assert!(split_terminal(&t2, &t3, Orientation::Horizontal, &notebook));

        let t4 = create_test_view(dev.clone(), queue.clone());
        assert!(split_terminal(&t3, &t4, Orientation::Horizontal, &notebook));

        let page0 = notebook.nth_page(Some(0)).unwrap();
        assert_eq!(find_terminals(&page0).len(), 4);

        // Unsplit keeping t3
        let unsplit_ok = unsplit_terminal(&t3, &notebook);
        assert!(unsplit_ok);
        let page0 = notebook.nth_page(Some(0)).unwrap();
        assert_eq!(find_terminals(&page0).len(), 1);

        // Tab label must still match label1
        let found = find_page_by_tab_label(&notebook, &label1.upcast());
        assert!(found.is_some());
    }

    #[test]
    fn test_split_4_vertical() {
        if gtk4::is_initialized() {
            if !gtk4::is_initialized_main_thread() {
                return;
            }
        } else if gtk4::init().is_err() {
            return;
        }
        let (dev, queue) = match setup_device_queue() {
            Some(dq) => dq,
            None => return,
        };

        let notebook = Notebook::new();
        let t1 = create_test_view(dev.clone(), queue.clone());
        notebook.append_page(&t1, Some(&gtk4::Label::new(Some("Tab 1"))));

        let t2 = create_test_view(dev.clone(), queue.clone());
        assert!(split_terminal(&t1, &t2, Orientation::Vertical, &notebook));

        let t3 = create_test_view(dev.clone(), queue.clone());
        assert!(split_terminal(&t2, &t3, Orientation::Vertical, &notebook));

        let t4 = create_test_view(dev.clone(), queue.clone());
        assert!(split_terminal(&t3, &t4, Orientation::Vertical, &notebook));

        let page0 = notebook.nth_page(Some(0)).unwrap();
        assert_eq!(find_terminals(&page0).len(), 4);
    }

    #[test]
    fn test_close_pane_and_close_tab_with_real_tab_box() {
        if gtk4::is_initialized() {
            if !gtk4::is_initialized_main_thread() {
                return;
            }
        } else if gtk4::init().is_err() {
            return;
        }
        let (dev, queue) = match setup_device_queue() {
            Some(dq) => dq,
            None => return,
        };

        let notebook = Notebook::new();
        let t1 = create_test_view(dev.clone(), queue.clone());
        let tab_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        let lbl = gtk4::Label::new(Some("Tab 1"));
        let close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
        tab_box.append(&lbl);
        tab_box.append(&close_btn);

        let popover = gtk4::Popover::new();
        popover.set_parent(&tab_box);

        notebook.append_page(&t1, Some(&tab_box));
        assert_eq!(notebook.n_pages(), 1);

        // Split horizontally
        let t2 = create_test_view(dev.clone(), queue.clone());
        assert!(split_terminal(&t1, &t2, Orientation::Horizontal, &notebook));

        // Find page by tab label
        let found = find_page_by_tab_label(&notebook, &tab_box.clone().upcast());
        assert!(found.is_some());

        // Close pane t1
        let closed = close_pane(&t1, &notebook);
        assert!(closed.is_some());

        // Tab label must still match tab_box
        let found2 = find_page_by_tab_label(&notebook, &tab_box.clone().upcast());
        assert!(found2.is_some());

        // Close tab page
        let page_idx = found2.unwrap().0;
        notebook.remove_page(Some(page_idx));
        assert_eq!(notebook.n_pages(), 0);
    }

    #[test]
    fn test_merge_tabs() {
        if gtk4::is_initialized() {
            if !gtk4::is_initialized_main_thread() {
                eprintln!("GTK already initialized on another thread, skipping");
                return;
            }
        } else if gtk4::init().is_err() {
            return;
        }
        let (dev, queue) = match setup_device_queue() {
            Some(dq) => dq,
            None => return,
        };

        let notebook = Notebook::new();
        let t1 = create_test_view(dev.clone(), queue.clone());
        notebook.append_page(&t1, Some(&gtk4::Label::new(Some("Tab 1"))));

        let t2 = create_test_view(dev.clone(), queue.clone());
        notebook.append_page(&t2, Some(&gtk4::Label::new(Some("Tab 2"))));

        assert_eq!(notebook.n_pages(), 2);

        // Merge Tab 1 into Tab 0
        let merged = merge_tabs(1, 0, Orientation::Horizontal, &notebook);
        assert!(merged);
        assert_eq!(notebook.n_pages(), 1);
        let page0 = notebook.nth_page(Some(0)).unwrap();
        assert_eq!(find_terminals(&page0).len(), 2);
    }
}
