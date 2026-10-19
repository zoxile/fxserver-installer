use super::{render::Input, MAX_LINES};
use crate::{models::fxserver::FxserverTerminalEntry, terminal::safe_text};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::{collections::VecDeque, ops::Range};

struct Row {
    id: u64,
    line: Line<'static>,
    searchable: String,
}

#[derive(Clone, Copy)]
struct Anchor {
    first: u64,
    // Exclusive snapshot boundary: pausing an incomplete page also freezes it.
    end: u64,
}

#[derive(Default)]
pub(super) struct Panel {
    rows: VecDeque<Row>,
    matches: VecDeque<u64>,
    next_id: u64,
    anchor: Option<Anchor>,
    height: usize,
    incoming: u64,
    expired: bool,
    search: Input,
    words: Vec<String>,
    filtering: bool,
    #[cfg(test)]
    evaluations: usize,
}

impl Panel {
    fn matches_text(&mut self, text: &str) -> bool {
        #[cfg(test)]
        {
            self.evaluations += 1;
        }
        self.words.iter().all(|word| text.contains(word))
    }

    fn push(&mut self, line: Line<'static>) {
        let searchable = line.to_string().to_lowercase();
        let id = self.next_id;
        self.next_id += 1;
        if self.anchor.is_some() {
            self.incoming = self.incoming.saturating_add(1);
        }
        if self.rows.len() == MAX_LINES {
            if let Some(old) = self.rows.pop_front() {
                if self.matches.front() == Some(&old.id) {
                    self.matches.pop_front();
                }
            }
        }
        if self.matches_text(&searchable) {
            self.matches.push_back(id);
        }
        self.rows.push_back(Row {
            id,
            line,
            searchable,
        });
        if self
            .anchor
            .is_some_and(|anchor| anchor.first < anchor.end && anchor.first < self.rows[0].id)
            || self.expired && self.visible_range().is_empty() && !self.matches.is_empty()
        {
            self.expired = true;
            self.anchor = Some(Anchor {
                first: self.matches.front().copied().unwrap_or(self.next_id),
                end: self.next_id,
            });
        }
    }

    pub(super) fn append(&mut self, entry: FxserverTerminalEntry) {
        let line = if entry.segments.is_empty() {
            Line::raw(row_text(&entry.plain_line))
        } else {
            Line::from(
                entry
                    .segments
                    .into_iter()
                    .map(|segment| {
                        let color = segment
                            .color
                            .and_then(|color| color.parse::<Color>().ok())
                            .unwrap_or(Color::Reset);
                        let mut style = Style::new().fg(color);
                        if segment.emphasis == Some(true) {
                            style = style.bold();
                        }
                        Span::styled(row_text(&segment.text), style)
                    })
                    .collect::<Vec<_>>(),
            )
        };
        self.push(line);
    }

    pub(super) fn rcon(&mut self, text: &str, error: bool) {
        let label = if error { "RCON error" } else { "RCON" };
        let text = safe_text(text);
        if text.trim().is_empty() {
            self.push(Line::raw(format!("[{label}] No response text.")));
        } else {
            for line in text.lines() {
                self.push(Line::raw(format!("[{label}] {}", row_text(line))));
            }
        }
    }

    fn visible_range(&self) -> Range<usize> {
        let height = self.height.max(1);
        if let Some(anchor) = self.anchor {
            let end = self.matches.partition_point(|id| *id < anchor.end);
            let start = self
                .matches
                .partition_point(|id| *id < anchor.first)
                .min(end);
            start..start.saturating_add(height).min(end)
        } else {
            self.matches.len().saturating_sub(height)..self.matches.len()
        }
    }

    fn pause_at(&mut self, index: usize) {
        self.incoming = 0;
        self.anchor = Some(Anchor {
            first: self.matches.get(index).copied().unwrap_or(self.next_id),
            end: self.next_id,
        });
    }

    fn filter(&mut self) {
        self.words = self
            .search
            .value
            .to_lowercase()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        self.matches.clear();
        for row in &self.rows {
            #[cfg(test)]
            {
                self.evaluations += 1;
            }
            if self.words.iter().all(|word| row.searchable.contains(word)) {
                self.matches.push_back(row.id);
            }
        }
        if let Some(anchor) = self.anchor {
            let index = self
                .matches
                .partition_point(|id| *id < anchor.first)
                .min(self.matches.len().saturating_sub(1));
            self.pause_at(index);
        }
        self.expired = false;
    }

    pub(super) fn paste(&mut self, value: &str) {
        if self.filtering
            && self.search.value.len().saturating_add(value.len()) <= 256
            && self.search.paste(value).is_ok()
        {
            self.filter();
        }
    }

    pub(super) fn key(&mut self, key: KeyEvent) -> bool {
        if self.filtering {
            match key.code {
                KeyCode::Enter | KeyCode::Esc => self.filtering = false,
                _ => {
                    let previous = self.search.value.clone();
                    if !matches!(key.code, KeyCode::Char(c)
                        if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                            && self.search.value.len() + c.len_utf8() > 256)
                    {
                        self.search.key(key);
                    }
                    if self.search.value != previous {
                        self.filter();
                    }
                }
            }
            return true;
        }
        if key.code == KeyCode::Char('/')
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            || key.code == KeyCode::Char('f') && key.modifiers == KeyModifiers::CONTROL
        {
            self.filtering = true;
            return true;
        }
        let start = self.visible_range().start;
        let last = self.matches.len().saturating_sub(self.height.max(1));
        let index = match key.code {
            KeyCode::Up => start.saturating_sub(1),
            KeyCode::Down => start.saturating_add(1).min(last),
            KeyCode::PageUp => start.saturating_sub(self.height.max(1)),
            KeyCode::PageDown => start.saturating_add(self.height.max(1)).min(last),
            KeyCode::Home => 0,
            KeyCode::End => {
                self.anchor = None;
                self.expired = false;
                return true;
            }
            KeyCode::Char(' ')
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if self.anchor.is_some() {
                    self.anchor = None;
                } else {
                    self.pause_at(start);
                }
                self.expired = false;
                return true;
            }
            KeyCode::Esc if !self.search.value.is_empty() => {
                self.search = Input::default();
                self.filter();
                return true;
            }
            _ => return false,
        };
        self.pause_at(index);
        self.expired = false;
        true
    }

    pub(super) fn draw(&mut self, frame: &mut Frame, area: Rect) {
        if area.is_empty() {
            return;
        }
        let status = if self.anchor.is_some() {
            let incoming = if self.incoming > 999 {
                "999+".into()
            } else {
                self.incoming.to_string()
            };
            format!(
                "Paused | {}/{} | {} matches | {incoming} new",
                self.rows.len(),
                MAX_LINES,
                self.matches.len()
            )
        } else {
            format!(
                "Following | {}/{} rows | {} matches",
                self.rows.len(),
                MAX_LINES,
                self.matches.len()
            )
        };
        frame.render_widget(
            Paragraph::new(status).style(Style::new().fg(Color::Cyan)),
            Rect::new(area.x, area.y, area.width, 1),
        );
        if area.height < 2 {
            return;
        }
        let label = if self.filtering {
            "Filter (Enter/Esc: done): "
        } else {
            "Filter / Ctrl+F (Esc: clear): "
        };
        let label_width = (label.len() as u16).min(area.width);
        frame.render_widget(
            Paragraph::new(label).style(Style::new().fg(if self.filtering {
                Color::Cyan
            } else {
                Color::DarkGray
            })),
            Rect::new(area.x, area.y + 1, label_width, 1),
        );
        let width = area.width - label_width;
        let (text, cursor) = self.search.visible(width as usize, false);
        frame.render_widget(
            Paragraph::new(text),
            Rect::new(area.x + label_width, area.y + 1, width, 1),
        );
        if self.filtering && width > 0 {
            frame.set_cursor_position((area.x + label_width + cursor, area.y + 1));
        }
        self.height = area.height.saturating_sub(2) as usize;
        if self.height == 0 {
            return;
        }
        let body = Rect::new(area.x, area.y + 2, area.width, self.height as u16);
        let range = self.visible_range();
        if range.is_empty() {
            let empty = if self.rows.is_empty() {
                "No console output yet."
            } else if self.matches.is_empty() {
                "No matching console rows."
            } else {
                "Paused before incoming rows."
            };
            frame.render_widget(Paragraph::new(empty), body);
            return;
        }
        // Borrow only visible rows; no history-wide text clones or filter work per frame.
        let first = self.rows[0].id;
        let lines: Vec<_> = range
            .map(|index| {
                let row = &self.rows[(self.matches[index] - first) as usize];
                Line::from(
                    row.line
                        .spans
                        .iter()
                        .map(|span| Span::styled(span.content.as_ref(), span.style))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), body);
    }

    pub(super) fn footer(&self) -> &'static str {
        if self.expired {
            "Paused rows expired; oldest retained shown."
        } else {
            "PgUp/PgDn Up/Down Home: history"
        }
    }

    #[cfg(test)]
    pub(super) fn lines(
        &self,
    ) -> impl DoubleEndedIterator<Item = &Line<'static>> + ExactSizeIterator {
        self.rows.iter().map(|row| &row.line)
    }
}

fn row_text(text: &str) -> String {
    safe_text(text).replace(['\n', '\t'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::fxserver::FxserverTerminalSegment;
    use ratatui::{backend::TestBackend, style::Modifier, Terminal};

    fn key(panel: &mut Panel, code: KeyCode) {
        panel.key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn entry(text: &str) -> FxserverTerminalEntry {
        FxserverTerminalEntry {
            id: 1,
            stream: "stdout".into(),
            line: text.into(),
            plain_line: text.into(),
            segments: vec![],
            timestamp: String::new(),
        }
    }

    fn visible(panel: &Panel) -> Vec<String> {
        let first = panel.rows.front().map_or(0, |row| row.id);
        panel
            .visible_range()
            .map(|index| {
                panel.rows[(panel.matches[index] - first) as usize]
                    .line
                    .to_string()
            })
            .collect()
    }

    fn screen(panel: &mut Panel, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| panel.draw(frame, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn literal_multiword_filter_is_cached_per_row_not_per_frame() {
        let mut panel = Panel::default();
        for text in [
            "DATABASE [Warn] ready",
            "database warn ready",
            "[warn] other",
        ] {
            panel.append(entry(text));
        }
        key(&mut panel, KeyCode::Char('/'));
        panel.paste("[WARN]  database");
        assert_eq!(panel.matches.len(), 1);
        assert_eq!(visible(&panel), ["DATABASE [Warn] ready"]);
        let evaluations = panel.evaluations;
        screen(&mut panel, 50, 14);
        screen(&mut panel, 140, 40);
        key(&mut panel, KeyCode::Left);
        assert_eq!(panel.evaluations, evaluations);
        panel.append(entry("[warn] Database later"));
        assert_eq!(panel.evaluations, evaluations + 1);
        assert_eq!(panel.matches.len(), 2);
        key(&mut panel, KeyCode::Esc);
        assert!(!panel.filtering);
        assert_eq!(panel.matches.len(), 2);
        key(&mut panel, KeyCode::Esc);
        assert_eq!(panel.matches.len(), 4);
    }

    #[test]
    fn filter_input_is_unicode_safe_bounded_and_rejects_controls() {
        let mut panel = Panel::default();
        panel.append(entry("\u{c4} \u{754c}"));
        panel.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        panel.paste("\u{e4} \u{754c}");
        assert_eq!(panel.matches.len(), 1);
        let value = panel.search.value.clone();
        for unsafe_text in ["\n", "\r", "\t", "\x1b[31m", "\u{85}"] {
            panel.paste(unsafe_text);
            assert_eq!(panel.search.value, value);
        }
        panel.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        panel.paste(&"x".repeat(253));
        key(&mut panel, KeyCode::Char('\u{754c}'));
        assert_eq!(panel.search.value.len(), 256);
        key(&mut panel, KeyCode::Char('z'));
        panel.paste("z");
        assert_eq!(panel.search.value.len(), 256);
        key(&mut panel, KeyCode::Backspace);
        assert_eq!(panel.search.value.len(), 253);
        panel.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        panel.paste("\u{754c}\u{754c}e\u{301}\u{754c}");
        for width in 1..20 {
            let (text, cursor) = panel.search.visible(width, false);
            assert!(Line::raw(text).width() <= width);
            assert!((cursor as usize) < width);
        }
        key(&mut panel, KeyCode::Char('\x1b'));
        assert!(!panel.search.value.chars().any(char::is_control));
    }

    #[test]
    fn paused_snapshot_freezes_partial_pages_and_collects_new_rows() {
        let mut panel = Panel {
            height: 5,
            ..Panel::default()
        };
        panel.append(entry("first"));
        key(&mut panel, KeyCode::Char(' '));
        panel.append(entry("second"));
        panel.rcon("response", false);
        assert_eq!(visible(&panel), ["first"]);
        assert_eq!(panel.rows.len(), 3);
        assert_eq!(panel.incoming, 2);
        assert!(screen(&mut panel, 48, 7)[0].contains("2 new"));
        key(&mut panel, KeyCode::End);
        assert!(panel.anchor.is_none());
        assert_eq!(visible(&panel), ["first", "second", "[RCON] response"]);
        let mut empty = Panel::default();
        key(&mut empty, KeyCode::Char(' '));
        empty.append(entry("arrived while paused"));
        assert!(visible(&empty).is_empty());
        assert!(!empty.expired);
        key(&mut empty, KeyCode::Char(' '));
        assert_eq!(visible(&empty), ["arrived while paused"]);
    }

    #[test]
    fn navigation_uses_viewport_rows_and_end_alone_resumes_follow() {
        let mut panel = Panel {
            height: 3,
            ..Panel::default()
        };
        for index in 0..10 {
            panel.append(entry(&format!("row-{index}")));
        }
        assert_eq!(visible(&panel), ["row-7", "row-8", "row-9"]);
        key(&mut panel, KeyCode::Up);
        assert_eq!(visible(&panel)[0], "row-6");
        key(&mut panel, KeyCode::PageUp);
        assert_eq!(visible(&panel)[0], "row-3");
        key(&mut panel, KeyCode::Home);
        assert_eq!(visible(&panel)[0], "row-0");
        key(&mut panel, KeyCode::Down);
        assert_eq!(visible(&panel)[0], "row-1");
        key(&mut panel, KeyCode::PageDown);
        assert_eq!(visible(&panel)[0], "row-4");
        for _ in 0..20 {
            key(&mut panel, KeyCode::Down);
        }
        assert_eq!(visible(&panel)[0], "row-7");
        assert!(panel.anchor.is_some());
        panel.append(entry("row-10"));
        assert_eq!(visible(&panel)[0], "row-7");
        key(&mut panel, KeyCode::End);
        assert_eq!(visible(&panel)[0], "row-8");
        assert!(panel.anchor.is_none());
    }

    #[test]
    fn eviction_preserves_retained_anchor_then_recovers_with_bounded_storage() {
        let mut panel = Panel {
            height: 3,
            ..Panel::default()
        };
        for index in 0..MAX_LINES {
            panel.append(entry(&format!("row-{index}")));
        }
        let capacities = (panel.rows.capacity(), panel.matches.capacity());
        key(&mut panel, KeyCode::Home);
        for _ in 0..5 {
            key(&mut panel, KeyCode::Down);
        }
        for _ in 0..5 {
            panel.append(entry("new row"));
        }
        assert_eq!(visible(&panel)[0], "row-5");
        assert!(!panel.expired);
        panel.append(entry("anchor eviction"));
        assert_eq!(visible(&panel)[0], "row-6");
        assert_eq!(
            panel.footer(),
            "Paused rows expired; oldest retained shown."
        );
        assert_eq!(panel.incoming, 6);
        for _ in 0..MAX_LINES * 3 {
            panel.append(entry("retained"));
            assert_eq!(panel.rows.len(), MAX_LINES);
            assert_eq!(panel.matches.len(), MAX_LINES);
            assert_eq!(visible(&panel).len(), 3);
        }
        assert_eq!(
            (panel.rows.capacity(), panel.matches.capacity()),
            capacities
        );
        let rendered = screen(&mut panel, 48, 3);
        assert!(rendered[0].contains("999+ new"));
        assert!(rendered[2].contains("retained"));
        key(&mut panel, KeyCode::End);
        assert!(!panel.expired);
        assert!(panel.anchor.is_none());
    }

    #[test]
    fn sparse_filter_eviction_never_leaves_an_expired_blank_anchor() {
        let mut panel = Panel {
            height: 3,
            ..Panel::default()
        };
        panel.append(entry("keep first"));
        key(&mut panel, KeyCode::Char('/'));
        panel.paste("keep");
        key(&mut panel, KeyCode::Enter);
        key(&mut panel, KeyCode::Home);
        for _ in 0..MAX_LINES {
            panel.append(entry("irrelevant"));
        }
        assert!(panel.expired);
        assert!(panel.matches.is_empty());
        panel.append(entry("keep recovered"));
        assert_eq!(visible(&panel), ["keep recovered"]);
        key(&mut panel, KeyCode::Esc);
        assert_eq!(panel.matches.len(), MAX_LINES);
    }

    #[test]
    fn rendering_borrows_only_visible_rows_and_preserves_safe_ansi_styles() {
        let mut panel = Panel::default();
        for index in 0..100 {
            panel.append(entry(&format!("hidden-{index}")));
        }
        let mut styled = entry("not the displayed segments");
        styled.segments = vec![FxserverTerminalSegment {
            text: "\u{754c}\u{754c}abcdef\x1b\x07\r\n\t".into(),
            color: Some("red".into()),
            emphasis: Some(true),
        }];
        panel.append(styled);
        assert!(!panel
            .rows
            .back()
            .unwrap()
            .line
            .to_string()
            .chars()
            .any(char::is_control));
        let mut terminal = Terminal::new(TestBackend::new(8, 3)).unwrap();
        let evaluations = panel.evaluations;
        terminal
            .draw(|frame| panel.draw(frame, frame.area()))
            .unwrap();
        assert_eq!(panel.visible_range().len(), 1);
        assert_eq!(panel.evaluations, evaluations);
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 2)].symbol(), "\u{754c}");
        assert_eq!(buffer[(2, 2)].symbol(), "\u{754c}");
        assert_eq!(buffer[(4, 2)].symbol(), "a");
        assert_eq!(buffer[(7, 2)].symbol(), "d");
        assert_eq!(buffer[(0, 2)].fg, Color::Red);
        assert!(buffer[(0, 2)].modifier.contains(Modifier::BOLD));
    }
}
