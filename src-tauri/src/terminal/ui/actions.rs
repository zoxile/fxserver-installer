use super::render::{ConfirmationPreview, Input};
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

pub(super) struct Panel {
    actions: Vec<Action>,
    search: Input,
    selected: usize,
    form: Option<Form>,
    output: Option<ConfirmationPreview>,
    show_output: bool,
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
            actions,
            search: Input::default(),
            selected: 0,
            form: None,
            output: None,
            show_output: false,
        }
    }

    pub(super) fn refresh(&mut self, actions: Vec<Action>) {
        self.actions = actions;
        self.form = None;
        self.show_output = false;
        self.search = Input::default();
        self.selected = 0;
    }

    pub(super) fn show_result(&mut self, text: String) {
        if self.form.as_ref().is_some_and(|form| form.submitted) {
            self.form = None;
        }
        self.output = Some(ConfirmationPreview::new(text));
        self.show_output = self.form.is_none();
    }

    pub(super) fn submission_rejected(&mut self) {
        if let Some(form) = &mut self.form {
            form.submitted = false;
        }
    }

    fn matches(&self) -> Vec<usize> {
        let query = self.search.value.to_lowercase();
        self.actions
            .iter()
            .enumerate()
            .filter_map(|(index, action)| {
                let text = format!("{} {}", action.group, action.title).to_lowercase();
                query
                    .split_whitespace()
                    .all(|word| text.contains(word))
                    .then_some(index)
            })
            .collect()
    }

    pub(super) fn paste(&mut self, value: &str) {
        if self.show_output {
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
            self.selected = 0;
        }
    }

    pub(super) fn key(&mut self, key: KeyEvent) -> Option<(Action, Submission)> {
        if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.show_output = self.output.is_some();
            return None;
        }
        if key.code == KeyCode::Esc {
            if self.show_output {
                self.show_output = false;
            } else {
                self.form = None;
            }
            return None;
        }
        if self.show_output {
            if let Some(output) = &mut self.output {
                output.key(key.code);
            }
            return None;
        }
        if let Some(form) = &mut self.form {
            return form.key(key);
        }
        let matches = self.matches();
        match key.code {
            KeyCode::Down => self.selected = (self.selected + 1) % matches.len().max(1),
            KeyCode::Up => {
                self.selected = (self.selected + matches.len().max(1) - 1) % matches.len().max(1)
            }
            KeyCode::Enter => {
                if let Some(index) = matches.get(self.selected) {
                    let action = self.actions[*index].clone();
                    if action.fields.is_empty() {
                        return Some((
                            action.clone(),
                            Submission {
                                id: action.id.into(),
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
                self.selected = 0;
            }
        }
        None
    }

    pub(super) fn draw(&mut self, frame: &mut Frame, area: Rect) {
        if self.show_output {
            if let Some(output) = &mut self.output {
                output.draw_panel(frame, area, "Result | Esc: back");
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
        if inner.width > 0 {
            frame.set_cursor_position((inner.x + cursor, inner.y));
        }
        let matches = self.matches();
        self.selected = self.selected.min(matches.len().saturating_sub(1));
        let items: Vec<_> = matches
            .iter()
            .map(|index| {
                let action = &self.actions[*index];
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{}  ", action.group),
                        Style::new().fg(Color::DarkGray),
                    ),
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
