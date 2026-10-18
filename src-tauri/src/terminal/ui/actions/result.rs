use super::super::render::{ConfirmationPreview, Input};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::ops::Range;

const MAX_QUERY_BYTES: usize = 256;

pub(super) struct ResultView {
    preview: ConfirmationPreview,
    input: Input,
    query: String,
    active: Option<Range<usize>>,
    editing: bool,
    reveal: bool,
    #[cfg(test)]
    searches: usize,
}

impl ResultView {
    pub(super) fn new(text: String) -> Self {
        Self {
            preview: ConfirmationPreview::new(text),
            input: Input::default(),
            query: String::new(),
            active: None,
            editing: false,
            reveal: false,
            #[cfg(test)]
            searches: 0,
        }
    }

    pub(super) fn begin_search(&mut self) {
        self.editing = true;
    }

    pub(super) fn paste(&mut self, value: &str) {
        if self.editing && self.input.value.len().saturating_add(value.len()) <= MAX_QUERY_BYTES {
            let _ = self.input.paste(value);
        }
    }

    // Return false only when Escape should restore the action list or draft form.
    pub(super) fn key(&mut self, key: KeyEvent) -> bool {
        if key.code == KeyCode::Esc {
            if !self.editing {
                return false;
            }
            self.editing = false;
            self.input = Input::new(self.query.clone());
        } else if key.code == KeyCode::Enter
            || (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('n' | 'p')))
        {
            let restart = self.query != self.input.value;
            self.query.clone_from(&self.input.value);
            let previous =
                key.modifiers.contains(KeyModifiers::SHIFT) || key.code == KeyCode::Char('p');
            self.find(previous, restart);
            self.editing = false;
        } else if self.editing {
            if matches!(key.code, KeyCode::Char(character)
                if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && self.input.value.len() + character.len_utf8() > MAX_QUERY_BYTES)
            {
                return true;
            }
            self.input.key(key);
        } else {
            self.preview.key(key.code);
            self.reveal = false;
        }
        true
    }

    fn find(&mut self, previous: bool, restart: bool) {
        #[cfg(test)]
        {
            self.searches += 1;
        }
        if self.query.is_empty() {
            self.active = None;
            self.reveal = false;
            return;
        }
        let text = self.preview.displayed_text();
        let query = self.query.as_str();
        // Keep one match, not an unbounded occurrence index. Scans happen only on commands.
        let offset = match self.active.as_ref().filter(|_| !restart) {
            Some(active) if previous => text[..active.start]
                .match_indices(query)
                .last()
                .or_else(|| text.match_indices(query).last())
                .map(|(offset, _)| offset),
            Some(active) => text[active.end..]
                .find(query)
                .map(|offset| active.end + offset)
                .or_else(|| text.find(query)),
            None if previous => text.match_indices(query).last().map(|(offset, _)| offset),
            None => text.find(query),
        };
        self.active = offset.map(|offset| offset..offset + query.len());
        self.reveal = self.active.is_some();
    }

    pub(super) fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let search_height = if self.editing || !self.query.is_empty() {
            3
        } else {
            0
        };
        let [result, search] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(search_height)]).areas(area);
        self.preview
            .draw_result(frame, result, self.active.as_ref(), self.reveal);
        if result.width > 2 && result.height > 2 {
            self.reveal = false;
        }
        if search_height == 0 {
            return;
        }
        let title = if self.editing {
            "Find (exact) | Enter: find | Esc: cancel"
        } else if self.active.is_none() {
            "Find (exact): no matches | Ctrl+F: edit"
        } else {
            "Find (exact) | Enter/Shift+Enter: next/prev"
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::new().fg(if self.editing {
                Color::Cyan
            } else {
                Color::Gray
            }));
        let inner = block.inner(search);
        frame.render_widget(block, search);
        let (text, cursor) = self.input.visible(inner.width as usize, false);
        frame.render_widget(Paragraph::new(text), inner);
        if self.editing && inner.width > 0 && inner.height > 0 {
            frame.set_cursor_position((inner.x + cursor, inner.y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn enter(view: &mut ResultView, modifiers: KeyModifiers) {
        assert!(view.key(KeyEvent::new(KeyCode::Enter, modifiers)));
    }

    #[test]
    fn search_query_byte_limit_rejects_paste_and_typing_without_changing_input() {
        let mut view = ResultView::new("fixture".into());
        view.begin_search();
        view.paste(&"x".repeat(MAX_QUERY_BYTES + 1));
        assert!(view.input.value.is_empty());
        view.paste(&"\u{754c}".repeat(85));
        let original = view.input.value.clone();
        view.paste("ab");
        view.key(KeyEvent::new(KeyCode::Char('\u{754c}'), KeyModifiers::NONE));
        assert_eq!(view.input.value, original);
        view.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(view.input.value.len(), MAX_QUERY_BYTES);
        let original = view.input.value.clone();
        view.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        assert_eq!(view.input.value, original);
        view.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(view.input.value.is_empty());
    }

    #[test]
    fn literal_unicode_search_wraps_in_both_directions() {
        let mut view = ResultView::new("[\u{754c}] a [\u{754c}]".into());
        view.begin_search();
        view.paste("[\u{754c}]");
        assert_eq!(view.searches, 0);
        enter(&mut view, KeyModifiers::NONE);
        assert_eq!(view.active, Some(0..5));
        enter(&mut view, KeyModifiers::NONE);
        assert_eq!(view.active, Some(8..13));
        enter(&mut view, KeyModifiers::NONE);
        assert_eq!(view.active, Some(0..5));
        enter(&mut view, KeyModifiers::SHIFT);
        assert_eq!(view.active, Some(8..13));
        view.key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert_eq!(view.active, Some(0..5));
        view.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        assert_eq!(view.active, Some(8..13));
    }

    #[test]
    fn escape_cancels_uncommitted_query_then_exits_result() {
        let mut view = ResultView::new("aaaaa".into());
        view.begin_search();
        view.paste("aa");
        enter(&mut view, KeyModifiers::NONE);
        assert_eq!(view.active, Some(0..2));
        enter(&mut view, KeyModifiers::SHIFT);
        assert_eq!(view.active, Some(2..4));
        enter(&mut view, KeyModifiers::NONE);
        assert_eq!(view.active, Some(0..2));
        view.begin_search();
        view.paste("missing");
        assert!(view.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert_eq!(view.input.value, "aa");
        assert_eq!(view.active, Some(0..2));
        assert!(!view.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    }

    #[test]
    fn queries_match_displayed_escapes_and_empty_or_missing_queries_are_safe() {
        let mut view = ResultView::new("literal\\t\t\x1b\u{301}".into());
        view.begin_search();
        view.paste("\n");
        assert!(view.input.value.is_empty());
        view.paste("\\u{1b}");
        enter(&mut view, KeyModifiers::NONE);
        assert_eq!(
            &view.preview.displayed_text()[view.active.clone().unwrap()],
            "\\u{1b}"
        );
        for query in ["MISSING", ""] {
            view.begin_search();
            view.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
            view.paste(query);
            enter(&mut view, KeyModifiers::NONE);
            assert!(view.active.is_none());
            enter(&mut view, KeyModifiers::SHIFT);
            assert!(view.active.is_none());
        }
    }

    #[test]
    fn repeated_draws_do_not_search_or_allocate_an_occurrence_index() {
        let mut view = ResultView::new(format!("{}END", "x\n".repeat(100_000)));
        view.begin_search();
        view.paste("END");
        enter(&mut view, KeyModifiers::NONE);
        assert_eq!(view.active, Some(200_000..200_003));
        for (width, height) in [(1, 1), (46, 8), (80, 24), (130, 36)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for _ in 0..10 {
                terminal
                    .draw(|frame| view.draw(frame, frame.area()))
                    .unwrap();
            }
        }
        assert_eq!(view.searches, 1);
        view.begin_search();
        view.paste("new query");
        assert_eq!(view.searches, 1);
    }
}
