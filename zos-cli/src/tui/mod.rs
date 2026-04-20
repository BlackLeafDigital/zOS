// === tui/mod.rs — Terminal setup, event loop, and view dispatch ===

pub mod dashboard;
pub mod doctor;
pub mod grub;
pub mod migrate;
pub mod setup;
#[allow(dead_code)]
pub mod theme;

use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::io::stdout;
use std::time::Duration;

// --- View enum ---
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum View {
    Dashboard,
    Migrate,
    Doctor,
    Grub,
    Setup,
}

// --- App state ---
pub struct App {
    current_view: View,
    running: bool,
    // View-specific state
    migrate_view: Option<migrate::MigrateView>,
    doctor_view: Option<doctor::DoctorView>,
    grub_view: Option<grub::GrubView>,
    setup_view: Option<setup::SetupView>,
}

impl App {
    pub fn new(initial_view: View) -> Self {
        Self {
            current_view: initial_view,
            running: true,
            migrate_view: None,
            doctor_view: None,
            grub_view: None,
            setup_view: None,
        }
    }

    fn switch_view(&mut self, view: View) {
        self.current_view = view;
        // Initialize view state on switch
        match view {
            View::Migrate => {
                self.migrate_view = Some(migrate::MigrateView::new());
            }
            View::Doctor => {
                self.doctor_view = Some(doctor::DoctorView::new());
            }
            View::Grub => {
                self.grub_view = Some(grub::GrubView::new());
            }
            View::Setup => {
                self.setup_view = Some(setup::SetupView::new());
            }
            View::Dashboard => {}
        }
    }
}

/// Initialize terminal, run event loop, restore terminal on exit.
pub fn run(initial_view: View) -> Result<()> {
    // Setup
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new(initial_view);
    // Initialize the starting view
    app.switch_view(initial_view);

    let result = event_loop(&mut terminal, &mut app);

    // Teardown — always runs
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    while app.running {
        // Draw
        terminal.draw(|frame| render(frame, app))?;

        // Tick for animated views
        if app.current_view == View::Doctor {
            if let Some(ref mut dv) = app.doctor_view {
                dv.tick();
            }
        }

        // Poll events with a timeout for tick-based updates
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Only handle key press events (not release/repeat)
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Global: 'q' quits from any view
                if key.code == KeyCode::Char('q') {
                    app.running = false;
                    continue;
                }

                // Global: Esc goes back to dashboard (unless a sub-view consumes it)
                if key.code == KeyCode::Esc {
                    let consumed = match app.current_view {
                        View::Grub => {
                            if let Some(ref gv) = app.grub_view {
                                gv.is_in_submode()
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };

                    if consumed {
                        // Let the sub-view handle Esc
                        dispatch_key(app, key);
                    } else if app.current_view != View::Dashboard {
                        app.switch_view(View::Dashboard);
                    }
                    continue;
                }

                // Dispatch to current view
                dispatch_key(app, key);
            }
        }
    }

    Ok(())
}

fn dispatch_key(app: &mut App, key: crossterm::event::KeyEvent) {
    match app.current_view {
        View::Dashboard => match key.code {
            KeyCode::Char('m') => app.switch_view(View::Migrate),
            KeyCode::Char('d') => app.switch_view(View::Doctor),
            KeyCode::Char('g') => app.switch_view(View::Grub),
            KeyCode::Char('s') => app.switch_view(View::Setup),
            _ => {}
        },
        View::Migrate => {
            if let Some(ref mut mv) = app.migrate_view {
                mv.handle_key(key);
            }
        }
        View::Doctor => {
            // Doctor view is read-only, no key handling beyond global
        }
        View::Grub => {
            if let Some(ref mut gv) = app.grub_view {
                gv.handle_key(key);
            }
        }
        View::Setup => {
            if let Some(ref mut sv) = app.setup_view {
                sv.handle_key(key);
            }
        }
    }
}

fn render(frame: &mut Frame, app: &mut App) {
    // Fill background
    let size = frame.area();
    let bg = Block::default().style(Style::default().bg(theme::BASE));
    frame.render_widget(bg, size);

    // Title bar + content area
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title bar
            Constraint::Min(10),   // Content
        ])
        .split(size);

    render_title_bar(frame, outer[0], app.current_view);

    match app.current_view {
        View::Dashboard => dashboard::render(frame, outer[1]),
        View::Migrate => {
            if let Some(ref mut mv) = app.migrate_view {
                mv.render(frame, outer[1]);
            }
        }
        View::Doctor => {
            if let Some(ref dv) = app.doctor_view {
                dv.render(frame, outer[1]);
            }
        }
        View::Grub => {
            if let Some(ref gv) = app.grub_view {
                gv.render(frame, outer[1]);
            }
        }
        View::Setup => {
            if let Some(ref mut sv) = app.setup_view {
                sv.render(frame, outer[1]);
            }
        }
    }
}

fn render_title_bar(frame: &mut Frame, area: Rect, current: View) {
    let view_name = match current {
        View::Dashboard => "Dashboard",
        View::Migrate => "Config Migration",
        View::Doctor => "System Doctor",
        View::Grub => "GRUB Configuration",
        View::Setup => "First-Login Setup",
    };

    let title = Line::from(vec![
        Span::styled(" zos-system ", theme::title_style()),
        Span::styled("| ", theme::subtext_style()),
        Span::styled(view_name, theme::accent_style()),
    ]);

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(theme::SURFACE0))
        .style(Style::default().bg(theme::MANTLE));

    frame.render_widget(Paragraph::new(title).block(block), area);
}
