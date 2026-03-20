mod app;
mod history;
mod input;
mod palette;
mod slurm;
mod ui;

use std::io;
use std::process::Command;
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

const DEFAULT_REFRESH_SECS: f64 = 1.0;

#[derive(Parser)]
#[command(name = "slurm-tui", about = "Slurm TUI monitor")]
struct Cli {
    /// Use dark background theme (default: light)
    #[arg(long)]
    dark: bool,
    /// SSH host for resolving UIDs to usernames
    #[arg(long = "login-node", value_name = "HOST")]
    login_node: Option<String>,
    /// Refresh interval in seconds (default: 1)
    #[arg(short = 'n', value_name = "SECS", default_value_t = DEFAULT_REFRESH_SECS)]
    refresh: f64,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    let mut app = App::new(cli.dark, cli.login_node.unwrap_or_default());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app, Duration::from_secs_f64(cli.refresh.max(0.1)));

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    app.history.save();

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }
    Ok(())
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App, refresh: Duration) -> io::Result<()> {
    let mut last_fetch = Instant::now() - refresh; // force immediate fetch
    let mut needs_draw = true;

    loop {
        // Check timed message dismissal
        if app.check_message_timeout() {
            needs_draw = true;
        }

        // Fetch data periodically when no modal is active (or showing a timed message)
        if app.modal.is_none() || app.has_timed_message() {
            if last_fetch.elapsed() >= refresh {
                let size = terminal.size()?;
                app.calc_max_jobs(size.height);
                app.fetch_data();
                last_fetch = Instant::now();
                needs_draw = true;
            }
        }

        if needs_draw {
            terminal.draw(|f| ui::draw(f, app))?;
            needs_draw = false;
        }

        if let Some(path) = app.pending_less_path.take() {
            open_in_less(terminal, &path)?;
            terminal.clear()?;
            needs_draw = true;
            last_fetch = Instant::now() - refresh;
            continue;
        }

        if app.should_quit {
            break;
        }

        // Sleep until next event or next refresh, whichever comes first
        let until_refresh = refresh.saturating_sub(last_fetch.elapsed());
        let poll_timeout = if app.has_timed_message() {
            // Check message timeout frequently
            until_refresh.min(Duration::from_millis(100))
        } else {
            until_refresh.min(Duration::from_secs(1))
        };

        if event::poll(poll_timeout)? {
            if let Event::Key(key) = event::read()? {
                needs_draw = true;
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
        KeyCode::Char('l') | KeyCode::Char('L') => app.open_logs(),
        _ => {}
    }
}

fn open_in_less(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, path: &str) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let status = Command::new("less").arg(path).status();

    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.hide_cursor()?;

    match status {
        Ok(_) => Ok(()),
        Err(err) => Err(io::Error::new(io::ErrorKind::Other, format!("failed to launch less: {err}"))),
    }
}

fn handle_modal_key(app: &mut App, key: KeyEvent) {
    let kind = app.modal.as_ref().map(|m| m.kind);
    let enables_tab = matches!(kind, Some(ModalKind::Submit) | Some(ModalKind::Chdir));
    let has_completions = app.modal.as_ref().map_or(false, |m| !m.completions.is_empty());

    match key.code {
        KeyCode::Esc => app.dismiss_modal(),
        KeyCode::Enter => {
            match kind {
                Some(ModalKind::Logs) => {
                    let job_id = app
                        .modal
                        .as_ref()
                        .and_then(|m| m.body_lines.get(m.selection))
                        .and_then(|(line, _)| line.split_whitespace().next())
                        .map(str::to_string);
                    if let Some(job_id) = job_id {
                        app.open_log_view(&job_id);
                    }
                }
<<<<<<< HEAD
                Some(ModalKind::Cancel) => {
                    let job_id = app
                        .modal
                        .as_ref()
                        .and_then(|m| m.body_lines.get(m.selection))
                        .and_then(|(line, _)| line.split_whitespace().next())
                        .map(str::to_string);
                    if let Some(job_id) = job_id {
                        app.select_cancel_job(&job_id);
                    }
                }
=======
>>>>>>> ec6b6fc (Use less for historical logs)
                _ => {
                    if has_completions {
                        if let Some(ref mut modal) = app.modal {
                            modal.clear_completions();
                        }
                    } else {
                        app.handle_modal_submit();
                    }
                }
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
        KeyCode::PageUp => {}
        KeyCode::PageDown => {}
        KeyCode::Up => {
            if let Some(ref mut m) = app.modal {
                match m.kind {
<<<<<<< HEAD
                    ModalKind::Logs | ModalKind::Cancel => {
=======
                    ModalKind::Logs => {
>>>>>>> ec6b6fc (Use less for historical logs)
                        if m.selection > 1 {
                            m.selection -= 1;
                        }
                    }
                    _ if has_completions => {
                        let len = m.completions.len() as isize;
                        m.comp_index = (m.comp_index - 1).rem_euclid(len);
                        let idx = m.comp_index as usize;
                        m.buf = format!("{}{}{}", m.comp_prefix, m.completions[idx], m.comp_suffix);
                        m.cursor = m.comp_prefix.len() + m.completions[idx].len();
                    }
                    _ => app.history_up(),
                }
            }
        }
        KeyCode::Down => {
            if let Some(ref mut m) = app.modal {
                match m.kind {
<<<<<<< HEAD
                    ModalKind::Logs | ModalKind::Cancel => {
=======
                    ModalKind::Logs => {
>>>>>>> ec6b6fc (Use less for historical logs)
                        let max_selection = m.body_lines.len().saturating_sub(1);
                        if m.selection < max_selection {
                            m.selection += 1;
                        }
                    }
                    _ if has_completions => {
                        let len = m.completions.len() as isize;
                        m.comp_index = (m.comp_index + 1).rem_euclid(len);
                        let idx = m.comp_index as usize;
                        m.buf = format!("{}{}{}", m.comp_prefix, m.completions[idx], m.comp_suffix);
                        m.cursor = m.comp_prefix.len() + m.completions[idx].len();
                    }
                    _ => app.history_down(),
                }
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
            } else if matches!(kind, Some(ModalKind::Submit) | Some(ModalKind::Cancel) | Some(ModalKind::CancelConfirm) | Some(ModalKind::Chdir)) {
                if let Some(ref mut m) = app.modal {
                    m.insert_char(c);
                }
            }
        }
        _ => {}
    }
}
