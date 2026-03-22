// === tui/grub.rs — GRUB management view ===

use crate::tui::theme;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use zos_core::commands::grub;

#[derive(Debug, PartialEq)]
enum GrubMode {
    Overview,
    SetTimeout,
    Confirm(ConfirmAction),
}

#[derive(Debug, PartialEq, Clone)]
enum ConfirmAction {
    Timeout(u32),
    WindowsBls,
}

#[derive(Debug)]
pub struct GrubView {
    status: grub::GrubStatus,
    mode: GrubMode,
    timeout_input: String,
    message: Option<(String, bool)>,
}

impl GrubView {
    pub fn new() -> Self {
        Self {
            status: grub::get_grub_status(),
            mode: GrubMode::Overview,
            timeout_input: String::new(),
            message: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match &self.mode {
            GrubMode::Overview => match key.code {
                KeyCode::Char('t') => {
                    if !grub::is_root() {
                        self.message = Some(("Requires root. Run: sudo zos grub".into(), true));
                    } else {
                        self.timeout_input.clear();
                        self.mode = GrubMode::SetTimeout;
                    }
                }
                KeyCode::Char('w') => {
                    if !grub::is_root() {
                        self.message = Some(("Requires root. Run: sudo zos grub".into(), true));
                    } else if !self.status.windows_detected {
                        self.message = Some(("No Windows installation detected.".into(), true));
                    } else {
                        self.mode = GrubMode::Confirm(ConfirmAction::WindowsBls);
                    }
                }
                _ => {}
            },
            GrubMode::SetTimeout => match key.code {
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    self.timeout_input.push(c);
                }
                KeyCode::Backspace => {
                    self.timeout_input.pop();
                }
                KeyCode::Enter => {
                    if let Ok(secs) = self.timeout_input.parse::<u32>() {
                        self.mode = GrubMode::Confirm(ConfirmAction::Timeout(secs));
                    } else {
                        self.message = Some(("Invalid number.".into(), true));
                        self.mode = GrubMode::Overview;
                    }
                }
                KeyCode::Esc => {
                    self.mode = GrubMode::Overview;
                }
                _ => {}
            },
            GrubMode::Confirm(action) => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    let action = action.clone();
                    self.execute_action(&action);
                    self.mode = GrubMode::Overview;
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.mode = GrubMode::Overview;
                    self.message = Some(("Cancelled.".into(), false));
                }
                _ => {}
            },
        }
    }

    /// Return true if this view is in a sub-mode that should consume Esc.
    pub fn is_in_submode(&self) -> bool {
        self.mode != GrubMode::Overview
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(9), // Status
                Constraint::Min(4),    // Action area
                Constraint::Length(4), // Message
                Constraint::Length(3), // Keybinds
            ])
            .split(area);

        self.render_status(frame, chunks[0]);
        self.render_action(frame, chunks[1]);
        self.render_message(frame, chunks[2]);
        self.render_keybinds(frame, chunks[3]);
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let timeout_str = self
            .status
            .current_timeout
            .map(|t| format!("{}s", t))
            .unwrap_or_else(|| "not set".into());

        let windows_str = if self.status.windows_detected {
            self.status.windows_path.as_deref().unwrap_or("detected")
        } else {
            "not detected"
        };

        let bls_str = if self.status.bls_entry_exists {
            "exists"
        } else {
            "not created"
        };

        let lines = vec![
            Line::from(vec![
                Span::styled("  GRUB Timeout:    ", theme::subtext_style()),
                Span::styled(timeout_str, theme::text_style()),
            ]),
            Line::from(vec![
                Span::styled("  Windows:         ", theme::subtext_style()),
                Span::styled(windows_str, theme::text_style()),
            ]),
            Line::from(vec![
                Span::styled("  BLS Entry:       ", theme::subtext_style()),
                Span::styled(bls_str, theme::text_style()),
            ]),
            Line::from(vec![
                Span::styled("  Running as root: ", theme::subtext_style()),
                if grub::is_root() {
                    Span::styled("yes", theme::pass_style())
                } else {
                    Span::styled("no", theme::warn_style())
                },
            ]),
        ];

        let block = Block::default()
            .title(Span::styled(" GRUB Configuration ", theme::title_style()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::SURFACE0))
            .style(Style::default().bg(theme::BASE));

        frame.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn render_action(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::SURFACE0))
            .style(Style::default().bg(theme::BASE));

        let content = match &self.mode {
            GrubMode::Overview => Paragraph::new(Line::from(Span::styled(
                "  Press 't' to set timeout, 'w' to add Windows boot entry.",
                theme::subtext_style(),
            ))),
            GrubMode::SetTimeout => Paragraph::new(vec![
                Line::from(Span::styled(
                    "  Enter timeout in seconds:",
                    theme::text_style(),
                )),
                Line::from(Span::styled(
                    format!("  > {}_", self.timeout_input),
                    theme::accent_style(),
                )),
            ]),
            GrubMode::Confirm(action) => {
                let desc = match action {
                    ConfirmAction::Timeout(s) => format!("Set GRUB timeout to {}s", s),
                    ConfirmAction::WindowsBls => "Create Windows BLS boot entry".into(),
                };
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        format!("  Confirm: {}?", desc),
                        theme::text_style(),
                    )),
                    Line::from(Span::styled("  [y] Yes  [n] No", theme::keybind_style())),
                ])
            }
        };

        frame.render_widget(content.block(block), area);
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
            None => Paragraph::new(""),
        };

        frame.render_widget(content.block(block), area);
    }

    fn render_keybinds(&self, frame: &mut Frame, area: Rect) {
        let hints = Line::from(vec![
            Span::styled(" [t]", theme::keybind_style()),
            Span::styled(" Timeout  ", theme::subtext_style()),
            Span::styled("[w]", theme::keybind_style()),
            Span::styled(" Windows Entry  ", theme::subtext_style()),
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

    fn execute_action(&mut self, action: &ConfirmAction) {
        match action {
            ConfirmAction::Timeout(secs) => match grub::apply_grub_timeout(*secs) {
                Ok(()) => {
                    self.status = grub::get_grub_status();
                    self.message = Some((format!("GRUB timeout set to {}s.", secs), false));
                }
                Err(e) => {
                    self.message = Some((format!("Error: {}", e), true));
                }
            },
            ConfirmAction::WindowsBls => match grub::create_windows_bls() {
                Ok(()) => {
                    self.status = grub::get_grub_status();
                    self.message = Some(("Windows BLS entry created.".into(), false));
                }
                Err(e) => {
                    self.message = Some((format!("Error: {}", e), true));
                }
            },
        }
    }
}
