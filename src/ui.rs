//! ratatui rendering: layout, pane borders, focus styling, and mapping the
//! vt100 screen (cells + colors) onto the ratatui buffer. Platform-independent.

use crate::app::{App, Focus, Mode};
use crate::pane::Pane;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

pub fn draw(f: &mut Frame, app: &App) {
    // A degenerate (e.g. not-yet-initialized 0x0) terminal must not panic.
    let area = f.area();
    if area.width < 2 || area.height < 2 {
        return;
    }
    let (la, ra) = app.rects();
    if let (Some(area), Some(pane)) = (la, &app.left) {
        draw_pane(
            f,
            area,
            pane,
            app.focus == Focus::Left,
            app.mode,
            app.zoomed,
        );
    }
    if let (Some(area), Some(pane)) = (ra, &app.right) {
        draw_pane(
            f,
            area,
            pane,
            app.focus == Focus::Right,
            app.mode,
            app.zoomed,
        );
    }
}

fn draw_pane(f: &mut Frame, area: Rect, pane: &Pane, focused: bool, mode: Mode, zoomed: bool) {
    let mut title = format!(" {} ", pane.name);
    if zoomed {
        title.push_str("(zoom) ");
    }
    if focused && mode == Mode::Scroll {
        title.push_str(&format!("(scroll: {}) ", pane.scroll));
    } else if pane.scroll > 0 {
        title.push_str(&format!("(scrolled: {}) ", pane.scroll));
    }
    let border_style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let show_cursor = focused && mode == Mode::Normal && pane.scroll == 0;
    render_screen(f, inner, pane, show_cursor);
}

fn render_screen(f: &mut Frame, inner: Rect, pane: &Pane, show_cursor: bool) {
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let Ok(mut term) = pane.term.lock() else {
        return;
    };
    term.parser_mut().set_scrollback(pane.scroll);
    let screen = term.parser().screen();

    let buf = f.buffer_mut();
    for row in 0..inner.height {
        // Write cell-by-cell at the exact vt100 column. vt100 columns already
        // match terminal columns: a wide (CJK/emoji) char sits in its own
        // column and the following column holds an empty continuation cell,
        // which we skip; ratatui's renderer handles the two-column glyph.
        // Empty cells are left as buffer defaults (blank), as before.
        for col in 0..inner.width {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            let text = cell.contents();
            if text.is_empty() {
                continue;
            }
            let target = &mut buf[(inner.x + col, inner.y + row)];
            target.set_symbol(text.as_str());
            target.set_style(cell_style(cell));
        }
    }

    if show_cursor {
        let (cr, cc) = screen.cursor_position();
        if cr < inner.height && cc < inner.width {
            f.set_cursor_position((inner.x + cc, inner.y + cr));
        }
    }
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut style = Style::default()
        .fg(vt_color(cell.fgcolor()))
        .bg(vt_color(cell.bgcolor()));
    if cell.bold() {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.italic() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.underline() {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if cell.inverse() {
        style = style.add_modifier(Modifier::REVERSED);
    }
    style
}

fn vt_color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::Pane;
    use crate::term::Term;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::sync::{Arc, Mutex};

    /// A wide (CJK) glyph must occupy exactly two columns: the glyph on the
    /// first, the continuation column untouched, and the next ASCII char two
    /// columns later — mirroring where the child program placed them.
    #[test]
    fn wide_chars_keep_column_positions() {
        let term = Arc::new(Mutex::new(Term::new(5, 20, 100)));
        term.lock().unwrap().feed("你ab好".as_bytes());
        let pane = Pane::dummy(term);

        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_screen(f, Rect::new(0, 0, 20, 5), &pane, false))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), "你");
        assert_eq!(buf[(2, 0)].symbol(), "a");
        assert_eq!(buf[(3, 0)].symbol(), "b");
        assert_eq!(buf[(4, 0)].symbol(), "好");
    }

    /// Plain ASCII still lands one char per column.
    #[test]
    fn ascii_layout_unchanged() {
        let term = Arc::new(Mutex::new(Term::new(5, 20, 100)));
        term.lock().unwrap().feed(b"hello");
        let pane = Pane::dummy(term);

        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_screen(f, Rect::new(0, 0, 20, 5), &pane, false))
            .unwrap();

        let buf = terminal.backend().buffer();
        for (i, ch) in "hello".chars().enumerate() {
            assert_eq!(buf[(i as u16, 0)].symbol(), ch.to_string());
        }
    }
}
