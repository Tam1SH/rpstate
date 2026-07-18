use crate::app::App;
use ratatui::prelude::*;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem},
};

pub fn render(f: &mut Frame, area: Rect, app: &mut App) {
    let mut items = vec![
        ListItem::new("[all]"),
        ListItem::new("[flatten]"),
        ListItem::new("─".repeat(area.width as usize)).style(Style::default().fg(Color::DarkGray)),
    ];

    for (prefix, snapshot) in &app.structs {
        let label = match &snapshot.struct_name {
            Some(name) => format!("{} ({})", prefix, name),
            None => prefix.clone(),
        };
        items.push(ListItem::new(label));
    }

    if let Some(item) = items.get_mut(app.selected) {
        *item = item.clone().style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Structs "))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0)])
        .split(area);

    f.render_widget(list, chunks[0]);
}
