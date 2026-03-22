// === tui/setup.rs — First-login setup checklist view ===

use crate::tui::theme;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use zos_core::commands::setup::{self, SetupStep};
use zos_core::config;

#[derive(Debug)]
pub struct SetupView {
    steps: Vec<SetupStep>,
    list_state: ListState,
    message: Option<(String, bool)>,
    running_step: Option<usize>,
    setup_done: bool,
}

impl SetupView {
    pub fn new() -> Self {
        let steps = setup::get_setup_steps();
        let mut list_state = ListState::default();
        if !steps.is_empty() {
            list_state.select(Some(0));
        }
        let setup_done = config::is_setup_done();
        Self {
            steps,
            list_state,
            message: None,
            running_step: None,
            setup_done,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.running_step.is_some() {
            return; // Ignore input while a step is running
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Enter => self.run_selected(),
            KeyCode::Char('a') => self.run_all_pending(),
            _ => {}
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(8),    // Checklist
                Constraint::Length(4), // Message
                Constraint::Length(3), // Keybinds
            ])
            .split(area);

        self.render_header(frame, chunks[0]);
        self.render_checklist(frame, chunks[1]);
        self.render_message(frame, chunks[2]);
        self.render_keybinds(frame, chunks[3]);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let status_text = if self.setup_done {
            Span::styled("  First-login setup: COMPLETE", theme::pass_style())
        } else {
            Span::styled("  First-login setup: PENDING", theme::warn_style())
        };

        let block = Block::default()
            .title(Span::styled(" Setup ", theme::title_style()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::SURFACE0))
            .style(Style::default().bg(theme::BASE));

        frame.render_widget(Paragraph::new(Line::from(status_text)).block(block), area);
    }

    fn render_checklist(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .steps
            .iter()
            .enumerate()
            .map(|(idx, step)| {
                let is_running = self.running_step == Some(idx);

                let (icon, name_style) = if is_running {
                    ("[..]", theme::accent_style())
                } else if step.installed {
                    ("[ok]", theme::dimmed_style())
                } else {
                    ("[  ]", theme::text_style())
                };

                let desc_style = if step.installed {
                    theme::dimmed_style()
                } else {
                    theme::subtext_style()
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!(" {} ", icon),
                        if step.installed {
                            theme::pass_style()
                        } else if is_running {
                            theme::accent_style()
                        } else {
                            theme::warn_style()
                        },
                    ),
                    Span::styled(
                        format!("{:<16}", step.name),
                        name_style.add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(&step.description, desc_style),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(Span::styled(" Setup Steps ", theme::title_style()))
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
            None => {
                if self.running_step.is_some() {
                    Paragraph::new(Line::from(Span::styled(
                        "  Running setup step...",
                        theme::accent_style(),
                    )))
                } else {
                    Paragraph::new(Line::from(Span::styled(
                        "  Select a step and press Enter, or 'a' to run all pending.",
                        theme::subtext_style(),
                    )))
                }
            }
        };

        frame.render_widget(content.block(block), area);
    }

    fn render_keybinds(&self, frame: &mut Frame, area: Rect) {
        let hints = Line::from(vec![
            Span::styled(" [j/k]", theme::keybind_style()),
            Span::styled(" Navigate  ", theme::subtext_style()),
            Span::styled("[Enter]", theme::keybind_style()),
            Span::styled(" Run Selected  ", theme::subtext_style()),
            Span::styled("[a]", theme::keybind_style()),
            Span::styled(" Run All  ", theme::subtext_style()),
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
        if self.steps.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let len = self.steps.len();
        let next = if delta < 0 {
            if current == 0 {
                len - 1
            } else {
                current - 1
            }
        } else {
            if current >= len - 1 {
                0
            } else {
                current + 1
            }
        };
        self.list_state.select(Some(next));
    }

    fn run_selected(&mut self) {
        if let Some(idx) = self.list_state.selected() {
            if idx < self.steps.len() && !self.steps[idx].installed {
                self.running_step = Some(idx);
                match setup::run_setup_step(&self.steps[idx]) {
                    Ok(()) => {
                        self.steps[idx].installed = true;
                        self.message =
                            Some((format!("Installed: {}", self.steps[idx].name), false));
                    }
                    Err(e) => {
                        self.message = Some((format!("Error: {}", e), true));
                    }
                }
                self.running_step = None;
                self.check_all_done();
            }
        }
    }

    fn run_all_pending(&mut self) {
        let mut installed_count = 0;
        let mut last_error: Option<String> = None;

        for idx in 0..self.steps.len() {
            if !self.steps[idx].installed {
                self.running_step = Some(idx);
                match setup::run_setup_step(&self.steps[idx]) {
                    Ok(()) => {
                        self.steps[idx].installed = true;
                        installed_count += 1;
                    }
                    Err(e) => {
                        last_error = Some(format!("{}: {}", self.steps[idx].name, e));
                        // Continue with remaining steps
                    }
                }
            }
        }
        self.running_step = None;

        if let Some(err) = last_error {
            self.message = Some((
                format!(
                    "Installed {} step(s), but errors occurred: {}",
                    installed_count, err
                ),
                true,
            ));
        } else {
            self.message = Some((
                format!("Installed {} step(s) successfully.", installed_count),
                false,
            ));
        }

        self.check_all_done();
    }

    fn check_all_done(&mut self) {
        if self.steps.iter().all(|s| s.installed) {
            let _ = setup::mark_setup_done();
            self.setup_done = true;
        }
    }
}
