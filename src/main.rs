mod app;
mod history;
mod input;
mod palette;
mod slurm;
mod ui;

use std::io;
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::App;
use input::ModalKind;

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Parser)]
#[command(name = "slurm-tui", about = "Slurm TUI monitor")]
struct Cli {
    /// Use dark background theme (default: light)
    #[arg(long)]
    dark: bool,
    /// SSH host for resolving UIDs to usernames
    #[arg(long = "login-node", value_name = "HOST")]
    login_node: Option<String>,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    let mut app = App::new(cli.dark, cli.login_node.unwrap_or_default());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    app.history.save();

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }
    Ok(())
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    let mut last_fetch = Instant::now() - REFRESH_INTERVAL; // force immediate fetch
    let mut had_modal = false;

    loop {
        // Check timed message dismissal
        app.check_message_timeout();

        // Fetch data periodically when no modal is active (or showing a timed message)
        if app.modal.is_none() || app.has_timed_message() {
            if last_fetch.elapsed() >= REFRESH_INTERVAL {
                let size = terminal.size()?;
                app.calc_max_jobs(size.height);
                app.fetch_data();
                last_fetch = Instant::now();
            }
        }

        // Force full redraw when modal state changes (open/close) to prevent
        // stale content from ratatui's double-buffered back buffer bleeding through.
        let has_modal = app.modal.is_some();
        if has_modal != had_modal {
            terminal.clear()?;
            had_modal = has_modal;
        }

        terminal.draw(|f| ui::draw(f, app))?;

        if app.should_quit {
            break;
        }

        // Poll for events with short timeout for responsive UI
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if app.has_timed_message() {
                    // Any key dismisses a timed message
                    app.dismiss_modal();
                    continue;
                }

                if app.modal.is_some() {
                    handle_modal_key(app, key);
                } else {
                    handle_normal_key(app, key);
                }
            }
        }
    }
    Ok(())
}

fn handle_normal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => app.should_quit = true,
        KeyCode::Char('s') | KeyCode::Char('S') => app.open_submit(),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => app.should_quit = true,
        KeyCode::Char('c') | KeyCode::Char('C') => app.open_cancel(),
        KeyCode::Char('d') | KeyCode::Char('D') => app.open_chdir(),
        _ => {}
    }
}

fn handle_modal_key(app: &mut App, key: KeyEvent) {
    let enables_tab = matches!(
        app.modal.as_ref().map(|m| m.kind),
        Some(ModalKind::Submit) | Some(ModalKind::Chdir)
    );
    let has_completions = app.modal.as_ref().map_or(false, |m| !m.completions.is_empty());

    match key.code {
        KeyCode::Esc => app.dismiss_modal(),
        KeyCode::Enter => {
            if has_completions {
                if let Some(ref mut modal) = app.modal {
                    modal.clear_completions();
                }
            } else {
                app.handle_modal_submit();
            }
        }
        KeyCode::Backspace => {
            if let Some(ref mut m) = app.modal {
                m.backspace();
            }
        }
        KeyCode::Delete => {
            if let Some(ref mut m) = app.modal {
                m.delete_char();
            }
        }
        KeyCode::Left => {
            if let Some(ref mut m) = app.modal {
                m.move_left();
            }
        }
        KeyCode::Right => {
            if let Some(ref mut m) = app.modal {
                m.move_right();
            }
        }
        KeyCode::Home => {
            if let Some(ref mut m) = app.modal {
                m.home();
            }
        }
        KeyCode::End => {
            if let Some(ref mut m) = app.modal {
                m.end();
            }
        }
        KeyCode::Up => {
            if has_completions {
                if let Some(ref mut m) = app.modal {
                    let len = m.completions.len() as isize;
                    m.comp_index = (m.comp_index - 1).rem_euclid(len);
                    let idx = m.comp_index as usize;
                    m.buf = format!("{}{}{}", m.comp_prefix, m.completions[idx], m.comp_suffix);
                    m.cursor = m.comp_prefix.len() + m.completions[idx].len();
                }
            } else {
                app.history_up();
            }
        }
        KeyCode::Down => {
            if has_completions {
                if let Some(ref mut m) = app.modal {
                    let len = m.completions.len() as isize;
                    m.comp_index = (m.comp_index + 1).rem_euclid(len);
                    let idx = m.comp_index as usize;
                    m.buf = format!("{}{}{}", m.comp_prefix, m.completions[idx], m.comp_suffix);
                    m.cursor = m.comp_prefix.len() + m.completions[idx].len();
                }
            } else {
                app.history_down();
            }
        }
        KeyCode::Tab if enables_tab => {
            let cwd = app.cwd.clone();
            if let Some(ref mut m) = app.modal {
                input::handle_tab(m, &cwd);
            }
        }
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match c {
                    'a' => { if let Some(ref mut m) = app.modal { m.home(); } }
                    'e' => { if let Some(ref mut m) = app.modal { m.end(); } }
                    'b' => { if let Some(ref mut m) = app.modal { m.move_left(); } }
                    'f' => { if let Some(ref mut m) = app.modal { m.move_right(); } }
                    'd' => { if let Some(ref mut m) = app.modal { m.delete_char(); } }
                    'k' => { if let Some(ref mut m) = app.modal { m.kill_to_end(); } }
                    'u' => { if let Some(ref mut m) = app.modal { m.kill_to_start(); } }
                    'w' => { if let Some(ref mut m) = app.modal { m.kill_word(); } }
                    'p' => { if !has_completions { app.history_up(); } }
                    'n' => { if !has_completions { app.history_down(); } }
                    _ => {}
                }
            } else {
                if let Some(ref mut m) = app.modal {
                    m.insert_char(c);
                }
            }
        }
        _ => {}
    }
}
