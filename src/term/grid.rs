// src/term/grid.rs
//! Cell storage for one screen (primary or alternate) plus scrollback.

use std::collections::VecDeque;

/// Colour of a cell, resolved to RGB by the renderer using the active palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Default,
    /// 0..=15 ANSI, 16..=255 xterm 256-colour cube / greys.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// SGR attribute bit flags.
pub mod flags {
    pub const BOLD: u8 = 1 << 0;
    pub const DIM: u8 = 1 << 1;
    pub const ITALIC: u8 = 1 << 2;
    pub const UNDERLINE: u8 = 1 << 3;
    pub const INVERSE: u8 = 1 << 4;
    pub const HIDDEN: u8 = 1 << 5;
    pub const STRIKE: u8 = 1 << 6;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: u8,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { c: ' ', fg: Color::Default, bg: Color::Default, flags: 0 }
    }
}

impl Cell {
    /// A blank cell that keeps the background of `pen` (background colour erase).
    pub fn blank(pen: &Cell) -> Self {
        Cell { bg: pen.bg, ..Cell::default() }
    }
}

pub type Row = Vec<Cell>;

pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    pub lines: Vec<Row>,
    pub scrollback: VecDeque<Row>,
    pub max_scrollback: usize,
}

impl Grid {
    pub fn new(cols: usize, rows: usize, max_scrollback: usize) -> Self {
        Grid {
            cols,
            rows,
            lines: vec![vec![Cell::default(); cols]; rows],
            scrollback: VecDeque::new(),
            max_scrollback,
        }
    }

    fn blank_row(&self, pen: &Cell) -> Row {
        vec![Cell::blank(pen); self.cols]
    }

    /// Scroll the region `[top, bottom]` up by `n`; lines leaving row 0 go to scrollback.
    pub fn scroll_up(&mut self, top: usize, bottom: usize, n: usize, pen: &Cell) {
        let n = n.min(bottom + 1 - top);
        for _ in 0..n {
            let line = self.lines.remove(top);
            if top == 0 && self.max_scrollback > 0 {
                if self.scrollback.len() == self.max_scrollback {
                    self.scrollback.pop_front();
                }
                self.scrollback.push_back(line);
            }
            let blank = self.blank_row(pen);
            self.lines.insert(bottom, blank);
        }
    }

    /// Like [`scroll_up`](Self::scroll_up) but never writes to scrollback (used by DL).
    pub fn scroll_up_no_history(&mut self, top: usize, bottom: usize, n: usize, pen: &Cell) {
        let n = n.min(bottom + 1 - top);
        for _ in 0..n {
            self.lines.remove(top);
            let blank = self.blank_row(pen);
            self.lines.insert(bottom, blank);
        }
    }

    /// Scroll the region `[top, bottom]` down by `n` (blank lines appear at `top`).
    pub fn scroll_down(&mut self, top: usize, bottom: usize, n: usize, pen: &Cell) {
        let n = n.min(bottom + 1 - top);
        for _ in 0..n {
            self.lines.remove(bottom);
            let blank = self.blank_row(pen);
            self.lines.insert(top, blank);
        }
    }

    pub fn clear_range(&mut self, row: usize, from: usize, to: usize, pen: &Cell) {
        let to = to.min(self.cols);
        for c in &mut self.lines[row][from.min(to)..to] {
            *c = Cell::blank(pen);
        }
    }

    pub fn clear_all(&mut self, pen: &Cell) {
        for r in 0..self.rows {
            self.clear_range(r, 0, self.cols, pen);
        }
    }

    /// Resize, pushing lines that fall off the top into scrollback.
    /// `cursor_row` is updated so the cursor stays on the same content line.
    pub fn resize(&mut self, cols: usize, rows: usize, cursor_row: &mut usize) {
        for line in self.lines.iter_mut().chain(self.scrollback.iter_mut()) {
            line.resize(cols, Cell::default());
        }
        self.cols = cols;
        if rows < self.rows {
            // Drop from the top only as much as needed to keep the cursor visible,
            // then trim from the bottom.
            let overflow = (*cursor_row + 1).saturating_sub(rows);
            for _ in 0..overflow {
                let line = self.lines.remove(0);
                if self.max_scrollback > 0 {
                    if self.scrollback.len() == self.max_scrollback {
                        self.scrollback.pop_front();
                    }
                    self.scrollback.push_back(line);
                }
            }
            *cursor_row -= overflow;
            self.lines.truncate(rows);
        } else {
            while self.lines.len() < rows {
                self.lines.push(vec![Cell::default(); cols]);
            }
        }
        self.rows = rows;
    }

    /// Plain-text dump of the visible screen (for tests / debugging).
    #[allow(dead_code)]
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.iter().map(|c| c.c).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Extract text from the given selection span (inclusive of start and end cells).
    pub fn extract_text(&self, start: (usize, usize), end: (usize, usize)) -> String {
        let (s, e) = if start <= end { (start, end) } else { (end, start) };
        if self.lines.is_empty() || self.cols == 0 || s.0 >= self.rows {
            return String::new();
        }

        let e_row = e.0.min(self.rows - 1);
        let mut result = Vec::new();

        for r in s.0..=e_row {
            let row = &self.lines[r];
            let col_start = if r == s.0 { s.1.min(self.cols - 1) } else { 0 };
            let col_end = if r == e_row { e.1.min(self.cols - 1) } else { self.cols - 1 };

            if col_start <= col_end {
                let line_str: String = row[col_start..=col_end]
                    .iter()
                    .map(|c| if c.c == '\0' { ' ' } else { c.c })
                    .collect();
                if r == e_row {
                    result.push(line_str.trim_end().to_string());
                } else {
                    result.push(line_str.trim_end().to_string());
                }
            } else {
                result.push(String::new());
            }
        }

        result.join("\n")
    }

    /// Find word boundaries around the given cell (for double-click word selection).
    pub fn word_at(&self, row: usize, col: usize) -> Option<((usize, usize), (usize, usize))> {
        if row >= self.rows || col >= self.cols {
            return None;
        }

        let cells = &self.lines[row];
        let target_char = cells[col].c;
        if target_char == ' ' || target_char == '\0' {
            return None;
        }

        fn is_word_char(c: char) -> bool {
            !c.is_whitespace()
                && c != '\0'
                && c != '"'
                && c != '\''
                && c != '`'
                && c != '('
                && c != ')'
                && c != '['
                && c != ']'
                && c != '{'
                && c != '}'
                && c != '<'
                && c != '>'
        }

        let mut start_col = col;
        while start_col > 0 && is_word_char(cells[start_col - 1].c) {
            start_col -= 1;
        }

        let mut end_col = col;
        while end_col + 1 < self.cols && is_word_char(cells[end_col + 1].c) {
            end_col += 1;
        }

        Some(((row, start_col), (row, end_col)))
    }

    /// Find an HTTP/HTTPS URL at the specified cell (for Ctrl + click hyperlink activation).
    pub fn url_at(&self, row: usize, col: usize) -> Option<(String, ((usize, usize), (usize, usize)))> {
        if row >= self.rows || col >= self.cols {
            return None;
        }

        let line_chars: Vec<char> = self.lines[row]
            .iter()
            .map(|c| if c.c == '\0' { ' ' } else { c.c })
            .collect();
        let line_str: String = line_chars.iter().collect();

        for prefix in ["https://", "http://"] {
            let mut search_idx = 0;
            while let Some(start_pos) = line_str[search_idx..].find(prefix) {
                let actual_start = search_idx + start_pos;
                let mut actual_end = actual_start;

                while actual_end < line_str.len() {
                    let ch = line_chars[actual_end];
                    if ch.is_whitespace() || ch == '"' || ch == '\'' || ch == '`' || ch == '<' || ch == '>' || ch == '(' || ch == ')' || ch == '[' || ch == ']' {
                        break;
                    }
                    actual_end += 1;
                }

                // Trim trailing punctuation like '.', ',', ';', ':'
                while actual_end > actual_start {
                    let last_ch = line_chars[actual_end - 1];
                    if last_ch == '.' || last_ch == ',' || last_ch == ';' || last_ch == ':' {
                        actual_end -= 1;
                    } else {
                        break;
                    }
                }

                if col >= actual_start && col < actual_end {
                    let url: String = line_chars[actual_start..actual_end].iter().collect();
                    return Some((url, ((row, actual_start), (row, actual_end - 1))));
                }

                search_idx = actual_start + prefix.len();
            }
        }

        None
    }
}
