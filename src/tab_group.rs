// src/tab_group.rs
//! UI container for terminal tabs using GTK4 Notebook.
//! Supports both horizontal (top) and vertical (left) tab layout.

use gtk4::prelude::*;
use gtk4::{Label, Notebook, PositionType};
use crate::config::TabOrientation;
use crate::widget::TerminalView;

pub struct TabGroup {
    orientation: TabOrientation,
    notebook: Notebook,
}

impl TabGroup {
    /// Create a TabGroup with the requested orientation.
    pub fn new(orientation: TabOrientation) -> Self {
        let notebook = Notebook::new();
        notebook.set_hexpand(true);
        notebook.set_vexpand(true);
        notebook.set_scrollable(true);

        let pos = match orientation {
            TabOrientation::Horizontal => PositionType::Top,
            TabOrientation::Vertical => PositionType::Left,
        };
        notebook.set_tab_pos(pos);

        TabGroup {
            orientation,
            notebook,
        }
    }

    /// Add a new terminal tab.
    #[allow(dead_code)]
    pub fn add_tab(&self, view: &TerminalView, title: &str) -> u32 {
        let label = Label::new(Some(title));
        self.notebook.append_page(view, Some(&label))
    }

    /// Return the underlying widget for insertion into the UI hierarchy.
    pub fn widget(&self) -> &Notebook {
        &self.notebook
    }

    /// Change orientation at runtime (e.g., user toggles vertical tabs).
    #[allow(dead_code)]
    pub fn set_orientation(&mut self, orientation: TabOrientation) {
        self.orientation = orientation;
        let pos = match orientation {
            TabOrientation::Horizontal => PositionType::Top,
            TabOrientation::Vertical => PositionType::Left,
        };
        self.notebook.set_tab_pos(pos);
    }
}
