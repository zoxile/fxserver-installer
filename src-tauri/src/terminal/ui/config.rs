use super::render_list;
use crate::{
    models::fxserver::ServerConfigFile,
    terminal::{safe_text, settings::Settings},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, ListItem, Paragraph},
    Frame,
};
use ratatui_textarea::TextArea;
use unicode_width::UnicodeWidthChar;

const MAX_BYTES: usize = 512 * 1024;

#[derive(Default)]
pub(super) struct Panel {
    files: Vec<ServerConfigFile>,
    settings: Option<Settings>,
    selected: usize,
    editor: Option<Editor>,
    notice: String,
}

struct Editor {
    path: String,
    name: String,
    original: String,
    newline: &'static str,
    text: TextArea<'static>,
    bytes: usize,
    changed: bool,
}

pub(super) enum Event {
    None,
    Load,
    Save {
        settings: Box<Settings>,
        path: String,
        original: String,
        content: String,
    },
}

impl Panel {
    pub(super) fn loaded(&mut self, settings: Settings, files: Vec<ServerConfigFile>) -> bool {
        if self.editor.is_some() {
            return false;
        }
        self.files = files;
        self.settings = Some(settings);
        self.selected = 0;
        self.notice = String::new();
        true
    }

    pub(super) fn dirty(&self) -> bool {
        self.editor.as_ref().is_some_and(|editor| editor.changed)
    }

    pub(super) fn saved(&mut self, file: ServerConfigFile) {
        if let Some(editor) = &mut self.editor {
            if editor.path == file.path {
                editor.original.clone_from(&file.content);
                editor.changed = editor.content() != editor.original;
            }
        }
        if let Some(item) = self.files.iter_mut().find(|item| item.path == file.path) {
            *item = file;
        }
        self.notice = "Saved. Encrypted history retained.".into();
    }

    pub(super) fn paste(&mut self, text: &str) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        let pasted_newline = match newline_style(text) {
            Ok(newline) => newline,
            Err(error) => {
                self.notice = error.into();
                return;
            }
        };
        // Budget the serialized bytes after the textarea normalizes pasted newlines.
        let line_breaks = text.bytes().filter(|byte| *byte == b'\n').count();
        let inserted_bytes = text.len() - line_breaks * (pasted_newline.len() - 1)
            + line_breaks * (editor.newline.len() - 1);
        if text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            self.notice = "Paste contains unsupported control characters.".into();
        } else if editor.bytes.saturating_add(inserted_bytes) > MAX_BYTES {
            self.notice = "Config files are limited to 512 KiB.".into();
        } else {
            editor.text.insert_str(text);
            editor.update_changed();
            if editor.text.lines().len() >= u16::MAX as usize {
                editor.text.undo();
                editor.update_changed();
                self.notice = "The terminal editor supports fewer than 65535 lines.".into();
            }
        }
    }

    pub(super) fn key(&mut self, key: KeyEvent) -> Event {
        self.notice.clear();
        if let Some(editor) = &mut self.editor {
            let control = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Esc if editor.changed => {
                    self.notice = "Unsaved changes. Ctrl+S saves; Ctrl+W discards.".into()
                }
                KeyCode::Esc => self.editor = None,
                KeyCode::Char('w') if control => self.editor = None,
                KeyCode::Char('s') if control => {
                    if let Some(settings) = &self.settings {
                        return Event::Save {
                            settings: Box::new(settings.clone()),
                            path: editor.path.clone(),
                            original: editor.original.clone(),
                            content: editor.content(),
                        };
                    }
                }
                _ => {
                    let modified = match key.code {
                        KeyCode::Char('z') if control => editor.text.undo(),
                        KeyCode::Char('y') if control => editor.text.redo(),
                        KeyCode::Char(_) | KeyCode::Enter | KeyCode::Tab
                            if !control && editor.bytes >= MAX_BYTES =>
                        {
                            self.notice = "Config files are limited to 512 KiB.".into();
                            false
                        }
                        _ => editor.text.input(key),
                    };
                    if modified {
                        editor.update_changed();
                        if editor.bytes > MAX_BYTES
                            || editor.text.lines().len() >= u16::MAX as usize
                        {
                            editor.text.undo();
                            editor.update_changed();
                            self.notice = "Config files are limited to 512 KiB.".into();
                        }
                    }
                }
            }
            return Event::None;
        }
        match key.code {
            KeyCode::Char('r') => return Event::Load,
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.files.len().saturating_sub(1))
            }
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Enter => {
                if let Some(file) = self.files.get(self.selected) {
                    match Editor::new(file) {
                        Ok(editor) => self.editor = Some(editor),
                        Err(error) => self.notice = error,
                    }
                }
            }
            _ => {}
        }
        Event::None
    }

    pub(super) fn draw(&self, frame: &mut Frame, area: Rect) {
        let body = Rect {
            height: area.height.saturating_sub(1),
            ..area
        };
        if let Some(editor) = &self.editor {
            let title = format!(
                "{}{}",
                safe_text(&editor.name),
                if editor.changed { " *" } else { "" }
            );
            let block = Block::default().borders(Borders::ALL).title(title);
            let inner = block.inner(body);
            frame.render_widget(block, body);
            frame.render_widget(&editor.text, inner);
            editor.highlight(frame, inner);
        } else {
            let items = self
                .files
                .iter()
                .map(|file| {
                    ListItem::new(format!("{}  ({} bytes)", safe_text(&file.name), file.size))
                })
                .collect();
            render_list(
                frame,
                body,
                items,
                self.selected,
                "No config files loaded. Refresh [R]",
            );
        }
        if area.height > 0 {
            frame.render_widget(
                Paragraph::new(&*self.notice).style(Style::new().fg(Color::Yellow)),
                Rect::new(area.x, area.bottom() - 1, area.width, 1),
            );
        }
    }
}

impl Editor {
    fn new(file: &ServerConfigFile) -> Result<Self, String> {
        if file.content.len() > MAX_BYTES || file.content.split('\n').count() >= u16::MAX as usize {
            return Err("This file exceeds the terminal editor size limit.".into());
        }
        if file
            .content
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err("This file contains control characters. Edit it externally.".into());
        }
        let newline = newline_style(&file.content)?;
        let normalized = file.content.replace("\r\n", "\n");
        let mut text = TextArea::new(normalized.split('\n').map(str::to_owned).collect());
        text.set_line_number_style(Style::new().fg(Color::DarkGray));
        text.set_cursor_line_style(Style::default());
        text.set_cursor_style(Style::new().bg(Color::White).fg(Color::Black));
        text.set_tab_length(4);
        text.set_max_histories(100);
        Ok(Self {
            path: file.path.clone(),
            name: file.name.clone(),
            original: file.content.clone(),
            newline,
            text,
            bytes: file.content.len(),
            changed: false,
        })
    }

    fn content(&self) -> String {
        self.text.lines().join(self.newline)
    }

    fn update_changed(&mut self) {
        let content = self.content();
        self.bytes = content.len();
        self.changed = content != self.original;
    }

    fn highlight(&self, frame: &mut Frame, area: Rect) {
        let (top, left) = self.text.scroll_offset();
        let gutter = self.text.line_number_width();
        let text_width = area.width.saturating_sub(gutter) as usize;
        if text_width == 0 || area.height == 0 {
            return;
        }
        let cursor = self.text.cursor();
        for (index, line) in self
            .text
            .lines()
            .iter()
            .enumerate()
            .skip(top as usize)
            .take(area.height as usize)
        {
            let colors = cfg_colors(line);
            let mut column = 0;
            for (character, (value, color)) in line.chars().zip(colors).enumerate() {
                let cells = if value == '\t' {
                    4 - column % 4
                } else {
                    value.width().unwrap_or(0)
                };
                if (index, character) != (cursor.0, cursor.1) {
                    for offset in 0..cells {
                        let cell = column + offset;
                        if cell >= left as usize && cell - (left as usize) < text_width {
                            let x = area.x + gutter + (cell - left as usize) as u16;
                            frame.buffer_mut()[(x, area.y + (index - top as usize) as u16)]
                                .set_fg(color);
                        }
                    }
                }
                column += cells;
                if column >= left as usize + text_width {
                    break;
                }
            }
        }
    }
}

fn newline_style(text: &str) -> Result<&'static str, &'static str> {
    let bytes = text.as_bytes();
    let mut lf = false;
    let mut crlf = false;
    for (index, &byte) in bytes.iter().enumerate() {
        match byte {
            b'\r' if bytes.get(index + 1) != Some(&b'\n') => {
                return Err("Lone carriage returns are unsupported. Use LF or CRLF line endings.");
            }
            b'\n' if index > 0 && bytes[index - 1] == b'\r' => crlf = true,
            b'\n' => lf = true,
            _ => {}
        }
        if lf && crlf {
            return Err(
                "Mixed LF and CRLF line endings are unsupported. Normalize them externally.",
            );
        }
    }
    Ok(if crlf { "\r\n" } else { "\n" })
}

fn cfg_colors(line: &str) -> impl Iterator<Item = Color> + '_ {
    let mut quote = None;
    let mut comment = false;
    let mut command = true;
    let mut started = false;
    let mut escaped = false;
    line.chars().map(move |c| {
        if comment {
            return Color::DarkGray;
        }
        if let Some(delimiter) = quote {
            if c == delimiter && !escaped {
                quote = None;
            }
            escaped = c == '\\' && !escaped;
            return Color::Green;
        }
        if c == '#' {
            comment = true;
            return Color::DarkGray;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
            return Color::Green;
        }
        if c.is_whitespace() {
            if started {
                command = false;
            }
            return Color::White;
        }
        started = true;
        if command {
            Color::Cyan
        } else if c.is_ascii_digit() {
            Color::Yellow
        } else {
            Color::White
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(content: &str) -> ServerConfigFile {
        ServerConfigFile {
            name: "server.cfg".into(),
            path: "fixture".into(),
            content: content.into(),
            size: content.len() as u64,
            modified: None,
            has_rcon_password: false,
            has_rconlog: false,
        }
    }

    fn panel(content: &str) -> Panel {
        let mut panel = Panel::default();
        assert!(panel.loaded(Settings::default(), vec![file(content)]));
        panel.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(panel.editor.is_some());
        panel
    }

    #[test]
    fn late_load_results_preserve_both_clean_and_dirty_active_editors() {
        for dirty in [false, true] {
            let mut panel = panel("ensure chat\n");
            if dirty {
                panel.paste("# draft\n");
            }
            let content = panel.editor.as_ref().unwrap().content();
            panel.notice = "keep this notice".into();
            let settings = Settings {
                profile: "other-profile".into(),
                ..Settings::default()
            };
            assert!(!panel.loaded(settings, vec![file("replacement\n")]));
            assert_eq!(panel.editor.as_ref().unwrap().content(), content);
            assert_eq!(panel.editor.as_ref().unwrap().original, "ensure chat\n");
            assert_eq!(panel.dirty(), dirty);
            assert_eq!(panel.files[0].content, "ensure chat\n");
            assert_eq!(panel.settings.as_ref().unwrap().profile, "default");
            assert_eq!(panel.notice, "keep this notice");
        }
    }

    #[test]
    fn loads_are_accepted_after_explicitly_closing_the_editor() {
        let mut panel = panel("ensure chat\n");
        panel.paste("# draft\n");
        panel.key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert!(panel.editor.is_none());
        let settings = Settings {
            profile: "other-profile".into(),
            ..Settings::default()
        };
        assert!(panel.loaded(settings, vec![file("replacement\n")]));
        assert_eq!(panel.files[0].content, "replacement\n");
        assert_eq!(panel.settings.as_ref().unwrap().profile, "other-profile");
        assert_eq!(panel.selected, 0);
        assert!(panel.notice.is_empty());
    }

    #[test]
    fn editor_rejects_mixed_newlines_and_lone_carriage_returns() {
        for content in ["a\r\nb\n", "a\nb\r\n", "a\r", "a\rb", "a\r\r\n"] {
            assert!(Editor::new(&file(content)).is_err());
        }
        for content in ["", "a", "a\n\n", "a\r\nb\r\n", "\r\n"] {
            assert_eq!(Editor::new(&file(content)).unwrap().content(), content);
        }
        assert!(Editor::new(&file(&"\n".repeat(u16::MAX as usize - 1))).is_err());
    }

    #[test]
    fn rejected_pastes_leave_the_draft_and_undo_history_intact() {
        for pasted in ["\r", "a\rb", "a\r\nb\r", "a\r\nb\n", "a\nb\r\n"] {
            let mut panel = panel("ensure chat\n");
            panel.paste("# draft\n");
            let content = panel.editor.as_ref().unwrap().content();
            panel.paste(pasted);
            assert!(!panel.notice.is_empty());
            let editor = panel.editor.as_mut().unwrap();
            assert_eq!(editor.content(), content);
            assert!(editor.changed);
            assert!(editor.text.undo());
            editor.update_changed();
            assert_eq!(editor.content(), "ensure chat\n");
            assert!(!editor.changed);
        }
    }

    #[test]
    fn valid_pastes_use_the_files_newline_style() {
        for (original, pasted, expected) in [
            ("tail\n", "one\r\ntwo\r\n", "one\ntwo\ntail\n"),
            ("tail\r\n", "one\ntwo\n", "one\r\ntwo\r\ntail\r\n"),
        ] {
            let mut panel = panel(original);
            panel.paste(pasted);
            let editor = panel.editor.as_ref().unwrap();
            assert_eq!(editor.content(), expected);
            assert_eq!(editor.bytes, expected.len());
            assert!(editor.text.lines().iter().all(|line| !line.contains('\r')));
        }
    }

    #[test]
    fn paste_byte_limits_account_for_newline_expansion_and_contraction() {
        let original = format!("{}\r\n", "x".repeat(MAX_BYTES - 3));
        let mut crlf = panel(&original);
        crlf.paste("\n");
        assert_eq!(crlf.editor.as_ref().unwrap().content(), original);
        assert!(!crlf.dirty());
        assert!(crlf.notice.contains("512 KiB"));

        let mut lf = panel(&format!("{}\n", "x".repeat(MAX_BYTES - 2)));
        lf.paste("\r\n");
        assert_eq!(lf.editor.as_ref().unwrap().bytes, MAX_BYTES);
        assert!(lf.dirty());
        assert!(lf.notice.is_empty());
    }
    #[test]
    fn editor_preserves_newlines_and_undo_and_rejects_escape_sequences() {
        for content in ["ensure chat\n", "ensure chat\r\n", "ensure chat"] {
            let mut editor = Editor::new(&file(content)).unwrap();
            assert_eq!(editor.content(), content);
            editor.text.insert_str("test");
            editor.update_changed();
            assert!(editor.changed);
            editor.text.undo();
            editor.update_changed();
            assert!(!editor.changed);
        }
        assert!(Editor::new(&file("\x1b[31mtest")).is_err());
    }
    #[test]
    fn quoted_comments_do_not_turn_strings_into_comments() {
        let colors: Vec<_> = cfg_colors("set title \"#value\" # comment").collect();
        assert_eq!(colors[0], Color::Cyan);
        assert_eq!(colors[11], Color::Green);
        assert_eq!(colors.last(), Some(&Color::DarkGray));
    }

    #[test]
    fn color_iteration_can_stop_after_a_long_lines_visible_prefix() {
        let line = format!("set title \"{}\" # comment", "x".repeat(MAX_BYTES / 2));
        let colors = cfg_colors(&line).take(12).collect::<Vec<_>>();
        assert_eq!(colors.len(), 12);
        assert_eq!(&colors[..3], &[Color::Cyan; 3]);
        assert_eq!(&colors[10..], &[Color::Green; 2]);
    }

    #[test]
    fn highlighting_tolerates_no_text_space() {
        use ratatui::{backend::TestBackend, Terminal};
        let editor = Editor::new(&file("ensure chat\n")).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).unwrap();
        for area in [Rect::new(0, 0, 0, 0), Rect::new(0, 0, 1, 3)] {
            terminal
                .draw(|frame| editor.highlight(frame, area))
                .unwrap();
        }
    }
    #[test]
    fn scroll_alignment_and_cursor_survive_unicode_and_tabs() {
        use ratatui::{backend::TestBackend, Terminal};
        use ratatui_textarea::CursorMove;
        let mut editor = Editor::new(&file(&"ensure\t界resource\n".repeat(200))).unwrap();
        editor.text.move_cursor(CursorMove::Bottom);
        editor.text.move_cursor(CursorMove::End);
        let mut terminal = Terminal::new(TestBackend::new(50, 14)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                frame.render_widget(&editor.text, area);
                editor.highlight(frame, area);
            })
            .unwrap();
        assert!(editor.text.scroll_offset().0 > 180);
        editor.text.insert_str("ensure last");
        assert_eq!(editor.text.lines().last().unwrap(), "ensure last");
    }
}
