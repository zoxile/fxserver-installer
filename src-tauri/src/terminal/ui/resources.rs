use super::{lists, render::Input};
use crate::{models::fxserver::FxserverResourceInfo, terminal::safe_text};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, ListItem, Paragraph},
    Frame,
};

#[derive(Default)]
pub(super) struct Panel {
    items: Vec<FxserverResourceInfo>,
    searchable: Vec<String>,
    matches: Vec<usize>,
    search: Input,
    selected: usize,
    filtering: bool,
}

impl Panel {
    pub(super) fn len(&self) -> usize {
        self.items.len()
    }

    pub(super) fn selected(&self) -> Option<&FxserverResourceInfo> {
        self.matches
            .get(self.selected)
            .and_then(|index| self.items.get(*index))
    }

    pub(super) fn replace(&mut self, items: Vec<FxserverResourceInfo>) {
        let selected = self.selected().map(|resource| resource.path.clone());
        self.searchable = items
            .iter()
            .map(|resource| {
                format!(
                    "{} {} {}",
                    resource.name,
                    resource.version.as_deref().unwrap_or(""),
                    resource.repository.as_deref().unwrap_or("")
                )
                .to_lowercase()
            })
            .collect();
        self.items = items;
        self.filter();
        if let Some(path) = selected {
            self.selected = self
                .matches
                .iter()
                .position(|index| self.items[*index].path == path)
                .unwrap_or(0);
        }
    }

    fn filter(&mut self) {
        let query = self.search.value.to_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        self.matches = self
            .searchable
            .iter()
            .enumerate()
            .filter_map(|(index, text)| {
                words
                    .iter()
                    .all(|word| text.contains(word))
                    .then_some(index)
            })
            .collect();
        self.selected = 0;
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
        if matches!(key.code, KeyCode::Char('/')) && !self.filtering
            || key.code == KeyCode::Char('f') && key.modifiers == KeyModifiers::CONTROL
        {
            self.filtering = true;
            return true;
        }
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
        let last = self.matches.len().saturating_sub(1);
        match key.code {
            KeyCode::Down => self.selected = self.selected.saturating_add(1).min(last),
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::PageDown => self.selected = self.selected.saturating_add(10).min(last),
            KeyCode::PageUp => self.selected = self.selected.saturating_sub(10),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = last,
            KeyCode::Esc if !self.search.value.is_empty() => {
                self.search = Input::default();
                self.filter();
            }
            _ => return false,
        }
        true
    }

    pub(super) fn draw(&self, frame: &mut Frame, area: Rect) {
        let [search, list] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(
                "Filter {}/{} | /: edit  Esc: clear",
                self.matches.len(),
                self.items.len()
            ))
            .border_style(Style::new().fg(if self.filtering {
                Color::Cyan
            } else {
                Color::DarkGray
            }));
        let inner = block.inner(search);
        frame.render_widget(block, search);
        let (text, cursor) = self.search.visible(inner.width as usize, false);
        frame.render_widget(Paragraph::new(text), inner);
        if self.filtering && !inner.is_empty() {
            frame.set_cursor_position((inner.x + cursor, inner.y));
        }
        lists::draw(
            frame,
            list,
            self.matches.len(),
            self.selected,
            |index| {
                let resource = &self.items[self.matches[index]];
                ListItem::new(format!(
                    "{}  {}",
                    safe_text(&resource.name),
                    safe_text(resource.version.as_deref().unwrap_or("no version"))
                ))
            },
            if self.items.is_empty() {
                "No resources loaded. Refresh [R]"
            } else {
                "No matching resources."
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn resource(name: &str) -> FxserverResourceInfo {
        FxserverResourceInfo {
            name: name.into(),
            path: format!("/resources/{name}"),
            manifest_path: String::new(),
            manifest_name: "fxmanifest.lua".into(),
            version: Some("1.0".into()),
            repository: None,
        }
    }

    #[test]
    fn filtering_maps_selection_to_the_original_resource() {
        let mut panel = Panel::default();
        panel.replace(vec![
            resource("alpha"),
            resource("Zulu"),
            resource("zulu-extra"),
        ]);
        panel.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        panel.paste("ZULU 1.0");
        panel.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(panel.selected().unwrap().name, "Zulu");
        panel.key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(panel.selected().unwrap().name, "zulu-extra");
        panel.replace(vec![resource("zulu-extra"), resource("Zulu")]);
        assert_eq!(panel.selected().unwrap().name, "zulu-extra");
        panel.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(panel.matches.len(), 2);
    }

    #[test]
    fn search_is_bounded_unicode_safe_and_never_triggers_actions() {
        let mut panel = Panel::default();
        panel.replace(vec![resource("日本語"), resource("foo")]);
        panel.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        panel.paste("日本語");
        assert_eq!(panel.matches.len(), 1);
        assert!(panel.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)));
        assert!(panel.selected().is_none());
        panel.paste(&"x".repeat(257));
        assert_eq!(panel.search.value, "日本語s");
        panel.paste("\x1b[31m");
        assert_eq!(panel.search.value, "日本語s");
    }

    #[test]
    fn full_search_can_still_be_cleared_with_control_u() {
        let mut panel = Panel::default();
        panel.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        panel.paste(&"x".repeat(256));
        panel.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(panel.search.value.is_empty());
    }
}
