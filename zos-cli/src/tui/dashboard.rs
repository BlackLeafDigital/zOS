// === tui/dashboard.rs — Main dashboard view ===

use zos_core::commands::status::{get_config_status, get_system_info};
use crate::tui::theme;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

pub fn render(frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // OS info
            Constraint::Min(8),    // Config status table
            Constraint::Length(3), // Keybind hints
        ])
        .split(area);

    render_system_info(frame, chunks[0]);
    render_config_table(frame, chunks[1]);
    render_keybinds(frame, chunks[2]);
}

fn render_system_info(frame: &mut Frame, area: Rect) {
    let info = get_system_info();

    let lines = vec![
        Line::from(vec![
            Span::styled("  OS Version:    ", theme::subtext_style()),
            Span::styled(&info.os_version, theme::text_style()),
        ]),
        Line::from(vec![
            Span::styled("  Image:         ", theme::subtext_style()),
            Span::styled(&info.image_name, theme::text_style()),
        ]),
        Line::from(vec![
            Span::styled("  Fedora:        ", theme::subtext_style()),
            Span::styled(&info.fedora_version, theme::text_style()),
        ]),
        Line::from(vec![
            Span::styled("  Last Update:   ", theme::subtext_style()),
            Span::styled(&info.last_update, theme::text_style()),
        ]),
    ];

    let block = Block::default()
        .title(Span::styled(
            " zOS System ",
            theme::title_style().add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BLUE))
        .style(Style::default().bg(theme::BASE));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_config_table(frame: &mut Frame, area: Rect) {
    let config_areas = get_config_status();

    let header = Row::new(vec![
        Cell::from("Status").style(theme::title_style()),
        Cell::from("Area").style(theme::title_style()),
        Cell::from("User Ver").style(theme::title_style()),
        Cell::from("System Ver").style(theme::title_style()),
    ])
    .height(1);

    let rows: Vec<Row> = config_areas
        .iter()
        .map(|area_item| {
            let (icon, style) = if area_item.up_to_date {
                (" [ok] ", theme::pass_style())
            } else {
                (" [!!] ", theme::warn_style())
            };

            Row::new(vec![
                Cell::from(icon).style(style),
                Cell::from(area_item.name.as_str()).style(theme::text_style()),
                Cell::from(format!("v{}", area_item.user_version)).style(theme::subtext_style()),
                Cell::from(format!("v{}", area_item.system_version)).style(theme::subtext_style()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Min(15),
            Constraint::Length(12),
            Constraint::Length(12),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(Span::styled(" Config Status ", theme::title_style()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::SURFACE0))
            .style(Style::default().bg(theme::BASE)),
    );

    frame.render_widget(table, area);
}

fn render_keybinds(frame: &mut Frame, area: Rect) {
    let hints = Line::from(vec![
        Span::styled(" [m]", theme::keybind_style()),
        Span::styled(" Migrate  ", theme::subtext_style()),
        Span::styled("[d]", theme::keybind_style()),
        Span::styled(" Doctor  ", theme::subtext_style()),
        Span::styled("[u]", theme::keybind_style()),
        Span::styled(" Update  ", theme::subtext_style()),
        Span::styled("[g]", theme::keybind_style()),
        Span::styled(" Grub  ", theme::subtext_style()),
        Span::styled("[s]", theme::keybind_style()),
        Span::styled(" Setup  ", theme::subtext_style()),
        Span::styled("[q]", theme::keybind_style()),
        Span::styled(" Quit", theme::subtext_style()),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::SURFACE0))
        .style(Style::default().bg(theme::MANTLE));

    let paragraph = Paragraph::new(hints).block(block);
    frame.render_widget(paragraph, area);
}
