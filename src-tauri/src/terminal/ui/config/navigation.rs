use super::super::render::Input;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::Paragraph,
    Frame,
};
use ratatui_textarea::{CursorMove, TextArea};
use std::{fmt::Write, ops::Range};

const MAX_FIND_BYTES: usize = 256;
const MAX_LINE_BYTES: usize = 10;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Find,
    Line,
}

#[derive(Default)]
pub(super) struct Navigation {
    mode: Option<Mode>,
    find: Input,
    line: Input,
    fresh: bool,
    found: Option<(usize, Range<usize>)>,
}

impl Navigation {
    pub(super) fn is_open(&self) -> bool {
        self.mode.is_some()
    }

    pub(super) fn is_match(&self, row: usize, column: usize) -> bool {
        self.found
            .as_ref()
            .is_some_and(|(found_row, columns)| *found_row == row && columns.contains(&column))
    }

    pub(super) fn key(
        &mut self,
        text: &mut TextArea<'_>,
        key: KeyEvent,
        notice: &mut String,
    ) -> bool {
        let control = key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT);
        if control {
            match key.code {
                KeyCode::Char('f' | 'F') => {
                    self.mode = Some(Mode::Find);
                    self.fresh = true;
                    self.found = None;
                    return true;
                }
                KeyCode::Char('g' | 'G') => {
                    self.mode = Some(Mode::Line);
                    self.line = Input::default();
                    self.found = None;
                    return true;
                }
                KeyCode::Char('s' | 'z' | 'y' | 'w') => {
                    self.close();
                    return false;
                }
                _ => {}
            }
        }
        let Some(mode) = self.mode else {
            return false;
        };
        match key.code {
            KeyCode::Esc => self.close(),
            KeyCode::Enter | KeyCode::Up | KeyCode::Down if mode == Mode::Find => {
                let previous = key.code == KeyCode::Up
                    || (key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT));
                self.search(text, previous, notice);
            }
            KeyCode::Enter => {
                let line = self.line.value.trim().parse::<usize>().ok();
                if let Some(line) = line.filter(|line| (1..=text.lines().len()).contains(line)) {
                    text.cancel_selection();
                    text.move_cursor(CursorMove::Jump((line - 1) as u16, 0));
                    *notice = format!("Line {line}.");
                    self.close();
                } else {
                    *notice = format!("Line must be 1..{}.", text.lines().len());
                }
            }
            _ => {
                let limit = self.limit();
                let input = self.input_mut();
                if let KeyCode::Char(character) = key.code {
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && input.value.len() + character.len_utf8() > limit
                    {
                        *notice = format!("Input is limited to {limit} bytes.");
                        return true;
                    }
                }
                let before = input.value.clone();
                input.key(key);
                if input.value != before {
                    self.fresh = true;
                    self.found = None;
                }
            }
        }
        true
    }

    pub(super) fn paste(&mut self, value: &str) -> Result<(), &'static str> {
        let limit = self.limit();
        let input = self.input_mut();
        if input.value.len().saturating_add(value.len()) > limit {
            return Err("Paste exceeds the prompt input limit.");
        }
        if value.chars().any(char::is_control) {
            return Err("Use a single line without control characters.");
        }
        input.paste(value)?;
        if !value.is_empty() {
            self.fresh = true;
            self.found = None;
        }
        Ok(())
    }

    fn limit(&self) -> usize {
        if self.mode == Some(Mode::Line) {
            MAX_LINE_BYTES
        } else {
            MAX_FIND_BYTES
        }
    }

    fn input_mut(&mut self) -> &mut Input {
        if self.mode == Some(Mode::Line) {
            &mut self.line
        } else {
            &mut self.find
        }
    }

    fn close(&mut self) {
        self.mode = None;
        self.found = None;
    }

    fn search(&mut self, text: &mut TextArea<'_>, previous: bool, notice: &mut String) {
        self.found = None;
        if self.find.value.is_empty() {
            *notice = "Find text is empty.".into();
            return;
        }
        // Hex literals keep metacharacters literal without another regex dependency.
        let mut pattern = String::new();
        for character in self.find.value.chars() {
            let _ = write!(pattern, "\\x{{{:x}}}", u32::from(character));
        }
        if text.set_search_pattern(pattern).is_err() {
            *notice = "Unable to search for this text.".into();
            return;
        }
        let before = text.cursor();
        let before = (before.0, before.1);
        let found = if previous {
            text.search_back(false)
        } else {
            text.search_forward(self.fresh)
        };
        self.fresh = false;
        // Render only the active match; do not rescan whole long lines on every frame.
        let _ = text.set_search_pattern("");
        if found {
            text.cancel_selection();
            let cursor = text.cursor();
            let (row, column) = (cursor.0, cursor.1);
            self.found = Some((row, column..column + self.find.value.chars().count()));
            let wrapped = if previous {
                (row, column) > before
            } else {
                (row, column) < before
            };
            *notice = format!(
                "Match {}:{}{}",
                row + 1,
                column + 1,
                if wrapped { " (wrapped)" } else { "" }
            );
        } else {
            *notice = "No matches.".into();
        }
    }

    pub(super) fn draw(&self, frame: &mut Frame, area: Rect) {
        let Some(mode) = self.mode else {
            return;
        };
        if area.width == 0 || area.height == 0 {
            return;
        }
        let (input, label, short_label) = if mode == Mode::Find {
            (&self.find, "Find: ", "F:")
        } else {
            (&self.line, "Line: ", "L:")
        };
        let label = if area.width >= 10 {
            label
        } else if area.width >= 4 {
            short_label
        } else {
            ""
        };
        let label_width = label.len() as u16;
        frame.render_widget(
            Paragraph::new(label).style(Style::new().fg(Color::Cyan)),
            Rect::new(area.x, area.y, label_width, 1),
        );
        let field = Rect::new(area.x + label_width, area.y, area.width - label_width, 1);
        let (visible, cursor) = input.visible(field.width as usize, false);
        frame.render_widget(
            Paragraph::new(visible).style(Style::new().fg(Color::White)),
            field,
        );
        frame.set_cursor_position((field.x + cursor, field.y));
    }
}
