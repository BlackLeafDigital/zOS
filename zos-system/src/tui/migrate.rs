// === tui/migrate.rs — Migration view with selection and apply ===

use crate::commands::migrate::{apply_migrations, plan_migrations, MigrationAction};
use crate::tui::theme;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

#[derive(Debug)]
pub struct MigrateView {
    actions: Vec<MigrationAction>,
    list_state: ListState,
    message: Option<(String, bool)>, // (text, is_error)
}

impl MigrateView {
    pub fn new() -> Self {
        let actions = plan_migrations();
        let mut list_state = ListState::default();
        if !actions.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            actions,
            list_state,
            message: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Enter => self.apply_selected(),
            KeyCode::Char('a') => self.apply_all(),
            _ => {}
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(6),    // Migration list
                Constraint::Length(4), // Status/message
                Constraint::Length(3), // Keybinds
            ])
            .split(area);

        self.render_list(frame, chunks[0]);
        self.render_message(frame, chunks[1]);
        self.render_keybinds(frame, chunks[2]);
    }

    fn render_list(&mut self, frame: &mut Frame, area: Rect) {
        if self.actions.is_empty() {
            let block = Block::default()
                .title(Span::styled(" Migrations ", theme::title_style()))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::SURFACE0))
                .style(Style::default().bg(theme::BASE));

            let text = Paragraph::new(Line::from(vec![Span::styled(
                "  All configs are up to date. Nothing to migrate.",
                theme::pass_style(),
            )]))
            .block(block);

            frame.render_widget(text, area);
            return;
        }

        let items: Vec<ListItem> = self
            .actions
            .iter()
            .map(|action| {
                let (icon, style) = if action.applied {
                    ("[ok]", theme::pass_style())
                } else {
                    ("[  ]", theme::warn_style())
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {} ", icon), style),
                    Span::styled(&action.area, theme::text_style().add_modifier(Modifier::BOLD)),
                    Span::styled(" - ", theme::subtext_style()),
                    Span::styled(&action.description, theme::subtext_style()),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(Span::styled(
                        format!(" Migrations ({} pending) ", self.pending_count()),
                        theme::title_style(),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::SURFACE0))
                    .style(Style::default().bg(theme::BASE)),
            )
            .highlight_style(theme::highlight_style())
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_message(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::SURFACE0))
            .style(Style::default().bg(theme::BASE));

        let content = match &self.message {
            Some((msg, is_error)) => {
                let style = if *is_error {
                    theme::fail_style()
                } else {
                    theme::pass_style()
                };
                Paragraph::new(Line::from(Span::styled(format!("  {}", msg), style)))
            }
            None => Paragraph::new(Line::from(Span::styled(
                "  Select a migration and press Enter, or 'a' to apply all.",
                theme::subtext_style(),
            ))),
        };

        frame.render_widget(content.block(block), area);
    }

    fn render_keybinds(&self, frame: &mut Frame, area: Rect) {
        let hints = Line::from(vec![
            Span::styled(" [j/k]", theme::keybind_style()),
            Span::styled(" Navigate  ", theme::subtext_style()),
            Span::styled("[Enter]", theme::keybind_style()),
            Span::styled(" Apply Selected  ", theme::subtext_style()),
            Span::styled("[a]", theme::keybind_style()),
            Span::styled(" Apply All  ", theme::subtext_style()),
            Span::styled("[Esc]", theme::keybind_style()),
            Span::styled(" Back  ", theme::subtext_style()),
            Span::styled("[q]", theme::keybind_style()),
            Span::styled(" Quit", theme::subtext_style()),
        ]);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::SURFACE0))
            .style(Style::default().bg(theme::MANTLE));

        frame.render_widget(Paragraph::new(hints).block(block), area);
    }

    fn move_selection(&mut self, delta: i32) {
        if self.actions.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let len = self.actions.len();
        let next = if delta < 0 {
            if current == 0 { len - 1 } else { current - 1 }
        } else {
            if current >= len - 1 { 0 } else { current + 1 }
        };
        self.list_state.select(Some(next));
    }

    fn apply_selected(&mut self) {
        if let Some(idx) = self.list_state.selected() {
            if idx < self.actions.len() && !self.actions[idx].applied {
                let mut single = vec![self.actions[idx].clone()];
                match apply_migrations(&mut single) {
                    Ok(()) => {
                        self.actions[idx].applied = true;
                        self.message = Some((
                            format!("Applied migration: {}", self.actions[idx].area),
                            false,
                        ));
                    }
                    Err(e) => {
                        self.message = Some((format!("Error: {}", e), true));
                    }
                }
            }
        }
    }

    fn apply_all(&mut self) {
        match apply_migrations(&mut self.actions) {
            Ok(()) => {
                let count = self.actions.iter().filter(|a| a.applied).count();
                self.message = Some((
                    format!("Applied {} migration(s) successfully.", count),
                    false,
                ));
            }
            Err(e) => {
                self.message = Some((format!("Error during migration: {}", e), true));
            }
        }
    }

    fn pending_count(&self) -> usize {
        self.actions.iter().filter(|a| !a.applied).count()
    }
}
