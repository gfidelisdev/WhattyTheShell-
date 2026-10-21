// src/term/mod.rs
//! Terminal state machine: feeds PTY bytes through the `vte` parser and
//! applies the resulting actions to a [`Grid`].

pub mod grid;

use grid::{flags, Cell, Color, Grid};
use vte::{Params, Parser, Perform};

const SCROLLBACK: usize = 10_000;

#[derive(Clone, Copy, Debug, Default)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    /// Attributes applied to newly printed characters.
    pub pen: Cell,
    /// Set after printing in the last column; the next print wraps first.
    pending_wrap: bool,
}

/// Everything except the parser, so the parser can borrow it mutably.
pub struct TermState {
    primary: Grid,
    alt: Grid,
    alt_active: bool,
    pub cursor: Cursor,
    saved_cursor: Option<Cursor>,
    scroll_top: usize,
    scroll_bottom: usize,
    pub cursor_visible: bool,
    autowrap: bool,
    pub app_cursor_keys: bool,
    pub bracketed_paste: bool,
    pub title: String,
    /// Bytes the terminal must send back to the PTY (e.g. DSR replies).
    responses: Vec<u8>,
    /// Set whenever visible state changes; cleared by the renderer.
    pub dirty: bool,
    /// Number of lines the display viewport is scrolled back into history (0 = bottom / live).
    pub display_offset: usize,
    /// Stack of active Kitty Keyboard Protocol flags (progressive enhancement).
    pub kitty_keyboard_stack: Vec<u8>,
    /// Last printed character, used by REP (CSI b).
    last_char: Option<char>,
}

pub struct Terminal {
    parser: Parser,
    pub state: TermState,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize) -> Self {
        Terminal {
            parser: Parser::new(),
            state: TermState {
                primary: Grid::new(cols, rows, SCROLLBACK),
                alt: Grid::new(cols, rows, 0),
                alt_active: false,
                cursor: Cursor::default(),
                saved_cursor: None,
                scroll_top: 0,
                scroll_bottom: rows - 1,
                cursor_visible: true,
                autowrap: true,
                app_cursor_keys: false,
                bracketed_paste: false,
                title: String::new(),
                responses: Vec::new(),
                dirty: true,
                display_offset: 0,
                kitty_keyboard_stack: Vec::new(),
                last_char: None,
            },
        }
    }

    /// Feed raw bytes read from the PTY.
    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.state, bytes);
        self.state.dirty = true;
    }

    pub fn display_offset(&self) -> usize {
        self.state.display_offset
    }

    pub fn is_alt_active(&self) -> bool {
        self.state.alt_active
    }

    /// Scroll display viewport up (positive delta = older history) or down (negative delta).
    pub fn scroll_display(&mut self, delta: isize) {
        if self.state.alt_active {
            return;
        }
        let max_offset = self.state.primary.scrollback.len();
        if delta > 0 {
            self.state.display_offset = (self.state.display_offset + delta as usize).min(max_offset);
        } else {
            let sub = (-delta) as usize;
            self.state.display_offset = self.state.display_offset.saturating_sub(sub);
        }
        self.state.dirty = true;
    }

    /// Snap view back to the live shell prompt.
    pub fn scroll_to_bottom(&mut self) {
        if self.state.display_offset != 0 {
            self.state.display_offset = 0;
            self.state.dirty = true;
        }
    }

    /// Jump viewport to the very top of scrollback history.
    pub fn scroll_to_top(&mut self) {
        if !self.state.alt_active {
            let max_offset = self.state.primary.scrollback.len();
            if self.state.display_offset != max_offset {
                self.state.display_offset = max_offset;
                self.state.dirty = true;
            }
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let s = &mut self.state;
        let cols = cols.max(1);
        let rows = rows.max(1);
        let mut row = s.cursor.row;
        let mut alt_row = s.cursor.row;
        s.primary.resize(cols, rows, &mut row);
        s.alt.resize(cols, rows, &mut alt_row);
        s.cursor.row = if s.alt_active { alt_row } else { row };
        s.cursor.col = s.cursor.col.min(cols - 1);
        s.cursor.pending_wrap = false;
        s.scroll_top = 0;
        s.scroll_bottom = rows - 1;
        s.display_offset = s.display_offset.min(s.primary.scrollback.len());
        s.dirty = true;
    }

    pub fn grid(&self) -> &Grid {
        self.state.grid()
    }

    pub fn take_responses(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.state.responses)
    }

    pub fn kitty_keyboard_flags(&self) -> u8 {
        self.state.kitty_keyboard_stack.last().copied().unwrap_or(0)
    }

    pub fn extract_text(&self, start: (usize, usize), end: (usize, usize)) -> String {
        self.state.grid().extract_text_with_offset(start, end, self.state.display_offset)
    }

    pub fn word_at(&self, row: usize, col: usize) -> Option<((usize, usize), (usize, usize))> {
        self.state.grid().word_at_with_offset(row, col, self.state.display_offset)
    }

    pub fn url_at(&self, row: usize, col: usize) -> Option<(String, ((usize, usize), (usize, usize)))> {
        self.state.grid().url_at_with_offset(row, col, self.state.display_offset)
    }
}

impl TermState {
    pub fn grid(&self) -> &Grid {
        if self.alt_active { &self.alt } else { &self.primary }
    }

    fn grid_mut(&mut self) -> &mut Grid {
        if self.alt_active { &mut self.alt } else { &mut self.primary }
    }

    fn cols(&self) -> usize {
        self.grid().cols
    }

    fn rows(&self) -> usize {
        self.grid().rows
    }

    fn goto(&mut self, row: usize, col: usize) {
        self.cursor.row = row.min(self.rows() - 1);
        self.cursor.col = col.min(self.cols() - 1);
        self.cursor.pending_wrap = false;
    }

    fn linefeed(&mut self) {
        let (top, bottom, pen) = (self.scroll_top, self.scroll_bottom, self.cursor.pen);
        if self.cursor.row == bottom {
            self.grid_mut().scroll_up(top, bottom, 1, &pen);
        } else if self.cursor.row < self.rows() - 1 {
            self.cursor.row += 1;
        }
        self.cursor.pending_wrap = false;
    }

    fn reverse_index(&mut self) {
        let (top, bottom, pen) = (self.scroll_top, self.scroll_bottom, self.cursor.pen);
        if self.cursor.row == top {
            self.grid_mut().scroll_down(top, bottom, 1, &pen);
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
        }
    }

    fn set_alt_screen(&mut self, on: bool) {
        if on == self.alt_active {
            return;
        }
        self.display_offset = 0;
        if on {
            self.saved_cursor = Some(self.cursor);
            self.alt_active = true;
            let pen = self.cursor.pen;
            self.alt.clear_all(&pen);
        } else {
            self.alt_active = false;
            if let Some(c) = self.saved_cursor.take() {
                self.cursor = c;
            }
        }
    }

    fn set_mode(&mut self, private: bool, mode: u16, on: bool) {
        match (private, mode) {
            (true, 1) => self.app_cursor_keys = on,
            (true, 7) => self.autowrap = on,
            (true, 25) => self.cursor_visible = on,
            (true, 47) | (true, 1047) | (true, 1049) => self.set_alt_screen(on),
            (true, 1048) => {
                if on { self.saved_cursor = Some(self.cursor) } else if let Some(c) = self.saved_cursor { self.cursor = c }
            }
            (true, 2004) => self.bracketed_paste = on,
            _ => {}
        }
    }

    fn sgr(&mut self, params: &Params) {
        let pen = &mut self.cursor.pen;
        if params.is_empty() {
            *pen = Cell::default();
            return;
        }
        let mut it = params.iter();
        while let Some(p) = it.next() {
            match p[0] {
                0 => *pen = Cell::default(),
                1 => pen.flags |= flags::BOLD,
                2 => pen.flags |= flags::DIM,
                3 => pen.flags |= flags::ITALIC,
                4 => pen.flags |= flags::UNDERLINE,
                7 => pen.flags |= flags::INVERSE,
                8 => pen.flags |= flags::HIDDEN,
                9 => pen.flags |= flags::STRIKE,
                21 | 22 => pen.flags &= !(flags::BOLD | flags::DIM),
                23 => pen.flags &= !flags::ITALIC,
                24 => pen.flags &= !flags::UNDERLINE,
                27 => pen.flags &= !flags::INVERSE,
                28 => pen.flags &= !flags::HIDDEN,
                29 => pen.flags &= !flags::STRIKE,
                n @ 30..=37 => pen.fg = Color::Indexed((n - 30) as u8),
                39 => pen.fg = Color::Default,
                n @ 40..=47 => pen.bg = Color::Indexed((n - 40) as u8),
                49 => pen.bg = Color::Default,
                n @ 90..=97 => pen.fg = Color::Indexed((n - 90 + 8) as u8),
                n @ 100..=107 => pen.bg = Color::Indexed((n - 100 + 8) as u8),
                n @ (38 | 48) => {
                    // Either colon form (38:2:r:g:b in one param) or semicolon form.
                    let color = if p.len() > 1 {
                        parse_ext_color(&p[1..])
                    } else {
                        match it.next().map(|x| x[0]) {
                            Some(5) => it.next().map(|x| Color::Indexed(x[0] as u8)),
                            Some(2) => {
                                let r = it.next().map(|x| x[0] as u8);
                                let g = it.next().map(|x| x[0] as u8);
                                let b = it.next().map(|x| x[0] as u8);
                                match (r, g, b) {
                                    (Some(r), Some(g), Some(b)) => Some(Color::Rgb(r, g, b)),
                                    _ => None,
                                }
                            }
                            _ => None,
                        }
                    };
                    if let Some(c) = color {
                        if n == 38 { pen.fg = c } else { pen.bg = c }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Parse the sub-parameters of `38:…` / `48:…`.
fn parse_ext_color(sub: &[u16]) -> Option<Color> {
    match sub {
        [5, idx, ..] => Some(Color::Indexed(*idx as u8)),
        // 38:2:r:g:b and 38:2:<colorspace>:r:g:b
        [2, r, g, b] => Some(Color::Rgb(*r as u8, *g as u8, *b as u8)),
        [2, _, r, g, b, ..] => Some(Color::Rgb(*r as u8, *g as u8, *b as u8)),
        _ => None,
    }
}


impl TermState {
    fn print_char(&mut self, c: char) {
        use unicode_width::UnicodeWidthChar;
        self.last_char = Some(c);
        let width = c.width().unwrap_or(1);
        if width == 0 {
            // Zero-width / combining character
            return;
        }

        if width >= 2 {
            if self.cursor.pending_wrap && self.autowrap {
                self.cursor.col = 0;
                self.linefeed();
            }
            if self.cursor.col + 1 >= self.cols() && self.autowrap {
                self.cursor.col = 0;
                self.linefeed();
            }

            let (row, col, pen) = (self.cursor.row, self.cursor.col, self.cursor.pen);
            self.grid_mut().lines[row][col] = Cell { c, ..pen };
            if col + 1 < self.cols() {
                self.grid_mut().lines[row][col + 1] = Cell {
                    c: ' ',
                    flags: pen.flags | flags::WIDE_SPACER,
                    ..pen
                };
            }
            if col + 2 >= self.cols() {
                self.cursor.col = self.cols() - 1;
                self.cursor.pending_wrap = true;
            } else {
                self.cursor.col += 2;
            }
        } else {
            if self.cursor.pending_wrap && self.autowrap {
                self.cursor.col = 0;
                self.linefeed();
            }
            let (row, col, pen) = (self.cursor.row, self.cursor.col, self.cursor.pen);
            self.grid_mut().lines[row][col] = Cell { c, ..pen };
            if col + 1 >= self.cols() {
                self.cursor.pending_wrap = true;
            } else {
                self.cursor.col += 1;
            }
        }
    }
}

impl Perform for TermState {
    fn print(&mut self, c: char) {
        self.print_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0b | 0x0c => self.linefeed(),
            b'\r' => {
                self.cursor.col = 0;
                self.cursor.pending_wrap = false;
            }
            0x08 => {
                self.cursor.col = self.cursor.col.saturating_sub(1);
                self.cursor.pending_wrap = false;
            }
            b'\t' => {
                let next = (self.cursor.col / 8 + 1) * 8;
                self.cursor.col = next.min(self.cols() - 1);
                self.cursor.pending_wrap = false;
            }
            _ => {} // BEL etc. ignored for now
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        let ps: Vec<u16> = params.iter().map(|p| p[0]).collect();
        // Parameter `i`, where 0/missing means `default`.
        let arg = |i: usize, default: usize| match ps.get(i) {
            Some(&0) | None => default,
            Some(&v) => v as usize,
        };
        let private = intermediates.first() == Some(&b'?');
        let (row, col, pen) = (self.cursor.row, self.cursor.col, self.cursor.pen);
        let (cols, rows) = (self.cols(), self.rows());

        match (action, private) {
            ('A', false) => self.goto(row.saturating_sub(arg(0, 1)), col),
            ('B', false) | ('e', false) => self.goto(row + arg(0, 1), col),
            ('C', false) | ('a', false) => self.goto(row, col + arg(0, 1)),
            ('D', false) => self.goto(row, col.saturating_sub(arg(0, 1))),
            ('E', false) => self.goto(row + arg(0, 1), 0),
            ('F', false) => self.goto(row.saturating_sub(arg(0, 1)), 0),
            ('I', false) => {
                // CHT: cursor forward tabulation
                let mut c = col;
                for _ in 0..arg(0, 1) { c = (c / 8 + 1) * 8; }
                self.goto(row, c);
            }
            ('Z', false) => {
                // CBT: cursor backward tabulation
                let mut c = col;
                for _ in 0..arg(0, 1) { c = if c == 0 { 0 } else { (c - 1) / 8 * 8 }; }
                self.goto(row, c);
            }
            ('b', false) => {
                // REP: repeat the preceding graphic character
                if let Some(ch) = self.last_char {
                    for _ in 0..arg(0, 1).min(65535) { self.print_char(ch); }
                }
            }
            ('G', false) | ('`', false) => self.goto(row, arg(0, 1) - 1),
            ('d', false) => self.goto(arg(0, 1) - 1, col),
            ('H', false) | ('f', false) => self.goto(arg(0, 1) - 1, arg(1, 1) - 1),
            ('J', _) if intermediates.is_empty() => {
                self.cursor.pending_wrap = false;
                let g = self.grid_mut();
                match ps.first().copied().unwrap_or(0) {
                    0 => {
                        g.clear_range(row, col, cols, &pen);
                        for r in row + 1..rows { g.clear_range(r, 0, cols, &pen) }
                    }
                    1 => {
                        for r in 0..row { g.clear_range(r, 0, cols, &pen) }
                        g.clear_range(row, 0, col + 1, &pen);
                    }
                    2 => g.clear_all(&pen),
                    3 => g.scrollback.clear(),
                    _ => {}
                }
            }
            ('K', _) if intermediates.is_empty() => {
                self.cursor.pending_wrap = false;
                let g = self.grid_mut();
                match ps.first().copied().unwrap_or(0) {
                    0 => g.clear_range(row, col, cols, &pen),
                    1 => g.clear_range(row, 0, col + 1, &pen),
                    2 => g.clear_range(row, 0, cols, &pen),
                    _ => {}
                }
            }
            ('L', false) | ('M', false) if intermediates.is_empty() => {
                self.cursor.pending_wrap = false;
                if row >= self.scroll_top && row <= self.scroll_bottom {
                    let bottom = self.scroll_bottom;
                    let n = arg(0, 1);
                    if action == 'L' {
                        self.grid_mut().scroll_down(row, bottom, n, &pen);
                    } else {
                        self.grid_mut().scroll_up_no_history(row, bottom, n, &pen);
                    }
                }
            }
            ('P', false) if intermediates.is_empty() => {
                self.cursor.pending_wrap = false;
                let n = arg(0, 1).min(cols - col);
                let line = &mut self.grid_mut().lines[row];
                line.drain(col..col + n);
                line.extend(std::iter::repeat(Cell::blank(&pen)).take(n));
            }
            ('@', false) if intermediates.is_empty() => {
                self.cursor.pending_wrap = false;
                let n = arg(0, 1).min(cols - col);
                let line = &mut self.grid_mut().lines[row];
                for _ in 0..n { line.insert(col, Cell::blank(&pen)) }
                line.truncate(cols);
            }
            ('X', false) if intermediates.is_empty() => {
                self.cursor.pending_wrap = false;
                self.grid_mut().clear_range(row, col, col + arg(0, 1), &pen);
            }
            ('S', false) => {
                let (t, b) = (self.scroll_top, self.scroll_bottom);
                self.grid_mut().scroll_up(t, b, arg(0, 1), &pen);
            }
            ('T', false) => {
                let (t, b) = (self.scroll_top, self.scroll_bottom);
                self.grid_mut().scroll_down(t, b, arg(0, 1), &pen);
            }
            ('m', false) if intermediates.is_empty() => self.sgr(params),
            ('r', false) => {
                let top = arg(0, 1) - 1;
                let bottom = arg(1, rows).min(rows) - 1;
                if top < bottom {
                    self.scroll_top = top;
                    self.scroll_bottom = bottom;
                    self.goto(0, 0);
                }
            }
            ('s', false) => self.saved_cursor = Some(self.cursor),
            ('u', false) => {
                match intermediates.first() {
                    Some(&b'>') => {
                        let flags = arg(0, 0) as u8;
                        self.kitty_keyboard_stack.push(flags);
                    }
                    Some(&b'<') => {
                        let count = arg(0, 1).max(1);
                        for _ in 0..count {
                            self.kitty_keyboard_stack.pop();
                        }
                    }
                    Some(&b'=') => {
                        let flags = arg(0, 0) as u8;
                        let mode = arg(1, 1);
                        match mode {
                            1 => {
                                if self.kitty_keyboard_stack.is_empty() {
                                    self.kitty_keyboard_stack.push(flags);
                                } else {
                                    let idx = self.kitty_keyboard_stack.len() - 1;
                                    self.kitty_keyboard_stack[idx] = flags;
                                }
                            }
                            2 => {
                                if let Some(top) = self.kitty_keyboard_stack.last_mut() {
                                    *top |= flags;
                                } else {
                                    self.kitty_keyboard_stack.push(flags);
                                }
                            }
                            3 => {
                                if let Some(top) = self.kitty_keyboard_stack.last_mut() {
                                    *top &= !flags;
                                }
                            }
                            _ => {}
                        }
                    }
                    None => {
                        if let Some(c) = self.saved_cursor {
                            self.cursor.row = c.row.min(self.rows().saturating_sub(1));
                            self.cursor.col = c.col.min(self.cols().saturating_sub(1));
                            self.cursor.pen = c.pen;
                            self.cursor.pending_wrap = false;
                        }
                    }
                    _ => {}
                }
            }
            ('u', true) => {
                // CSI ? u -> Query Kitty Keyboard Protocol
                let flags = self.kitty_keyboard_stack.last().copied().unwrap_or(0);
                self.responses.extend(format!("\x1b[?{}u", flags).bytes());
            }
            ('h', p) | ('l', p) => {
                for &m in &ps { self.set_mode(p, m, action == 'h') }
            }
            ('n', false) => match arg(0, 0) {
                5 => self.responses.extend_from_slice(b"\x1b[0n"),
                6 => self.responses.extend(format!("\x1b[{};{}R", row + 1, col + 1).bytes()),
                _ => {}
            },
            ('c', false) => {
                if intermediates.first() == Some(&b'>') {
                    // Secondary Device Attributes (DA2): VT100 / xterm compatible version
                    self.responses.extend_from_slice(b"\x1b[>0;10;1c");
                } else if intermediates.is_empty() {
                    // Primary Device Attributes (DA1): VT102 compatible
                    self.responses.extend_from_slice(b"\x1b[?62;22c");
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        if !intermediates.is_empty() {
            return; // charset designations etc.
        }
        match byte {
            b'7' => self.saved_cursor = Some(self.cursor),
            b'8' => {
                if let Some(c) = self.saved_cursor {
                    self.cursor.row = c.row.min(self.rows().saturating_sub(1));
                    self.cursor.col = c.col.min(self.cols().saturating_sub(1));
                    self.cursor.pen = c.pen;
                    self.cursor.pending_wrap = false;
                }
            }
            b'D' => self.linefeed(),
            b'E' => {
                self.cursor.col = 0;
                self.linefeed();
            }
            b'M' => self.reverse_index(),
            b'c' => {
                let (cols, rows) = (self.cols(), self.rows());
                let title = std::mem::take(&mut self.title);
                *self = Terminal::new(cols, rows).state;
                self.title = title;
            }
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let [kind, title, ..] = params {
            if *kind == b"0" || *kind == b"2" {
                self.title = String::from_utf8_lossy(title).into_owned();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_and_wraps() {
        let mut t = Terminal::new(5, 2);
        t.advance(b"hello world");
        // "hello" scrolled into history; " worl" + "d" remain visible.
        assert_eq!(t.grid().text(), " worl\nd");
        assert_eq!(t.grid().scrollback.len(), 1);
    }

    #[test]
    fn prints_special_and_accents() {
        let mut t = Terminal::new(10, 2);
        t.advance("cd ~".as_bytes());
        assert_eq!(t.grid().lines[0][0].c, 'c');
        assert_eq!(t.grid().lines[0][1].c, 'd');
        assert_eq!(t.grid().lines[0][2].c, ' ');
        assert_eq!(t.grid().lines[0][3].c, '~');

        let mut t2 = Terminal::new(10, 2);
        t2.advance("olá".as_bytes());
        assert_eq!(t2.grid().lines[0][0].c, 'o');
        assert_eq!(t2.grid().lines[0][1].c, 'l');
        assert_eq!(t2.grid().lines[0][2].c, 'á');
    }

    #[test]
    fn cursor_movement_and_erase() {
        let mut t = Terminal::new(10, 3);
        t.advance(b"abcdef\x1b[1;3H\x1b[K");
        assert_eq!(t.grid().text(), "ab\n\n");
        t.advance(b"\x1b[2J\x1b[2;2HX");
        assert_eq!(t.grid().text(), "\n X\n");
    }

    #[test]
    fn test_kitty_keyboard_protocol() {
        let mut t = Terminal::new(10, 2);
        // Query initial flags
        t.advance(b"\x1b[?u");
        assert_eq!(t.take_responses(), b"\x1b[?0u");
        assert_eq!(t.kitty_keyboard_flags(), 0);

        // Push flag 1 (disambiguate escape codes)
        t.advance(b"\x1b[>1u");
        assert_eq!(t.kitty_keyboard_flags(), 1);

        // Query again
        t.advance(b"\x1b[?u");
        assert_eq!(t.take_responses(), b"\x1b[?1u");

        // Push flag 3
        t.advance(b"\x1b[>3u");
        assert_eq!(t.kitty_keyboard_flags(), 3);

        // Pop
        t.advance(b"\x1b[<u");
        assert_eq!(t.kitty_keyboard_flags(), 1);

        t.advance(b"\x1b[<u");
        assert_eq!(t.kitty_keyboard_flags(), 0);

        // ANSI restore cursor with empty intermediates still works
        t.advance(b"\x1b[1;2H\x1b[s\x1b[2;5H");
        assert_eq!(t.state.cursor.row, 1);
        assert_eq!(t.state.cursor.col, 4);
        t.advance(b"\x1b[u");
        assert_eq!(t.state.cursor.row, 0);
        assert_eq!(t.state.cursor.col, 1);
    }

    #[test]
    fn sgr_colours() {
        let mut t = Terminal::new(10, 1);
        t.advance(b"\x1b[1;31mA\x1b[38;2;1;2;3mB\x1b[38:5:200mC\x1b[0mD");
        let l = &t.grid().lines[0];
        assert_eq!(l[0].fg, Color::Indexed(1));
        assert!(l[0].flags & flags::BOLD != 0);
        assert_eq!(l[1].fg, Color::Rgb(1, 2, 3));
        assert_eq!(l[2].fg, Color::Indexed(200));
        assert_eq!(l[3].fg, Color::Default);
        assert_eq!(l[3].flags, 0);
    }

    #[test]
    fn alt_screen_restores_primary() {
        let mut t = Terminal::new(10, 2);
        t.advance(b"main");
        t.advance(b"\x1b[?1049h\x1b[HALT");
        assert_eq!(t.grid().text(), "ALT\n");
        t.advance(b"\x1b[?1049l");
        assert_eq!(t.grid().text(), "main\n");
        assert_eq!(t.state.cursor.col, 4);
    }

    #[test]
    fn scroll_region() {
        let mut t = Terminal::new(3, 4);
        t.advance(b"1\r\n2\r\n3\r\n4\x1b[2;3r\x1b[3;1H\n");
        assert_eq!(t.grid().text(), "1\n3\n\n4");
    }

    #[test]
    fn dsr_reply() {
        let mut t = Terminal::new(10, 5);
        t.advance(b"\x1b[3;4H\x1b[6n");
        assert_eq!(t.take_responses(), b"\x1b[3;4R");
    }

    #[test]
    fn test_extract_text_selection() {
        let mut t = Terminal::new(20, 3);
        t.advance(b"hello world\r\nsecond line here\r\nthird");

        // Single line selection: "world"
        let sel = t.extract_text((0, 6), (0, 10));
        assert_eq!(sel, "world");

        // Reverse coordinates normalized: "hello"
        let sel_rev = t.extract_text((0, 4), (0, 0));
        assert_eq!(sel_rev, "hello");

        // Multi-line selection
        let sel_multi = t.extract_text((0, 6), (1, 5));
        assert_eq!(sel_multi, "world\nsecond");
    }

    #[test]
    fn test_word_at_boundaries() {
        let mut t = Terminal::new(30, 2);
        t.advance(b"cargo run --release");

        // Word "cargo" at col 2
        let word1 = t.word_at(0, 2);
        assert_eq!(word1, Some(((0, 0), (0, 4))));

        // Word "run" at col 6
        let word2 = t.word_at(0, 6);
        assert_eq!(word2, Some(((0, 6), (0, 8))));

        // Flag "--release" at col 10
        let flag = t.word_at(0, 10);
        assert_eq!(flag, Some(((0, 10), (0, 18))));

        // Word "release" at col 13 (inside "--release")
        let word3 = t.word_at(0, 13);
        assert_eq!(word3, Some(((0, 10), (0, 18))));
    }

    #[test]
    fn test_url_detection() {
        let mut t = Terminal::new(50, 2);
        t.advance(b"visit https://example.com/test for info");

        // In URL
        let url = t.url_at(0, 10);
        assert!(url.is_some());
        let (extracted_url, span) = url.unwrap();
        assert_eq!(extracted_url, "https://example.com/test");
        assert_eq!(span, ((0, 6), (0, 29)));

        // Outside URL (col 2 is inside "visit")
        let none_url = t.url_at(0, 2);
        assert!(none_url.is_none());

        // Test with multi-byte unicode prefix (e.g. starship prompt "❯ ")
        let mut t2 = Terminal::new(90, 2);
        t2.advance("❯ git: https://github.com/gfidelis/WhattyTheShell and http://example.org".as_bytes());
        let url_unicode = t2.url_at(0, 15);
        assert!(url_unicode.is_some());
        let (extracted, span) = url_unicode.unwrap();
        assert_eq!(extracted, "https://github.com/gfidelis/WhattyTheShell");
        assert_eq!(span, ((0, 7), (0, 48)));

        // Second URL on same line
        let url_second = t2.url_at(0, 56);
        assert!(url_second.is_some());
        let (extracted2, span2) = url_second.unwrap();
        assert_eq!(extracted2, "http://example.org");
        assert_eq!(span2, ((0, 54), (0, 71)));

        // Standalone file:// URL
        let mut t3 = Terminal::new(100, 2);
        t3.advance(b"File created at file:///home/gfidelis/Code/test.txt#L10-L20 ready");
        let file_url = t3.url_at(0, 20);
        assert!(file_url.is_some());
        let (extracted3, span3) = file_url.unwrap();
        assert_eq!(extracted3, "file:///home/gfidelis/Code/test.txt#L10-L20");
        assert_eq!(span3, ((0, 16), (0, 58)));

        // Markdown link with file:// URL [Review Plan](file:///tmp/plan.md)
        let mut t4 = Terminal::new(100, 2);
        t4.advance(b"Please review [Review Plan](file:///tmp/plan.md) before continuing");
        // Clicking inside label "Review Plan"
        let md_label_click = t4.url_at(0, 20);
        assert!(md_label_click.is_some());
        let (extracted4, span4) = md_label_click.unwrap();
        assert_eq!(extracted4, "file:///tmp/plan.md");
        assert_eq!(span4, ((0, 14), (0, 47)));

        // Clicking inside URL part
        let md_url_click = t4.url_at(0, 35);
        assert!(md_url_click.is_some());
        let (extracted5, span5) = md_url_click.unwrap();
        assert_eq!(extracted5, "file:///tmp/plan.md");
        assert_eq!(span5, ((0, 14), (0, 47)));

        // Markdown link with code formatting: [`src/widget.rs`](file:///path/to/src/widget.rs#L42)
        let mut t5 = Terminal::new(100, 2);
        t5.advance(b"Updated [`src/widget.rs`](file:///path/to/src/widget.rs#L42) with changes");
        let code_click = t5.url_at(0, 12);
        assert!(code_click.is_some());
        let (extracted6, span6) = code_click.unwrap();
        assert_eq!(extracted6, "file:///path/to/src/widget.rs#L42");
        assert_eq!(span6, ((0, 8), (0, 59)));
    }

    #[test]
    fn test_high_throughput_terminal_burst() {
        let mut t = Terminal::new(80, 24);
        let line = b"Testing heavy output line with some ANSI codes: \x1b[32mOK\x1b[0m\r\n";
        let mut chunk = Vec::with_capacity(line.len() * 100);
        for _ in 0..100 {
            chunk.extend_from_slice(line);
        }
        for _ in 0..100 {
            t.advance(&chunk);
        }
        assert_eq!(t.grid().rows, 24);
        assert_eq!(t.grid().cols, 80);
        assert!(t.grid().scrollback.len() <= 10_000);
    }

    #[test]
    fn test_terminal_scrollback_and_text_extraction() {
        let mut t = Terminal::new(20, 3);
        // Print 5 lines: line 0, 1, 2, 3, 4
        t.advance(b"line 0\r\nline 1\r\nline 2\r\nline 3\r\nline 4");
        // Lines on live screen (display_offset 0): line 2, line 3, line 4
        assert_eq!(t.extract_text((0, 0), (0, 5)), "line 2");
        assert_eq!(t.extract_text((1, 0), (1, 5)), "line 3");
        assert_eq!(t.extract_text((2, 0), (2, 5)), "line 4");
        assert_eq!(t.display_offset(), 0);

        // Scroll up 1 line: live viewport shows line 1, line 2, line 3
        t.scroll_display(1);
        assert_eq!(t.display_offset(), 1);
        assert_eq!(t.extract_text((0, 0), (0, 5)), "line 1");
        assert_eq!(t.extract_text((1, 0), (1, 5)), "line 2");
        assert_eq!(t.extract_text((2, 0), (2, 5)), "line 3");

        // Scroll up another line (top of history): line 0, line 1, line 2
        t.scroll_display(1);
        assert_eq!(t.display_offset(), 2);
        assert_eq!(t.extract_text((0, 0), (0, 5)), "line 0");
        assert_eq!(t.extract_text((1, 0), (1, 5)), "line 1");

        // Trying to scroll past max scrollback clamps to max (2)
        t.scroll_display(10);
        assert_eq!(t.display_offset(), 2);

        // Scroll down 1 line
        t.scroll_display(-1);
        assert_eq!(t.display_offset(), 1);
        assert_eq!(t.extract_text((0, 0), (0, 5)), "line 1");

        // Scroll to bottom
        t.scroll_to_bottom();
        assert_eq!(t.display_offset(), 0);
        assert_eq!(t.extract_text((0, 0), (0, 5)), "line 2");

        // Scroll to top
        t.scroll_to_top();
        assert_eq!(t.display_offset(), 2);
        assert_eq!(t.extract_text((0, 0), (0, 5)), "line 0");
    }

    #[test]
    fn test_wide_characters_and_spacers() {
        let mut t = Terminal::new(10, 2);
        // "🚀" has width 2, "a" has width 1
        t.advance("🚀a".as_bytes());
        assert_eq!(t.grid().lines[0][0].c, '🚀');
        assert_eq!(t.grid().lines[0][1].flags & flags::WIDE_SPACER, flags::WIDE_SPACER);
        assert_eq!(t.grid().lines[0][2].c, 'a');
        assert_eq!(t.state.cursor.col, 3);
        // Text extraction should not contain the phantom spacer
        assert_eq!(t.extract_text((0, 0), (0, 3)), "🚀a");
    }

    #[test]
    fn test_insert_delete_line_preserves_cursor_col() {
        let mut t = Terminal::new(10, 4);
        t.advance(b"\x1b[2;5H"); // row 1, col 4
        assert_eq!(t.state.cursor.row, 1);
        assert_eq!(t.state.cursor.col, 4);

        // IL (Insert Line)
        t.advance(b"\x1b[L");
        assert_eq!(t.state.cursor.row, 1);
        assert_eq!(t.state.cursor.col, 4);

        // DL (Delete Line)
        t.advance(b"\x1b[M");
        assert_eq!(t.state.cursor.row, 1);
        assert_eq!(t.state.cursor.col, 4);
    }

    #[test]
    fn test_modify_other_keys_does_not_corrupt_sgr() {
        let mut t = Terminal::new(20, 2);
        // Vim sends CSI > 4 ; 2 m on entering insert mode or pressing delete
        t.advance(b"\x1b[>4;2mTest");
        // Ensure character 'T' does NOT have UNDERLINE or DIM flags set!
        assert_eq!(t.grid().lines[0][0].c, 'T');
        assert_eq!(t.grid().lines[0][0].flags & flags::UNDERLINE, 0);
        assert_eq!(t.grid().lines[0][0].flags & flags::DIM, 0);
    }

    #[test]
    fn test_secondary_device_attributes_query() {
        let mut t = Terminal::new(20, 2);
        t.advance(b"\x1b[>c");
        assert_eq!(t.take_responses(), b"\x1b[>0;10;1c");

        // Primary DA query
        t.advance(b"\x1b[c");
        assert_eq!(t.take_responses(), b"\x1b[?62;22c");
    }

    #[test]
    fn test_alt_screen_resets_display_offset() {
        let mut t = Terminal::new(20, 3);
        t.advance(b"line 1\r\nline 2\r\nline 3\r\nline 4\r\nline 5");
        t.scroll_display(2);
        assert_eq!(t.display_offset(), 2);

        // Enter alt screen (like nano/vim opening)
        t.advance(b"\x1b[?1049h");
        assert_eq!(t.display_offset(), 0);

        // Leave alt screen
        t.advance(b"\x1b[?1049l");
        assert_eq!(t.display_offset(), 0);
    }

    #[test]
    fn test_cbt_cht_rep() {
        let mut t = Terminal::new(40, 2);
        t.advance(b"\x1b[1;20H\x1b[Z");
        assert_eq!(t.state.cursor.col, 16);
        t.advance(b"\x1b[2Z");
        assert_eq!(t.state.cursor.col, 0);
        t.advance(b"\x1b[I");
        assert_eq!(t.state.cursor.col, 8);
        t.advance(b"\x1b[1;1Ha\x1b[3b");
        assert_eq!(t.grid().lines[0][3].c, 'a');
        assert_eq!(t.state.cursor.col, 4);
    }
}
