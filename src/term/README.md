# term

## Overview
Terminal emulation state machine and screen grid storage, feeding raw PTY byte streams through a `vte::Parser` to update primary and alternate cell grids, scrollback history, cursor state, Kitty Keyboard Protocol stacks, and hyperlink/word boundary detection.

## Interface
- `[mod.rs:grid]`: Public submodule (`pub mod grid`) exposing cell and grid structures.
- `[mod.rs:Cursor]`: Tracks `row`, `col`, current attribute `pen: Cell`, and private `pending_wrap` flag.
- `[mod.rs:TermState]`: Holds `primary` and `alt` `Grid` instances, `cursor`, `cursor_visible`, `app_cursor_keys`, `bracketed_paste`, `title`, `dirty`, `display_offset`, and `kitty_keyboard_stack`. Implements `vte::Perform` (`print`, `execute`, `csi_dispatch`, `esc_dispatch`, `osc_dispatch`) and exposes `[mod.rs:TermState::grid]`.
- `[mod.rs:Terminal]`: High-level terminal façade pairing `vte::Parser` with `pub state: TermState`. Methods: `[mod.rs:Terminal::new]`, `[mod.rs:Terminal::advance]`, `[mod.rs:Terminal::display_offset]`, `[mod.rs:Terminal::is_alt_active]`, `[mod.rs:Terminal::scroll_display]`, `[mod.rs:Terminal::scroll_to_bottom]`, `[mod.rs:Terminal::scroll_to_top]`, `[mod.rs:Terminal::resize]`, `[mod.rs:Terminal::grid]`, `[mod.rs:Terminal::take_responses]`, `[mod.rs:Terminal::kitty_keyboard_flags]`, `[mod.rs:Terminal::extract_text]`, `[mod.rs:Terminal::extract_text_buffer]`, `[mod.rs:Terminal::viewport_to_buffer_row]`, `[mod.rs:Terminal::buffer_to_viewport_row]`, `[mod.rs:Terminal::word_at]`, `[mod.rs:Terminal::url_at]`.
- `[grid.rs:Color]`: Cell color enum (`Default`, `Indexed(u8)`, `Rgb(u8, u8, u8)`).
- `[grid.rs:flags]`: SGR cell attribute bitmasks (`BOLD`, `DIM`, `ITALIC`, `UNDERLINE`, `INVERSE`, `HIDDEN`, `STRIKE`, `WIDE_SPACER`).
- `[grid.rs:Cell]`, `[grid.rs:Row]`: Individual terminal cell (`c`, `fg`, `bg`, `flags`) and `Vec<Cell>` row alias; `[grid.rs:Cell::blank]` creates a space cell preserving `pen.bg`.
- `[grid.rs:Grid]`: Screen buffer (`cols`, `rows`, `lines`, `scrollback: VecDeque<Row>`, `max_scrollback`). Methods: `[grid.rs:Grid::new]`, `[grid.rs:Grid::scroll_up]`, `[grid.rs:Grid::scroll_up_no_history]`, `[grid.rs:Grid::scroll_down]`, `[grid.rs:Grid::clear_range]`, `[grid.rs:Grid::clear_all]`, `[grid.rs:Grid::resize]`, `[grid.rs:Grid::visible_row]`, `[grid.rs:Grid::buffer_row]`, `[grid.rs:Grid::total_lines]`, `[grid.rs:Grid::extract_text_buffer]`, `[grid.rs:Grid::text]`, `[grid.rs:Grid::extract_text]`, `[grid.rs:Grid::extract_text_with_offset]`, `[grid.rs:Grid::word_at]`, `[grid.rs:Grid::word_at_with_offset]`, `[grid.rs:Grid::url_at]`, `[grid.rs:Grid::url_at_with_offset]`.
- `[grid.rs:find_links_in_chars]`: Scans a character slice for markdown links (`[Label](Target)`), explicit URIs (`http://`, `https://`, `file://`), absolute/relative paths (`/`, `~/`, `./`, `../`), and recognized source file tokens.

## Invariants
- `[mod.rs:SCROLLBACK]` & `[mod.rs:Terminal::new]`: Primary screen grid is initialized with `max_scrollback = 10_000` lines, whereas `alt` is initialized with `max_scrollback = 0` and never stores scrollback history.
- `[mod.rs:Terminal::scroll_display]` & `[mod.rs:TermState::set_alt_screen]`: Viewport scrolling (`display_offset`) is a no-op while `alt_active` is true, is clamped to `0..=primary.scrollback.len()`, and resets to `0` when toggling the alternate screen (`?47`, `?1047`, `?1049`).
- `[mod.rs:Terminal::resize]` & `[grid.rs:Grid::resize]`: Grid dimensions `cols` and `rows` are clamped to `.max(1)`, `cursor.col` is clamped to `cols - 1`, `pending_wrap` is cleared, scroll margins reset to `0..=rows - 1`, and shrinking rows pushes overflowed top lines into bounded `scrollback` while keeping `lines.len() == self.rows`.
- `[mod.rs:TermState::print_char]`: Zero-width characters (`unicode_width == 0`) are ignored after recording `last_char`; double-width characters (`width >= 2`) write the character into `col` and a `' '` cell with `flags::WIDE_SPACER` into `col + 1`.
- `[mod.rs:TermState::csi_dispatch]`: Character repeat (`CSI b` / `REP`) caps repetitions at `65,535` (`arg(0, 1).min(65535)`); `IL`/`DL` (`L`/`M`) apply only inside `[scroll_top, scroll_bottom]`; `DECSTBM` (`r`) updates margins only when `top < bottom`.
- `[grid.rs:Grid::extract_text_buffer]` & `[grid.rs:Grid::word_at_with_offset]` & `[grid.rs:Grid::url_at_with_offset]`: Text extraction filters out `flags::WIDE_SPACER`, maps `'\0'` to `' '`, and right-trims rows; `url_at_with_offset` joins at most 2 preceding and 2 following wrapped lines before single-line fallback.

## Side Effects
- `[mod.rs:Terminal::advance]`, `[mod.rs:Terminal::scroll_display]`, `[mod.rs:Terminal::resize]`: Mutates internal `vte::Parser`, `TermState`, `Grid` rows/scrollback, and sets `state.dirty = true` (pure in-memory state; performs no OS, PTY, or filesystem I/O).
- `[mod.rs:Terminal::take_responses]`: Drains queued protocol replies from `TermState::responses` via `std::mem::take`.
- `[mod.rs:TermState::csi_dispatch]` & `[mod.rs:TermState::esc_dispatch]` & `[mod.rs:TermState::osc_dispatch]`: Appends terminal replies (`\x1b[0n`, `\x1b[row;colR`, `\x1b[?flags u`, DA1 `\x1b[?62;22c`, DA2 `\x1b[>0;10;1c`) to `responses`, updates `kitty_keyboard_stack`, clears scrollback on `CSI 3 J`, resets `TermState` on `ESC c` (preserving `title`), and overwrites `title` on `OSC 0` / `OSC 2`.

## Verification
- Run: `cargo test`
