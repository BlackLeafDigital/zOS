// === tui/doctor.rs — Doctor diagnostics view ===

use crate::tui::theme;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use zos_core::commands::doctor::{self, CheckStatus, DoctorCheck};

#[derive(Debug)]
pub struct DoctorView {
    checks: Vec<DoctorCheck>,
    running: bool,
    tick: u16,
}

impl DoctorView {
    pub fn new() -> Self {
        Self {
            checks: Vec::new(),
            running: true,
            tick: 0,
        }
    }

    /// Called on each tick to advance spinner / run checks.
    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);

        // Run checks on first tick (simulates async loading)
        if self.running && self.tick >= 2 {
            self.checks = doctor::run_doctor_checks();
            self.running = false;
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),    // Check results
                Constraint::Length(3), // Summary
                Constraint::Length(3), // Keybinds
            ])
            .split(area);

        self.render_checks(frame, chunks[0]);
        self.render_summary(frame, chunks[1]);
        self.render_keybinds(frame, chunks[2]);
    }

    fn render_checks(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(" System Doctor ", theme::title_style()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::SURFACE0))
            .style(Style::default().bg(theme::BASE));

        if self.running {
            let spinner_chars = ['|', '/', '-', '\\'];
            let spinner = spinner_chars[(self.tick as usize) % spinner_chars.len()];
            let text = Paragraph::new(Line::from(vec![Span::styled(
                format!("  {} Running diagnostics...", spinner),
                theme::accent_style(),
            )]))
            .block(block);
            frame.render_widget(text, area);
            return;
        }

        let items: Vec<ListItem> = self
            .checks
            .iter()
            .map(|check| {
                let (icon, style) = match check.status {
                    CheckStatus::Pass => ("[PASS]", theme::pass_style()),
                    CheckStatus::Fail => ("[FAIL]", theme::fail_style()),
                    CheckStatus::Warn => ("[WARN]", theme::warn_style()),
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {} ", icon), style),
                    Span::styled(format!("{:<25}", check.name), theme::text_style()),
                    Span::styled(&check.message, theme::subtext_style()),
                ]))
            })
            .collect();

        let list = List::new(items).block(block);
        frame.render_widget(list, area);
    }

    fn render_summary(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::SURFACE0))
            .style(Style::default().bg(theme::BASE));

        if self.running {
            let text = Paragraph::new(Line::from(Span::styled(
                "  Scanning...",
                theme::subtext_style(),
            )))
            .block(block);
            frame.render_widget(text, area);
            return;
        }

        let (pass, fail, warn) = doctor::summarize(&self.checks);

        let summary = Line::from(vec![
            Span::styled("  ", theme::text_style()),
            Span::styled(format!("{} passed", pass), theme::pass_style()),
            Span::styled("  |  ", theme::subtext_style()),
            Span::styled(format!("{} failed", fail), theme::fail_style()),
            Span::styled("  |  ", theme::subtext_style()),
            Span::styled(format!("{} warnings", warn), theme::warn_style()),
        ]);

        frame.render_widget(Paragraph::new(summary).block(block), area);
    }

    fn render_keybinds(&self, frame: &mut Frame, area: Rect) {
        let hints = Line::from(vec![
            Span::styled(" [Esc]", theme::keybind_style()),
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
}
