use super::render::Input;
use crate::terminal::{
    actions::{Action, FieldKind, Submission},
    safe_text,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

mod result;
use result::ResultView;

pub(super) struct Panel {
    actions: Vec<Action>,
    searchable: Vec<String>,
    matches: Vec<usize>,
    filtered_query: String,
    search: Input,
    selected: usize,
    form: Option<Form>,
    output: Option<ResultView>,
    show_output: bool,
    #[cfg(test)]
    filter_runs: usize,
}

struct Form {
    action: Action,
    inputs: Vec<Input>,
    selected: usize,
    error: String,
    reveal: bool,
    submitted: bool,
}

impl Panel {
    pub(super) fn new(actions: Vec<Action>) -> Self {
        Self {
            searchable: Self::searchable(&actions),
            matches: (0..actions.len()).collect(),
            actions,
            filtered_query: String::new(),
            search: Input::default(),
            selected: 0,
            form: None,
            output: None,
            show_output: false,
            #[cfg(test)]
            filter_runs: 0,
        }
    }

    pub(super) fn refresh(&mut self, actions: Vec<Action>) {
        self.searchable = Self::searchable(&actions);
        self.matches = (0..actions.len()).collect();
        self.actions = actions;
        self.filtered_query.clear();
        self.form = None;
        self.show_output = false;
        self.search = Input::default();
        self.selected = 0;
    }

    pub(super) fn show_result(&mut self, text: String) {
        if self.form.as_ref().is_some_and(|form| form.submitted) {
            self.form = None;
        }
        self.output = Some(ResultView::new(text));
        self.show_output = self.form.is_none();
    }

    pub(super) fn submission_rejected(&mut self) {
        if let Some(form) = &mut self.form {
            form.submitted = false;
        }
    }

    fn searchable(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .map(|action| format!("{} {}", action.group, action.title).to_lowercase())
            .collect()
    }

    fn filter_changed_query(&mut self) {
        if self.filtered_query == self.search.value {
            return;
        }
        self.filtered_query.clone_from(&self.search.value);
        let query = self.search.value.to_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        self.matches.clear();
        self.matches.extend(
            self.searchable
                .iter()
                .enumerate()
                .filter_map(|(index, text)| {
                    words
                        .iter()
                        .all(|word| text.contains(*word))
                        .then_some(index)
                }),
        );
        self.selected = 0;
        #[cfg(test)]
        {
            self.filter_runs += 1;
        }
    }

    pub(super) fn paste(&mut self, value: &str) {
        if self.show_output {
            if let Some(output) = &mut self.output {
                output.paste(value);
            }
            return;
        }
        if let Some(form) = &mut self.form {
            form.submitted = false;
            if matches!(form.action.fields[form.selected].kind, FieldKind::Choice(_)) {
                return;
            }
            if let Err(error) = form.inputs[form.selected].paste(value) {
                form.error = error.into();
            }
        } else if self.search.paste(value).is_ok() {
            self.filter_changed_query();
        }
    }

    pub(super) fn key(&mut self, key: KeyEvent) -> Option<(Action, Submission)> {
        if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.show_output = self.output.is_some();
            return None;
        }
        if key.code == KeyCode::Char('f') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(output) = &mut self.output {
                self.show_output = true;
                output.begin_search();
            }
            return None;
        }
        if self.show_output {
            if let Some(output) = &mut self.output {
                self.show_output = output.key(key);
            }
            return None;
        }
        if key.code == KeyCode::Esc {
            self.form = None;
            return None;
        }
        if let Some(form) = &mut self.form {
            return form.key(key);
        }
        let matches = &self.matches;
        match key.code {
            KeyCode::Down => self.selected = (self.selected + 1) % matches.len().max(1),
            KeyCode::Up => {
                self.selected = (self.selected + matches.len().max(1) - 1) % matches.len().max(1)
            }
            KeyCode::Enter => {
                if let Some(index) = matches.get(self.selected) {
                    let action = self.actions[*index].clone();
                    if action.fields.is_empty() {
                        let id = action.id.into();
                        return Some((
                            action,
                            Submission {
                                id,
                                values: Default::default(),
                            },
                        ));
                    }
                    let inputs = action
                        .fields
                        .iter()
                        .map(|f| Input::new(f.value.clone()))
                        .collect();
                    self.form = Some(Form {
                        action,
                        inputs,
                        selected: 0,
                        error: String::new(),
                        reveal: false,
                        submitted: false,
                    });
                }
            }
            _ => {
                self.search.key(key);
                self.filter_changed_query();
            }
        }
        None
    }

    pub(super) fn draw(&mut self, frame: &mut Frame, area: Rect) {
        if self.show_output {
            if let Some(output) = &mut self.output {
                output.draw(frame, area);
                return;
            }
        }
        if let Some(form) = &mut self.form {
            form.draw(frame, area);
            return;
        }
        let [search, list, description] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .areas(area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Search actions | Ctrl+O: last result");
        let inner = block.inner(search);
        frame.render_widget(block, search);
        let (text, cursor) = self.search.visible(inner.width as usize, false);
        frame.render_widget(Paragraph::new(text), inner);
        if inner.width > 0 && inner.height > 0 {
            frame.set_cursor_position((inner.x + cursor, inner.y));
        }
        let matches = &self.matches;
        let items: Vec<_> = matches
            .iter()
            .map(|index| {
                let action = &self.actions[*index];
                ListItem::new(Line::from(vec![
                    Span::styled(action.group, Style::new().fg(Color::DarkGray)),
                    Span::raw("  "),
                    Span::raw(action.title),
                ]))
            })
            .collect();
        if items.is_empty() {
            frame.render_widget(Paragraph::new("No matching actions."), list);
        } else {
            let mut state = ListState::default().with_selected(Some(self.selected));
            frame.render_stateful_widget(
                List::new(items)
                    .highlight_symbol("> ")
                    .highlight_style(Style::new().fg(Color::Cyan).bold()),
                list,
                &mut state,
            );
        }
        let text = matches
            .get(self.selected)
            .map(|index| self.actions[*index].description)
            .unwrap_or("");
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .fg(Color::Gray),
            description,
        );
    }
}

impl Form {
    fn key(&mut self, key: KeyEvent) -> Option<(Action, Submission)> {
        self.error.clear();
        self.submitted = false;
        if key.code == KeyCode::Char('v') && key.modifiers.contains(KeyModifiers::ALT) {
            self.reveal = !self.reveal;
            return None;
        }
        if matches!(
            key.code,
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down
        ) {
            self.reveal = false;
        }
        match key.code {
            KeyCode::Tab | KeyCode::Down => self.selected = (self.selected + 1) % self.inputs.len(),
            KeyCode::BackTab | KeyCode::Up => {
                self.selected = (self.selected + self.inputs.len() - 1) % self.inputs.len()
            }
            KeyCode::Enter | KeyCode::F(10) => {
                for (index, field) in self.action.fields.iter().enumerate() {
                    let value = &self.inputs[index].value;
                    if field.required && value.trim().is_empty() {
                        self.selected = index;
                        self.error = format!("{} is required.", field.label);
                        return None;
                    }
                    if let FieldKind::Choice(options) = &field.kind {
                        if !options.contains(value) {
                            self.error = format!("Choose a valid {}.", field.label);
                            self.selected = index;
                            return None;
                        }
                    }
                }
                let submission = Submission {
                    id: self.action.id.into(),
                    values: self
                        .action
                        .fields
                        .iter()
                        .zip(&self.inputs)
                        .map(|(field, input)| (field.key.into(), input.value.clone()))
                        .collect(),
                };
                self.submitted = true;
                return Some((self.action.clone(), submission));
            }
            _ => {
                if let FieldKind::Choice(options) = &self.action.fields[self.selected].kind {
                    if !options.is_empty()
                        && matches!(
                            key.code,
                            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                        )
                    {
                        let index = options
                            .iter()
                            .position(|s| s == &self.inputs[self.selected].value)
                            .unwrap_or(0);
                        let next = if key.code == KeyCode::Left {
                            (index + options.len() - 1) % options.len()
                        } else {
                            (index + 1) % options.len()
                        };
                        self.inputs[self.selected] = Input::new(options[next].clone());
                    }
                } else if !key.modifiers.contains(KeyModifiers::ALT) {
                    self.inputs[self.selected].key(key);
                }
            }
        }
        None
    }

    fn draw(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(self.action.title)
            .title_bottom("Tab: field  Enter: review/run  Esc: cancel");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let description_height = if inner.height < 7 { 0 } else { 2 };
        let [description, fields, status] = Layout::vertical([
            Constraint::Length(description_height),
            Constraint::Min(2),
            Constraint::Length(1),
        ])
        .areas(inner);
        frame.render_widget(
            Paragraph::new(self.action.description)
                .wrap(Wrap { trim: false })
                .fg(Color::Gray),
            description,
        );
        let rows = (fields.height as usize / 2).max(1);
        let start = self.selected.saturating_sub(rows - 1);
        for (index, (field, input)) in self
            .action
            .fields
            .iter()
            .zip(&self.inputs)
            .enumerate()
            .skip(start)
            .take(rows)
        {
            let y = fields.y + ((index - start) * 2) as u16;
            if y + 1 >= fields.bottom() {
                break;
            }
            let active = index == self.selected;
            let style = if active {
                Style::new().fg(Color::Cyan)
            } else {
                Style::new().fg(Color::Gray)
            };
            let hint = match field.kind {
                FieldKind::Choice(_) => " [Left/Right]",
                FieldKind::Secret => " [Alt+V: show/hide]",
                _ => "",
            };
            frame.render_widget(
                Paragraph::new(format!("{}{hint}", field.label)).style(style),
                Rect::new(fields.x, y, fields.width, 1),
            );
            let (value, cursor) = input.visible(
                fields.width.saturating_sub(2) as usize,
                matches!(field.kind, FieldKind::Secret) && !(active && self.reveal),
            );
            frame.render_widget(
                Paragraph::new(format!("  {}", safe_text(&value))).style(style),
                Rect::new(fields.x, y + 1, fields.width, 1),
            );
            if active && fields.width > 2 {
                frame.set_cursor_position((fields.x + 2 + cursor, y + 1));
            }
        }
        frame.render_widget(
            Paragraph::new(&*self.error)
                .wrap(Wrap { trim: false })
                .fg(Color::Red),
            status,
        );
    }
}

pub(super) fn confirmation(action: &Action, submission: &Submission) -> String {
    let mut text = format!(
        "{}\n\n{}",
        action.title,
        action.confirmation.unwrap_or("Review this action.")
    );
    for field in &action.fields {
        let value = if matches!(field.kind, FieldKind::Secret) {
            "[hidden]"
        } else {
            submission.get(field.key)
        };
        text.push_str(&format!("\n{}: {}", field.label, safe_text(value)));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    fn action() -> Action {
        Action {
            id: "test",
            group: "Database",
            title: "Create user",
            description: "New user",
            fields: vec![
                crate::terminal::actions::Field::text("user", "User", ""),
                crate::terminal::actions::Field::text("password", "Password", "secret-fixture")
                    .secret(),
                crate::terminal::actions::Field::text("scope", "Scope", "database")
                    .choice(&["database", "global"]),
            ],
            confirmation: Some("Confirm creation"),
        }
    }

    #[test]
    fn filter_cache_invalidates_on_query_and_catalog_changes_only() {
        let mut server = action();
        server.id = "server";
        server.group = "Server";
        server.title = "Restart server";
        let mut panel = Panel::new(vec![action(), server.clone()]);
        panel.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(panel.selected, 1);
        panel.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        panel.paste("");
        assert_eq!(panel.selected, 1);
        assert_eq!(panel.filter_runs, 0);
        panel.paste("USER dataBASE");
        assert_eq!(panel.matches, [0]);
        assert_eq!(panel.selected, 0);
        assert_eq!(panel.filter_runs, 1);
        panel.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(panel.matches, [0, 1]);
        assert_eq!(panel.filter_runs, 2);
        panel.paste("unmatched");
        assert!(panel.matches.is_empty());
        panel.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(panel.selected, 0);
        assert!(panel
            .key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .is_none());
        panel.refresh(vec![server]);
        assert_eq!(panel.matches, [0]);
        assert_eq!(panel.searchable, ["server restart server"]);
        assert!(panel.search.value.is_empty());
        panel.paste("database");
        assert!(panel.matches.is_empty());
    }

    #[test]
    fn repeated_draws_and_navigation_reuse_filter_cache() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut panel = Panel::new(vec![action(); 84]);
        panel.paste("create");
        let allocation = panel.matches.as_ptr();
        let searchable = panel.searchable.as_ptr();
        let mut terminal = Terminal::new(TestBackend::new(50, 14)).unwrap();
        for _ in 0..50 {
            panel.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            terminal
                .draw(|frame| panel.draw(frame, frame.area()))
                .unwrap();
        }
        assert_eq!(panel.filter_runs, 1);
        assert_eq!(panel.matches.as_ptr(), allocation);
        assert_eq!(panel.searchable.as_ptr(), searchable);
        assert_eq!(panel.selected, 50);
    }

    #[test]
    fn filtering_normalizes_unicode_without_changing_actions() {
        let mut item = action();
        item.title = "\u{c5}terst\u{e4}ll database";
        let mut panel = Panel::new(vec![item]);
        panel.paste("\u{e5}TERST\u{c4}LL");
        assert_eq!(panel.matches, [0]);
        assert_eq!(panel.actions[0].title, "\u{c5}terst\u{e4}ll database");
    }

    #[test]
    fn result_search_preserves_a_masked_draft_and_escape_restores_it() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut panel = Panel::new(vec![action()]);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        panel.key(key(KeyCode::Enter));
        panel.paste("draft");
        panel.show_result("First match and another match.".into());
        panel.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        panel.paste("match");
        panel.key(key(KeyCode::Enter));
        let mut terminal = Terminal::new(TestBackend::new(50, 14)).unwrap();
        terminal
            .draw(|frame| panel.draw(frame, frame.area()))
            .unwrap();
        assert!(panel.show_output);
        assert_eq!(panel.form.as_ref().unwrap().inputs[0].value, "draft");
        assert!(!terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("secret-fixture"));
        panel.key(key(KeyCode::Esc));
        assert!(!panel.show_output);
        assert_eq!(panel.form.as_ref().unwrap().inputs[0].value, "draft");
        panel.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert!(panel.show_output);
    }
    #[test]
    fn forms_validate_and_keep_passwords_out_of_previews() {
        let mut panel = Panel::new(vec![action()]);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        panel.key(key(KeyCode::Enter));
        assert!(panel.key(key(KeyCode::Enter)).is_none());
        panel.paste("alice");
        let (action, submission) = panel.key(key(KeyCode::Enter)).unwrap();
        assert_eq!(submission.get("user"), "alice");
        assert!(!confirmation(&action, &submission).contains("secret-fixture"));
    }
    #[test]
    fn selection_wraps_and_paste_cannot_submit() {
        let mut panel = Panel::new(vec![action(), action()]);
        panel.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(panel.selected, 1);
        panel.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(panel.selected, 0);
        panel.paste("create\ny");
        assert!(panel.search.value.is_empty());
    }
    #[test]
    fn completed_results_preserve_another_form_and_remain_accessible() {
        let mut panel = Panel::new(vec![action()]);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        panel.key(key(KeyCode::Enter));
        panel.paste("draft");
        panel.show_result("Earlier operation completed.".into());
        assert!(!panel.show_output);
        assert_eq!(panel.form.as_ref().unwrap().inputs[0].value, "draft");
        panel.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert!(panel.show_output);
        panel.paste("ignored");
        panel.key(key(KeyCode::Esc));
        assert_eq!(panel.form.as_ref().unwrap().inputs[0].value, "draft");
        panel.key(key(KeyCode::Enter)).unwrap();
        panel.show_result("Current operation completed.".into());
        assert!(panel.show_output);
        assert!(panel.form.is_none());
        panel.refresh(vec![action()]);
        panel.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert!(panel.show_output);
    }
    #[test]
    fn rejected_submission_does_not_lose_the_draft_to_another_result() {
        let mut panel = Panel::new(vec![action()]);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        panel.key(key(KeyCode::Enter));
        panel.paste("draft");
        panel.key(key(KeyCode::Enter)).unwrap();
        panel.submission_rejected();
        panel.show_result("Previous operation finished.".into());
        assert!(!panel.show_output);
        assert_eq!(panel.form.as_ref().unwrap().inputs[0].value, "draft");
    }
    #[test]
    fn forms_render_at_small_sizes_without_leaking_secrets() {
        use ratatui::{backend::TestBackend, Terminal};
        for (width, height) in [(46, 8), (80, 20), (130, 36)] {
            let mut panel = Panel::new(vec![action()]);
            panel.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| panel.draw(frame, frame.area()))
                .unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(!text.contains("secret-fixture"));
            assert!(text.contains("Create user"));
        }
    }
}
