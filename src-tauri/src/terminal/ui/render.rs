use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use std::ops::Range;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Default)]
pub(super) struct Input {
    pub(super) value: String,
    // UTF-8 byte offset, always on a character boundary.
    pub(super) cursor: usize,
}

impl Input {
    pub(super) fn new(value: String) -> Self {
        let cursor = value.len();
        Self { value, cursor }
    }

    fn previous(&self) -> usize {
        self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(offset, _)| offset)
    }

    fn next(&self) -> usize {
        self.cursor
            + self.value[self.cursor..]
                .chars()
                .next()
                .map_or(0, char::len_utf8)
    }

    pub(super) fn paste(&mut self, value: &str) -> Result<(), &'static str> {
        if value.chars().any(char::is_control) {
            return Err("Paste one line at a time. Use SQL File for multiline SQL.");
        }
        if self.value.len().saturating_add(value.len()) > 8192 {
            return Err("Input is limited to 8 KiB.");
        }
        self.value.insert_str(self.cursor, value);
        self.cursor += value.len();
        Ok(())
    }

    pub(super) fn key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.value.clear();
                self.cursor = 0;
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && !c.is_control()
                    && self.value.len() + c.len_utf8() <= 8192 =>
            {
                self.value.insert(self.cursor, c);
                self.cursor += c.len_utf8();
            }
            KeyCode::Backspace if self.cursor > 0 => {
                let previous = self.previous();
                self.value.drain(previous..self.cursor);
                self.cursor = previous;
            }
            KeyCode::Delete if self.cursor < self.value.len() => {
                self.value.remove(self.cursor);
            }
            KeyCode::Left => self.cursor = self.previous(),
            KeyCode::Right => self.cursor = self.next(),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.value.len(),
            _ => {}
        }
    }

    pub(super) fn visible(&self, width: usize, secret: bool) -> (String, u16) {
        if width == 0 {
            return (String::new(), 0);
        }
        let display = |c| if secret { '*' } else { c };
        let mut start = self.cursor;
        let mut cells = 0;
        for (offset, c) in self.value[..self.cursor].char_indices().rev() {
            let next = display(c).width().unwrap_or(0);
            if cells + next >= width {
                break;
            }
            start = offset;
            cells += next;
        }
        let mut output = String::new();
        let mut cells = 0;
        let mut cursor_bytes = 0;
        for (offset, c) in self.value[start..].char_indices() {
            let c = display(c);
            let next = c.width().unwrap_or(0);
            if cells + next > width {
                break;
            }
            output.push(c);
            cells += next;
            if start + offset < self.cursor {
                cursor_bytes = output.len();
            }
        }
        let cursor = output[..cursor_bytes].width().min(width - 1);
        (output, cursor.min(u16::MAX as usize) as u16)
    }
}

pub(super) struct ConfirmationPreview {
    text: String,
    escaped: bool,
    rows: Vec<usize>,
    width: usize,
    height: usize,
    scroll: usize,
    #[cfg(test)]
    layouts: usize,
}

impl ConfirmationPreview {
    pub(super) fn new(text: String) -> Self {
        // Make invisible/control characters explicit and bound bytes per display cell.
        // This is display-only; the queued job retains the exact original SQL.
        let needs_escape = |c: char| c != '\n' && (c.is_control() || c.width().unwrap_or(0) == 0);
        let escaped = text.chars().any(needs_escape);
        let text = if escaped {
            let mut preview = String::with_capacity(text.len());
            for c in text.chars() {
                if c == '\\' || needs_escape(c) {
                    preview.extend(c.escape_default());
                } else {
                    preview.push(c);
                }
            }
            preview
        } else {
            text
        };
        Self {
            text,
            escaped,
            rows: vec![0],
            width: 0,
            height: 1,
            scroll: 0,
            #[cfg(test)]
            layouts: 0,
        }
    }

    fn resize(&mut self, width: usize, height: usize) {
        let width = width.max(2);
        let anchor = self.rows[self.scroll];
        if self.width != width {
            #[cfg(test)]
            {
                self.layouts += 1;
            }
            self.rows.clear();
            self.rows.push(0);
            let mut cells = 0;
            for (offset, c) in self.text.char_indices() {
                if c == '\n' {
                    self.rows.push(offset + 1);
                    cells = 0;
                    continue;
                }
                let next = c.width().unwrap_or(1);
                if cells + next > width {
                    self.rows.push(offset);
                    cells = 0;
                }
                cells += next;
            }
            self.width = width;
            self.scroll = self
                .rows
                .partition_point(|offset| *offset <= anchor)
                .saturating_sub(1);
        }
        self.height = height.max(1);
        self.scroll = self.scroll.min(self.max_scroll());
    }

    fn max_scroll(&self) -> usize {
        self.rows.len().saturating_sub(self.height)
    }

    pub(super) fn key(&mut self, key: KeyCode) {
        self.scroll = match key {
            KeyCode::PageDown => self.scroll.saturating_add(self.height),
            KeyCode::PageUp => self.scroll.saturating_sub(self.height),
            KeyCode::Down => self.scroll.saturating_add(1),
            KeyCode::Up => self.scroll.saturating_sub(1),
            KeyCode::Home => 0,
            KeyCode::End => self.max_scroll(),
            _ => self.scroll,
        }
        .min(self.max_scroll());
    }

    fn visible_rows(&self) -> impl Iterator<Item = &str> {
        let end = self.scroll.saturating_add(self.height).min(self.rows.len());
        (self.scroll..end).map(|index| {
            let end = self.rows.get(index + 1).copied().unwrap_or(self.text.len());
            self.text[self.rows[index]..end]
                .strip_suffix('\n')
                .unwrap_or(&self.text[self.rows[index]..end])
        })
    }

    pub(super) fn displayed_text(&self) -> &str {
        &self.text
    }

    fn highlighted_rows(&self, active: Range<usize>) -> impl Iterator<Item = Line<'_>> {
        self.visible_rows().enumerate().map(move |(index, text)| {
            let offset = self.rows[self.scroll + index];
            let start = active.start.max(offset);
            let end = active.end.min(offset + text.len());
            if start >= end {
                return Line::raw(text);
            }
            let start = start - offset;
            let end = end - offset;
            Line::from(vec![
                Span::raw(&text[..start]),
                Span::styled(
                    &text[start..end],
                    Style::new().fg(Color::Black).bg(Color::Yellow),
                ),
                Span::raw(&text[end..]),
            ])
        })
    }

    pub(super) fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let title = if self.escaped {
            "Confirm Y | N/Esc | Escaped preview"
        } else {
            "Confirm Y | Cancel N/Esc"
        };
        self.draw_panel(frame, area, title);
    }

    pub(super) fn draw_panel(&mut self, frame: &mut Frame, area: Rect, title: &str) {
        self.draw_panel_with_match(frame, area, title, None, false);
    }

    pub(super) fn draw_result(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        active: Option<&Range<usize>>,
        reveal: bool,
    ) {
        let title = if self.escaped {
            "Result (escaped) | Ctrl+F: find | Esc: back"
        } else {
            "Result | Ctrl+F: find | Esc: back"
        };
        self.draw_panel_with_match(frame, area, title, active, reveal);
    }

    fn draw_panel_with_match(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        title: &str,
        active: Option<&Range<usize>>,
        reveal: bool,
    ) {
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        self.resize(inner.width as usize, inner.height as usize);
        let active = active.filter(|range| {
            range.start < range.end
                && range.end <= self.text.len()
                && self.text.is_char_boundary(range.start)
                && self.text.is_char_boundary(range.end)
        });
        if let Some(active) = active.filter(|_| reveal) {
            self.scroll = self
                .rows
                .partition_point(|offset| *offset <= active.start)
                .saturating_sub(1)
                .min(self.max_scroll());
        }
        let end = self.scroll.saturating_add(self.height).min(self.rows.len());
        let range = format!("{}-{}/{}", self.scroll + 1, end, self.rows.len());
        let position = if range.len() + " | PgUp/PgDn Home/End".len() <= inner.width as usize {
            format!("{range} | PgUp/PgDn Home/End")
        } else {
            format!("{range} | PgUp/PgDn")
        };
        frame.render_widget(Clear, area);
        frame.render_widget(
            block
                .title(title)
                .title_bottom(position)
                .border_style(Style::new().fg(Color::Yellow)),
            area,
        );
        // Already wrapped; never ask Paragraph to walk text above the viewport.
        let lines: Vec<_> = match active {
            Some(active) => self.highlighted_rows(active.clone()).collect(),
            None => self.visible_rows().map(Line::raw).collect(),
        };
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn result_highlighting_spans_wrapped_unicode_without_changing_confirmation() {
        let mut preview = ConfirmationPreview::new("aaaaa\u{754c}XYZ tail".into());
        let active = 4..9;
        let mut terminal = Terminal::new(TestBackend::new(8, 6)).unwrap();
        terminal
            .draw(|frame| preview.draw_result(frame, frame.area(), Some(&active), true))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(5, 1)].bg, Color::Yellow);
        assert_eq!(buffer[(1, 2)].bg, Color::Yellow);
        assert_eq!(buffer[(3, 2)].bg, Color::Yellow);
        let lines: Vec<_> = preview.highlighted_rows(active).collect();
        assert_eq!(
            lines.iter().map(Line::to_string).collect::<String>(),
            "aaaaa\u{754c}XYZ tail"
        );
        assert!(lines.len() <= 4 && lines.iter().all(|line| line.width() <= 6));
        terminal
            .draw(|frame| preview.draw(frame, frame.area()))
            .unwrap();
        assert!(terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .all(|cell| cell.bg != Color::Yellow));
    }

    #[test]
    fn result_match_reveal_and_repeated_highlighting_reuse_cached_layout() {
        let text = format!("{}MATCH", "row\n".repeat(100_000));
        let active = text.len() - 5..text.len();
        let mut preview = ConfirmationPreview::new(text);
        let mut terminal = Terminal::new(TestBackend::new(50, 14)).unwrap();
        terminal
            .draw(|frame| preview.draw_result(frame, frame.area(), Some(&active), true))
            .unwrap();
        for _ in 0..50 {
            terminal
                .draw(|frame| preview.draw_result(frame, frame.area(), Some(&active), false))
                .unwrap();
        }
        assert_eq!(preview.layouts, 1);
        assert!(preview.scroll > u16::MAX as usize);
        assert!(preview
            .visible_rows()
            .collect::<String>()
            .ends_with("MATCH"));
        let lines: Vec<_> = preview.highlighted_rows(active).collect();
        assert!(lines.len() <= 12);
        assert!(
            lines
                .iter()
                .map(|line| line.to_string().len())
                .sum::<usize>()
                <= 48 * 12
        );
    }

    #[test]
    fn invalid_match_boundaries_are_ignored_without_panics() {
        let mut preview = ConfirmationPreview::new("\u{754c}x".into());
        let mut terminal = Terminal::new(TestBackend::new(50, 14)).unwrap();
        for active in [1..2, 0..100, Range { start: 3, end: 2 }, 0..0] {
            terminal
                .draw(|frame| preview.draw_result(frame, frame.area(), Some(&active), true))
                .unwrap();
            assert!(terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .all(|cell| cell.bg != Color::Yellow));
        }
    }

    #[test]
    fn large_preview_is_complete_and_only_submits_viewport_rows() {
        let text = format!("{}SELECT 'final statement';", "x".repeat(10 * 1024 * 1024));
        let mut preview = ConfirmationPreview::new(text.clone());
        preview.resize(42, 6);
        assert_eq!(preview.text, text);
        assert!(preview.rows.len() > u16::MAX as usize);
        for _ in 0..100 {
            preview.key(KeyCode::PageDown);
            preview.resize(42, 6);
            assert_eq!(
                preview.layouts, 1,
                "Unchanged width must not rescan the document"
            );
            assert!(preview.visible_rows().count() <= 6);
            assert!(preview.visible_rows().map(str::len).sum::<usize>() <= 42 * 6);
        }
        preview.key(KeyCode::End);
        assert!(preview.scroll > u16::MAX as usize);
        assert!(preview
            .visible_rows()
            .collect::<String>()
            .ends_with("SELECT 'final statement';"));
        preview.key(KeyCode::PageDown);
        assert_eq!(preview.scroll, preview.max_scroll());
        preview.key(KeyCode::Home);
        preview.key(KeyCode::PageUp);
        assert_eq!(preview.scroll, 0);
    }

    #[test]
    fn wrapping_preserves_whitespace_empty_rows_and_unicode_without_clipping() {
        let mut preview = ConfirmationPreview::new("  SELECT '界';\n\n  SELECT 2;\n".into());
        preview.resize(4, 100);
        assert!(!preview.escaped);
        assert!(preview.visible_rows().all(|row| row.width() <= 4));
        assert_eq!(
            preview.visible_rows().collect::<String>(),
            "  SELECT '界';  SELECT 2;"
        );
        assert_eq!(preview.visible_rows().last(), Some(""));
        for (index, start) in preview.rows.iter().enumerate() {
            let end = preview
                .rows
                .get(index + 1)
                .copied()
                .unwrap_or(preview.text.len());
            assert!(preview.text.is_char_boundary(*start) && preview.text.is_char_boundary(end));
        }
    }

    #[test]
    fn control_and_unbounded_zero_width_sequences_are_explicit_and_bounded() {
        let mut preview = ConfirmationPreview::new(format!(
            "SELECT '\\t\t\r\x1b{}';",
            "\u{301}".repeat(100_000)
        ));
        assert!(preview.escaped);
        assert!(preview.text.contains("\\\\t\\t\\r\\u{1b}"));
        assert!(!preview.text.contains('\x1b'));
        preview.resize(42, 6);
        assert!(preview.visible_rows().map(str::len).sum::<usize>() <= 42 * 6);
        preview.key(KeyCode::End);
        assert!(preview.visible_rows().collect::<String>().ends_with("';"));
    }

    #[test]
    fn resizing_preserves_the_visible_anchor_then_clamps_to_a_valid_page() {
        let mut preview = ConfirmationPreview::new("SELECT 1;\n".repeat(100));
        preview.resize(42, 6);
        preview.key(KeyCode::PageDown);
        let anchor = preview.rows[preview.scroll];
        preview.resize(5, 10);
        assert!(preview.rows[preview.scroll] <= anchor);
        assert!(preview.rows[preview.scroll + 1] > anchor);
        preview.key(KeyCode::End);
        preview.resize(80, 500);
        assert_eq!(preview.scroll, 0);
        let mut empty = ConfirmationPreview::new(String::new());
        empty.resize(0, 0);
        empty.key(KeyCode::End);
        assert_eq!(empty.visible_rows().collect::<Vec<_>>(), [""]);
    }

    #[test]
    fn confirmation_position_and_final_statement_render_at_minimum_size() {
        let mut terminal = Terminal::new(TestBackend::new(50, 14)).unwrap();
        let mut preview =
            ConfirmationPreview::new(format!("{}\nFINAL SQL;", "line\n".repeat(100_000)));
        let area = Rect::new(3, 3, 44, 8);
        terminal.draw(|f| preview.draw(f, area)).unwrap();
        preview.key(KeyCode::End);
        terminal.draw(|f| preview.draw(f, area)).unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(content.contains("FINAL SQL;"));
        assert!(content.contains("100002/100002"));
        assert!(content.contains("Home/End"));
    }

    #[test]
    fn input_navigation_is_utf8_safe_and_does_not_reallocate_on_arrow_keys() {
        let mut input = Input::new("a界\u{10348}b".into());
        let allocation = input.value.as_ptr();
        for _ in 0..5 {
            input.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        }
        assert_eq!(input.cursor, 0);
        for _ in 0..3 {
            input.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        }
        assert_eq!(input.cursor, "a界\u{10348}".len());
        assert_eq!(input.value.as_ptr(), allocation);
        input.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        input.key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(input.value, "a界");
        assert_eq!(input.cursor, input.value.len());
    }

    #[test]
    fn full_input_has_bounded_visible_text_and_preserves_the_byte_limit() {
        let mut input = Input::new(format!("{}a", "界".repeat(2730)));
        input.key(KeyEvent::new(KeyCode::Char('界'), KeyModifiers::NONE));
        assert_eq!(input.value.len(), 8191);
        for width in [0, 1, 2, 42, 80] {
            for secret in [false, true] {
                let (visible, cursor) = input.visible(width, secret);
                assert!(visible.width() <= width);
                assert!(width == 0 || (cursor as usize) < width);
                assert!(visible.len() <= width * 3);
                if secret {
                    assert!(visible.chars().all(|c| c == '*'));
                }
            }
        }
        input.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(input.value.is_empty());
        assert_eq!(input.cursor, 0);
    }
}
