use super::lists;
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

mod navigation;
use navigation::Navigation;

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
    navigation: Navigation,
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
        if editor.navigation.is_open() {
            self.notice = editor
                .navigation
                .paste(text)
                .err()
                .unwrap_or_default()
                .into();
            return;
        }
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
            if editor
                .navigation
                .key(&mut editor.text, key, &mut self.notice)
            {
                return Event::None;
            }
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
        let prompt_open = self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.navigation.is_open());
        let footer_height = if prompt_open { 2 } else { 1 };
        let body = Rect {
            height: area.height.saturating_sub(footer_height),
            ..area
        };
        if let Some(editor) = &self.editor {
            let title = format!(
                "{}{}",
                safe_text(&editor.name),
                if editor.changed { " *" } else { "" }
            );
            let block = Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_bottom("Ctrl+F: find  Ctrl+G: line");
            let inner = block.inner(body);
            frame.render_widget(block, body);
            frame.render_widget(&editor.text, inner);
            editor.highlight(frame, inner);
        } else {
            lists::draw(
                frame,
                body,
                self.files.len(),
                self.selected,
                |index| {
                    let file = &self.files[index];
                    ListItem::new(format!("{}  ({} bytes)", safe_text(&file.name), file.size))
                },
                "No config files loaded. Refresh [R]",
            );
        }
        if area.height > u16::from(prompt_open) {
            frame.render_widget(
                Paragraph::new(&*self.notice).style(Style::new().fg(Color::Yellow)),
                Rect::new(area.x, area.bottom() - 1, area.width, 1),
            );
        }
        if prompt_open && area.height > 0 {
            if let Some(editor) = &self.editor {
                editor
                    .navigation
                    .draw(frame, Rect::new(area.x, body.bottom(), area.width, 1));
            }
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
            navigation: Navigation::default(),
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
                            let cell = &mut frame.buffer_mut()
                                [(x, area.y + (index - top as usize) as u16)];
                            if self.navigation.is_match(index, character) {
                                cell.set_fg(Color::Black).set_bg(Color::Yellow);
                            } else {
                                cell.set_fg(color);
                            }
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

    fn control(panel: &mut Panel, character: char) -> Event {
        panel.key(KeyEvent::new(
            KeyCode::Char(character),
            KeyModifiers::CONTROL,
        ))
    }

    fn press(panel: &mut Panel, code: KeyCode) {
        panel.key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn find(panel: &mut Panel, query: &str) {
        control(panel, 'f');
        control(panel, 'u');
        panel.paste(query);
        press(panel, KeyCode::Enter);
    }

    #[test]
    fn find_uses_unicode_character_columns_and_literal_text() {
        let query = "界e\u{301}";
        let original = format!("α {query}\r\n{query}");
        let mut panel = panel(&original);
        find(&mut panel, query);
        let editor = panel.editor.as_ref().unwrap();
        assert_eq!(editor.text.cursor(), (0, 2));
        assert!(editor.navigation.is_match(0, 2));
        assert!(editor.navigation.is_match(0, 4));
        assert!(!editor.navigation.is_match(0, 5));
        assert!(editor.text.search_pattern().is_none());
        press(&mut panel, KeyCode::Enter);
        assert_eq!(panel.editor.as_ref().unwrap().text.cursor(), (1, 0));
        assert_eq!(panel.editor.as_ref().unwrap().content(), original);
        assert!(!panel.dirty());

        let literal = r".*[x](a)?+^$\end";
        let mut literal_panel = self::panel(&format!("prefix {literal}"));
        find(&mut literal_panel, literal);
        assert_eq!(literal_panel.editor.as_ref().unwrap().text.cursor(), (0, 7));
    }

    #[test]
    fn find_next_and_previous_wrap_in_both_directions() {
        let mut panel = panel("hit\nskip\nhit");
        find(&mut panel, "hit");
        assert_eq!(panel.editor.as_ref().unwrap().text.cursor(), (0, 0));
        press(&mut panel, KeyCode::Enter);
        assert_eq!(panel.editor.as_ref().unwrap().text.cursor(), (2, 0));
        press(&mut panel, KeyCode::Enter);
        assert_eq!(panel.editor.as_ref().unwrap().text.cursor(), (0, 0));
        assert!(panel.notice.contains("wrapped"));
        panel.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        assert_eq!(panel.editor.as_ref().unwrap().text.cursor(), (2, 0));
        assert!(panel.notice.contains("wrapped"));
        press(&mut panel, KeyCode::Up);
        assert_eq!(panel.editor.as_ref().unwrap().text.cursor(), (0, 0));
        press(&mut panel, KeyCode::Down);
        assert_eq!(panel.editor.as_ref().unwrap().text.cursor(), (2, 0));
    }

    #[test]
    fn previous_search_from_end_of_line_finds_the_last_match() {
        use ratatui_textarea::CursorMove;
        let mut panel = panel("界 hit hit");
        panel
            .editor
            .as_mut()
            .unwrap()
            .text
            .move_cursor(CursorMove::End);
        control(&mut panel, 'f');
        panel.paste("hit");
        panel.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        assert_eq!(panel.editor.as_ref().unwrap().text.cursor(), (0, 6));
    }

    #[test]
    fn empty_and_missing_searches_do_not_move_or_modify_the_draft() {
        let mut panel = panel("ensure chat");
        panel.paste("# draft ");
        let content = panel.editor.as_ref().unwrap().content();
        let cursor = panel.editor.as_ref().unwrap().text.cursor();
        find(&mut panel, "");
        assert_eq!(panel.notice, "Find text is empty.");
        find(&mut panel, "missing");
        assert_eq!(panel.notice, "No matches.");
        press(&mut panel, KeyCode::Up);
        assert_eq!(panel.notice, "No matches.");
        let editor = panel.editor.as_ref().unwrap();
        assert_eq!(editor.text.cursor(), cursor);
        assert_eq!(editor.content(), content);
        assert!(editor.changed);
        assert!(!editor.navigation.is_match(cursor.0, cursor.1));
    }

    #[test]
    fn escaping_prompts_preserves_clean_and_dirty_editors_and_find_text() {
        for dirty in [false, true] {
            let mut panel = panel("ensure chat");
            if dirty {
                panel.paste("# draft ");
            }
            let content = panel.editor.as_ref().unwrap().content();
            find(&mut panel, "chat");
            let cursor = panel.editor.as_ref().unwrap().text.cursor();
            press(&mut panel, KeyCode::Esc);
            let editor = panel.editor.as_ref().unwrap();
            assert!(!editor.navigation.is_open());
            assert_eq!(editor.text.cursor(), cursor);
            assert_eq!(editor.content(), content);
            assert_eq!(panel.dirty(), dirty);
            control(&mut panel, 'f');
            press(&mut panel, KeyCode::Enter);
            assert!(panel.notice.starts_with("Match "));
            control(&mut panel, 'g');
            panel.paste("1");
            press(&mut panel, KeyCode::Esc);
            assert_eq!(panel.editor.as_ref().unwrap().text.cursor(), cursor);
            assert_eq!(panel.editor.as_ref().unwrap().content(), content);
            press(&mut panel, KeyCode::Esc);
            assert_eq!(panel.editor.is_some(), dirty);
        }
    }

    #[test]
    fn save_undo_and_redo_pass_through_prompts_without_editing_prompt_text_into_document() {
        let original = "ensure chat\r\n";
        let mut panel = panel(original);
        panel.paste("# draft\n");
        let content = panel.editor.as_ref().unwrap().content();
        find(&mut panel, "chat");
        let Event::Save {
            original: saved_original,
            content: saved_content,
            ..
        } = control(&mut panel, 's')
        else {
            panic!("expected the existing save event");
        };
        assert_eq!(saved_original, original);
        assert_eq!(saved_content, content);
        assert!(!panel.editor.as_ref().unwrap().navigation.is_open());
        control(&mut panel, 'g');
        panel.paste("1");
        control(&mut panel, 'z');
        assert_eq!(panel.editor.as_ref().unwrap().content(), original);
        assert!(!panel.dirty());
        control(&mut panel, 'f');
        control(&mut panel, 'y');
        assert_eq!(panel.editor.as_ref().unwrap().content(), content);
        assert!(panel.dirty());
    }

    #[test]
    fn go_to_line_rejects_invalid_input_without_moving_then_accepts_last_line() {
        use ratatui_textarea::CursorMove;
        let mut panel = panel("one\r\ntwo");
        panel
            .editor
            .as_mut()
            .unwrap()
            .text
            .move_cursor(CursorMove::End);
        for value in ["", "0", "-1", "3", "abc", "9999999999"] {
            control(&mut panel, 'g');
            panel.paste(value);
            press(&mut panel, KeyCode::Enter);
            assert!(panel.editor.as_ref().unwrap().navigation.is_open());
            assert_eq!(panel.notice, "Line must be 1..2.");
            assert_eq!(panel.editor.as_ref().unwrap().text.cursor(), (0, 3));
        }
        control(&mut panel, 'u');
        panel.paste("2");
        press(&mut panel, KeyCode::Enter);
        assert_eq!(panel.editor.as_ref().unwrap().text.cursor(), (1, 0));
        assert!(!panel.editor.as_ref().unwrap().navigation.is_open());
        assert_eq!(panel.editor.as_ref().unwrap().content(), "one\r\ntwo");
        assert!(!panel.dirty());
    }

    #[test]
    fn prompt_input_is_bounded_and_rejects_multiline_and_control_pastes() {
        let query = "é".repeat(128);
        let mut panel = panel(&query);
        control(&mut panel, 'f');
        panel.paste(&query);
        press(&mut panel, KeyCode::Char('é'));
        assert!(panel.notice.contains("256 bytes"));
        for value in ["x", "\n", "\r", "\x1b"] {
            panel.paste(value);
            assert!(!panel.notice.is_empty());
        }
        press(&mut panel, KeyCode::Enter);
        assert!(panel.notice.starts_with("Match "));
        control(&mut panel, 'u');
        for value in ["a\nb", "a\r\nb", "\r", "\t", "\x1b[31m"] {
            panel.paste(value);
            assert!(!panel.notice.is_empty());
            press(&mut panel, KeyCode::Enter);
            assert_eq!(panel.notice, "Find text is empty.");
        }
        control(&mut panel, 'g');
        panel.paste("12345678901");
        assert!(!panel.notice.is_empty());
        press(&mut panel, KeyCode::Enter);
        assert_eq!(panel.notice, "Line must be 1..1.");
        assert_eq!(panel.editor.as_ref().unwrap().content(), query);
        assert!(!panel.dirty());
    }

    #[test]
    fn navigation_leaves_plain_keys_and_altgr_for_the_existing_key_handlers() {
        let mut navigation = Navigation::default();
        let mut text = TextArea::default();
        let mut notice = String::new();
        for key in [
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('界'), KeyModifiers::NONE),
            KeyEvent::new(
                KeyCode::Char('f'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            KeyEvent::new(
                KeyCode::Char('g'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
        ] {
            assert!(!navigation.key(&mut text, key, &mut notice));
            assert!(!navigation.is_open());
        }
        let mut panel = panel("");
        press(&mut panel, KeyCode::Char('界'));
        assert_eq!(panel.editor.as_ref().unwrap().content(), "界");
    }

    #[test]
    fn search_and_go_to_line_keep_the_document_cursor_visible_after_rendering() {
        use ratatui::{backend::TestBackend, Terminal};
        let content = format!(
            "{}{}{}needle",
            "ensure chat\n".repeat(80),
            "\t".repeat(20),
            "界".repeat(30)
        );
        let mut panel = panel(&content);
        find(&mut panel, "needle");
        let mut terminal = Terminal::new(TestBackend::new(24, 10)).unwrap();
        terminal
            .draw(|frame| panel.draw(frame, frame.area()))
            .unwrap();
        let editor = panel.editor.as_ref().unwrap();
        assert_eq!(editor.text.cursor(), (80, 50));
        let (top, left) = editor.text.scroll_offset();
        assert!(top as usize <= 80 && 80 < top as usize + 6);
        let visible_width = 22 - editor.text.line_number_width();
        assert!(left as usize <= 140 && 140 < left as usize + visible_width as usize);
        assert_eq!(editor.content(), content);

        control(&mut panel, 'g');
        panel.paste("1");
        press(&mut panel, KeyCode::Enter);
        terminal
            .draw(|frame| panel.draw(frame, frame.area()))
            .unwrap();
        let editor = panel.editor.as_ref().unwrap();
        assert_eq!(editor.text.cursor(), (0, 0));
        assert_eq!(editor.text.scroll_offset(), (0, 0));
        assert!(!panel.dirty());
    }

    #[test]
    fn prompt_cursor_and_suffix_stay_visible_in_narrow_terminals() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut panel = panel("ensure chat");
        control(&mut panel, 'f');
        panel.paste(&format!("{}tail", "界".repeat(40)));
        for width in [1, 4, 9, 12, 40] {
            for height in [1, 2, 6] {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| panel.draw(frame, frame.area()))
                    .unwrap();
                let cursor = terminal.get_cursor_position().unwrap();
                assert!(cursor.x < width && cursor.y < height);
                let rendered: String = terminal
                    .backend()
                    .buffer()
                    .content()
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect();
                if width >= 9 {
                    assert!(rendered.contains("tail"));
                }
            }
        }
        assert_eq!(panel.editor.as_ref().unwrap().content(), "ensure chat");
        assert!(!panel.dirty());
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
