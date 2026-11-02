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
    pub const WIDE_SPACER: u8 = 1 << 7;
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

    /// Returns the row at visible position `r` (0..self.rows) taking `display_offset` into account.
    /// `display_offset == 0` corresponds to the active/live screen lines.
    /// `display_offset > 0` scrolls into the scrollback history.
    pub fn visible_row(&self, r: usize, display_offset: usize) -> Option<&Row> {
        if r >= self.rows {
            return None;
        }
        let dist_from_bottom = (self.rows - 1 - r).checked_add(display_offset)?;
        if dist_from_bottom < self.rows {
            let line_idx = self.rows - 1 - dist_from_bottom;
            self.lines.get(line_idx)
        } else {
            let offset_into_scrollback = dist_from_bottom - self.rows + 1;
            if offset_into_scrollback <= self.scrollback.len() {
                let sb_idx = self.scrollback.len() - offset_into_scrollback;
                self.scrollback.get(sb_idx)
            } else {
                None
            }
        }
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

    /// Extract text from the given selection span on the live screen (offset 0).
    pub fn extract_text(&self, start: (usize, usize), end: (usize, usize)) -> String {
        self.extract_text_with_offset(start, end, 0)
    }

    /// Extract text from the given selection span taking `display_offset` into account.
    pub fn extract_text_with_offset(
        &self,
        start: (usize, usize),
        end: (usize, usize),
        display_offset: usize,
    ) -> String {
        let (s, e) = if start <= end { (start, end) } else { (end, start) };
        if self.lines.is_empty() || self.cols == 0 || s.0 >= self.rows {
            return String::new();
        }

        let e_row = e.0.min(self.rows - 1);
        let mut result = Vec::new();

        for r in s.0..=e_row {
            let row = match self.visible_row(r, display_offset) {
                Some(row) => row,
                None => continue,
            };
            let col_start = if r == s.0 { s.1.min(self.cols - 1) } else { 0 };
            let col_end = if r == e_row { e.1.min(self.cols - 1) } else { self.cols - 1 };

            if col_start <= col_end {
                let line_str: String = row[col_start..=col_end]
                    .iter()
                    .filter(|c| (c.flags & flags::WIDE_SPACER) == 0)
                    .map(|c| if c.c == '\0' { ' ' } else { c.c })
                    .collect();
                result.push(line_str.trim_end().to_string());
            } else {
                result.push(String::new());
            }
        }

        result.join("\n")
    }

    /// Find word boundaries around the given cell (for double-click word selection) on live screen.
    pub fn word_at(&self, row: usize, col: usize) -> Option<((usize, usize), (usize, usize))> {
        self.word_at_with_offset(row, col, 0)
    }

    /// Find word boundaries around the given cell taking `display_offset` into account.
    pub fn word_at_with_offset(
        &self,
        row: usize,
        col: usize,
        display_offset: usize,
    ) -> Option<((usize, usize), (usize, usize))> {
        if row >= self.rows || col >= self.cols {
            return None;
        }

        let cells = self.visible_row(row, display_offset)?;
        let actual_col = if col > 0 && (cells[col].flags & flags::WIDE_SPACER) != 0 {
            col - 1
        } else {
            col
        };
        let target_char = cells[actual_col].c;
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

    /// Find a URL or hyperlink (HTTP/HTTPS/file or markdown link) at the specified cell (for Ctrl + click hyperlink activation) on live screen.
    pub fn url_at(&self, row: usize, col: usize) -> Option<(String, ((usize, usize), (usize, usize)))> {
        self.url_at_with_offset(row, col, 0)
    }

    /// Find a URL, file path, or markdown link at the specified cell taking `display_offset` into account.
    pub fn url_at_with_offset(
        &self,
        row: usize,
        col: usize,
        display_offset: usize,
    ) -> Option<(String, ((usize, usize), (usize, usize)))> {
        if row >= self.rows || col >= self.cols {
            return None;
        }

        let cells = self.visible_row(row, display_offset)?;
        let line_chars: Vec<char> = cells
            .iter()
            .map(|c| if c.c == '\0' { ' ' } else { c.c })
            .collect();

        // 1. Check if row is part of a wrapped line
        // A row wraps into next row if its last character is not space/null
        let row_wraps = |r: usize| -> bool {
            if let Some(r_cells) = self.visible_row(r, display_offset) {
                if let Some(last) = r_cells.last() {
                    return last.c != ' ' && last.c != '\0';
                }
            }
            false
        };

        let start_row = if row > 0 && row_wraps(row - 1) {
            if row >= 2 && row_wraps(row - 2) {
                row - 2
            } else {
                row - 1
            }
        } else {
            row
        };

        let end_row = if row_wraps(row) {
            if row + 1 < self.rows && row_wraps(row + 1) {
                (row + 2).min(self.rows - 1)
            } else {
                (row + 1).min(self.rows - 1)
            }
        } else {
            row
        };

        if start_row < end_row {
            let mut joined_chars = Vec::new();
            for r in start_row..=end_row {
                if let Some(r_cells) = self.visible_row(r, display_offset) {
                    let mut r_chars: Vec<char> = r_cells
                        .iter()
                        .map(|c| if c.c == '\0' { ' ' } else { c.c })
                        .collect();
                    r_chars.resize(self.cols, ' ');
                    joined_chars.extend(r_chars);
                }
            }

            let joined_matches = find_links_in_chars(&joined_chars);
            let offset_in_joined = (row - start_row) * self.cols + col;
            for (target, s, e) in joined_matches {
                if offset_in_joined >= s && offset_in_joined <= e {
                    let s_row = start_row + s / self.cols;
                    let s_col = s % self.cols;
                    let e_row = start_row + e / self.cols;
                    let e_col = e % self.cols;
                    return Some((target, ((s_row, s_col), (e_row, e_col))));
                }
            }
        }

        // 2. Try single line matching
        let matches = find_links_in_chars(&line_chars);
        for (target, s, e) in matches {
            if col >= s && col <= e {
                return Some((target, ((row, s), (row, e))));
            }
        }

        None
    }
}

fn is_path_char(c: char) -> bool {
    c.is_alphanumeric()
        || c == '/'
        || c == '_'
        || c == '-'
        || c == '.'
        || c == '+'
        || c == '@'
        || c == '%'
        || c == '~'
        || c == '!'
        || c == ':'
        || c == '#'
}

fn is_token_boundary(c: char) -> bool {
    c.is_whitespace() || c == '"' || c == '\'' || c == '`' || c == '<' || c == '>' || c == '(' || c == ')' || c == '[' || c == ']' || c == '{' || c == '}' || c == '='
}

fn is_valid_link_target(target: &str) -> bool {
    if target.is_empty() {
        return false;
    }
    target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("file://")
        || target.starts_with('/')
        || target.starts_with("~/")
        || target.starts_with("./")
        || target.starts_with("../")
        || is_source_file_path(target)
}

fn is_valid_absolute_path(path: &str) -> bool {
    if !path.starts_with('/') || path.len() < 2 {
        return false;
    }
    let clean = path.split(&[':', '#'][..]).next().unwrap_or(path);
    if clean.len() < 2 || clean.starts_with("//") {
        return false;
    }
    let segments: Vec<&str> = clean.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return false;
    }
    let first = segments[0];
    let is_std_root = matches!(
        first,
        "home" | "usr" | "etc" | "tmp" | "var" | "opt" | "root" | "dev" | "proc" | "sys" | "mnt" | "media" | "bin" | "lib" | "lib64"
    );
    if is_std_root {
        true
    } else {
        segments.len() >= 2
    }
}

fn is_source_file_path(token: &str) -> bool {
    let clean = token.split(&[':', '#'][..]).next().unwrap_or(token);
    if clean.is_empty() {
        return false;
    }

    let file_name = clean.split('/').last().unwrap_or(clean);
    let known_exact_files = [
        "Cargo.toml", "Cargo.lock", "Makefile", "Dockerfile", "README.md",
        "LICENSE", "package.json", "package-lock.json", "tsconfig.json",
        ".gitignore", "rules.json", "firestore.rules"
    ];
    if known_exact_files.iter().any(|&f| f.eq_ignore_ascii_case(file_name)) {
        return true;
    }

    if let Some(ext) = clean.rsplit('.').next() {
        if ext == clean {
            return false;
        }
        let ext_lower = ext.to_ascii_lowercase();
        let known_extensions = [
            "rs", "toml", "json", "md", "txt", "c", "h", "cpp", "hpp", "cc",
            "py", "js", "ts", "tsx", "jsx", "go", "sh", "bash", "zsh", "yaml",
            "yml", "lock", "rules", "diff", "patch", "html", "css", "xml", "svg",
            "png", "jpg", "jpeg", "gif", "webp", "ini", "conf", "sql", "proto"
        ];
        if known_extensions.contains(&ext_lower.as_str()) {
            if clean.contains('/') || token.contains(':') || token.contains('#') {
                return true;
            }
            let stem = file_name.strip_suffix(&format!(".{}", ext)).unwrap_or("");
            return stem.len() >= 2;
        }
    }

    false
}

pub fn find_links_in_chars(chars: &[char]) -> Vec<(String, usize, usize)> {
    let mut results = Vec::new();
    let len = chars.len();
    if len == 0 {
        return results;
    }

    let mut i = 0;
    while i < len {
        // 1. Markdown link: [Label](Target)
        if chars[i] == '[' {
            let start_bracket = i;
            let mut sep_idx = None;
            let mut j = start_bracket + 1;
            while j < len {
                if chars[j] == '[' {
                    break;
                }
                if chars[j] == ']' && j + 1 < len && chars[j + 1] == '(' {
                    sep_idx = Some(j);
                    break;
                }
                j += 1;
            }

            if let Some(sep) = sep_idx {
                let mut close_paren = None;
                let mut k = sep + 2;
                while k < len {
                    if chars[k] == ')' {
                        close_paren = Some(k);
                        break;
                    }
                    if chars[k].is_whitespace() || chars[k] == '[' || chars[k] == '(' {
                        break;
                    }
                    k += 1;
                }

                if let Some(cp) = close_paren {
                    let raw_target: String = chars[sep + 2..cp].iter().collect();
                    let target = raw_target.trim_matches(|c| c == '<' || c == '>').trim().to_string();
                    if is_valid_link_target(&target) {
                        results.push((target, start_bracket, cp));
                        i = cp + 1;
                        continue;
                    }
                }
            }
        }

        // 2. Explicit URI: file://, https://, http://
        let https_prefix = ['h', 't', 't', 'p', 's', ':', '/', '/'];
        let http_prefix = ['h', 't', 't', 'p', ':', '/', '/'];
        let file_prefix = ['f', 'i', 'l', 'e', ':', '/', '/'];

        let is_https = chars[i..].starts_with(&https_prefix);
        let is_http = chars[i..].starts_with(&http_prefix);
        let is_file = chars[i..].starts_with(&file_prefix);

        if is_https || is_http || is_file {
            let start_col = i;
            let prefix_len = if is_https { 8 } else { 7 };
            let mut end_col = start_col + prefix_len;

            while end_col < len {
                let ch = chars[end_col];
                if ch.is_whitespace()
                    || ch == '"'
                    || ch == '\''
                    || ch == '`'
                    || ch == '<'
                    || ch == '>'
                    || ch == '('
                    || ch == ')'
                    || ch == '['
                    || ch == ']'
                    || ch == '{'
                    || ch == '}'
                {
                    break;
                }
                end_col += 1;
            }

            while end_col > start_col + prefix_len {
                let last_ch = chars[end_col - 1];
                if last_ch == '.' || last_ch == ',' || last_ch == ';' || last_ch == ':' || last_ch == '!' || last_ch == '?' {
                    end_col -= 1;
                } else {
                    break;
                }
            }

            if end_col > start_col + prefix_len {
                let url: String = chars[start_col..end_col].iter().collect();
                results.push((url, start_col, end_col - 1));
                i = end_col;
                continue;
            }
        }

        // 3. Absolute path (/home/..., /usr/..., /etc/..., etc.)
        if chars[i] == '/' {
            let prev_is_char = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '/');
            if !prev_is_char {
                let start_col = i;
                let mut end_col = start_col + 1;
                while end_col < len {
                    let ch = chars[end_col];
                    if is_path_char(ch) {
                        end_col += 1;
                    } else {
                        break;
                    }
                }

                while end_col > start_col + 1 {
                    let last_ch = chars[end_col - 1];
                    if last_ch == '.' || last_ch == ',' || last_ch == ';' || last_ch == '!' || last_ch == '?' {
                        end_col -= 1;
                    } else {
                        break;
                    }
                }

                let path_candidate: String = chars[start_col..end_col].iter().collect();
                if is_valid_absolute_path(&path_candidate) {
                    results.push((path_candidate, start_col, end_col - 1));
                    i = end_col;
                    continue;
                }
            }
        }

        // 4. Home-relative path (~/...)
        if chars[i] == '~' && i + 1 < len && chars[i + 1] == '/' {
            let prev_is_char = i > 0 && chars[i - 1].is_alphanumeric();
            if !prev_is_char {
                let start_col = i;
                let mut end_col = start_col + 2;
                while end_col < len {
                    let ch = chars[end_col];
                    if is_path_char(ch) {
                        end_col += 1;
                    } else {
                        break;
                    }
                }

                while end_col > start_col + 2 {
                    let last_ch = chars[end_col - 1];
                    if last_ch == '.' || last_ch == ',' || last_ch == ';' || last_ch == '!' || last_ch == '?' {
                        end_col -= 1;
                    } else {
                        break;
                    }
                }

                if end_col > start_col + 2 {
                    let path_candidate: String = chars[start_col..end_col].iter().collect();
                    results.push((path_candidate, start_col, end_col - 1));
                    i = end_col;
                    continue;
                }
            }
        }

        // 5. Explicit relative path (./... or ../...)
        let starts_with_dot_slash = chars[i] == '.' && i + 1 < len && chars[i + 1] == '/';
        let starts_with_dot_dot_slash = chars[i] == '.' && i + 2 < len && chars[i + 1] == '.' && chars[i + 2] == '/';
        if starts_with_dot_slash || starts_with_dot_dot_slash {
            let prev_is_char = i > 0 && chars[i - 1].is_alphanumeric();
            if !prev_is_char {
                let start_col = i;
                let min_len = if starts_with_dot_slash { 2 } else { 3 };
                let mut end_col = start_col + min_len;
                while end_col < len {
                    let ch = chars[end_col];
                    if is_path_char(ch) {
                        end_col += 1;
                    } else {
                        break;
                    }
                }

                while end_col > start_col + min_len {
                    let last_ch = chars[end_col - 1];
                    if last_ch == '.' || last_ch == ',' || last_ch == ';' || last_ch == '!' || last_ch == '?' {
                        end_col -= 1;
                    } else {
                        break;
                    }
                }

                if end_col > start_col + min_len {
                    let path_candidate: String = chars[start_col..end_col].iter().collect();
                    results.push((path_candidate, start_col, end_col - 1));
                    i = end_col;
                    continue;
                }
            }
        }

        // 6. Project file / source file path with extension (e.g. src/widget.rs:883, Cargo.toml)
        let is_token_start = i == 0 || is_token_boundary(chars[i - 1]);
        if is_token_start && (chars[i].is_alphanumeric() || chars[i] == '_') {
            let start_col = i;
            let mut end_col = start_col + 1;
            while end_col < len {
                let ch = chars[end_col];
                if is_path_char(ch) {
                    end_col += 1;
                } else {
                    break;
                }
            }

            while end_col > start_col {
                let last_ch = chars[end_col - 1];
                if last_ch == '.' || last_ch == ',' || last_ch == ';' || last_ch == '!' || last_ch == '?' {
                    end_col -= 1;
                } else {
                    break;
                }
            }

            let token: String = chars[start_col..end_col].iter().collect();
            if is_source_file_path(&token) {
                results.push((token, start_col, end_col - 1));
                i = end_col;
                continue;
            }
        }

        i += 1;
    }

    results
}
