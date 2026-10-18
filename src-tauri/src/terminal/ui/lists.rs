use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{List, ListItem, ListState, Paragraph},
    Frame,
};
use std::ops::Range;

fn viewport(total: usize, selected: usize, height: usize) -> Range<usize> {
    let selected = selected.min(total.saturating_sub(1));
    let start = selected
        .saturating_sub(height / 2)
        .min(total.saturating_sub(height));
    start..start.saturating_add(height).min(total)
}

pub(super) fn draw<'a>(
    frame: &mut Frame,
    area: Rect,
    total: usize,
    selected: usize,
    item: impl FnMut(usize) -> ListItem<'a>,
    empty: &str,
) {
    if area.is_empty() {
        return;
    }
    if total == 0 {
        frame.render_widget(Paragraph::new(empty), area);
        return;
    }
    let range = viewport(total, selected, area.height as usize);
    let mut state = ListState::default()
        .with_selected(Some(selected.min(total - 1).saturating_sub(range.start)));
    frame.render_stateful_widget(
        List::new(range.map(item).collect::<Vec<_>>())
            .highlight_symbol("> ")
            .highlight_style(Style::new().fg(Color::Cyan).bold()),
        area,
        &mut state,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn viewport_keeps_selection_visible_and_handles_empty_space() {
        for total in [0, 1, 10, 20_000] {
            for height in [0, 1, 8, 30] {
                for selected in [0, 5, 19_999, usize::MAX] {
                    let range = viewport(total, selected, height);
                    assert!(range.end <= total && range.len() <= height);
                    if total > 0 && height > 0 {
                        assert!(range.contains(&selected.min(total - 1)));
                    }
                }
            }
        }
    }

    #[test]
    fn rendering_formats_only_visible_rows() {
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let mut formatted = 0;
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    frame.area(),
                    20_000,
                    19_999,
                    |index| {
                        formatted += 1;
                        ListItem::new(format!("resource-{index}"))
                    },
                    "empty",
                )
            })
            .unwrap();
        assert_eq!(formatted, 20);
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("> resource-19999"));
    }
}
