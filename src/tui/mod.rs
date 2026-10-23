//! `kget tui`: full-screen terminal UI (ratatui + crossterm).
//!
//! - [`source`] — the [`JobSource`] trait the UI talks to, and the in-process
//!   [`LocalSource`] over a [`JobManager`](kget::jobs::JobManager)
//! - [`state`] — the model, the event reducer and the key map
//! - [`ui`] — rendering
//!
//! This module only owns the terminal and the event loop: it feeds job
//! events and input into the model and runs the resulting actions.

mod source;
mod state;
mod ui;

use kget::config::Config;
use kget::jobs::JobManager;
use kget::runner::DownloadJob;
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyEventKind, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Position, Rect};
use std::io::{self, Stdout};
use std::time::{Duration, Instant};

pub use source::{DaemonSource, JobSource, LocalSource};
use state::{Action, App, Focus, Tab};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// How often the screen is redrawn and events are drained.
const TICK: Duration = Duration::from_millis(200);

/// Where `kget tui` runs downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The running daemon if there is one, else this process
    Auto,
    /// Always this process (`--local`)
    Local,
    /// The daemon, started if needed (`--daemon`)
    Daemon,
}

/// Run the terminal UI.
pub fn run(config: Config, mode: Mode) -> Result<(), BoxError> {
    let daemon = match mode {
        Mode::Local => None,
        Mode::Auto => DaemonSource::connect(false).ok(),
        Mode::Daemon => Some(DaemonSource::connect(true)?),
    };
    match daemon {
        Some(mut source) => run_with_source(&mut source),
        None => {
            let manager = JobManager::new(config);
            let mut source = LocalSource::new(manager);
            run_with_source(&mut source)
        }
    }
}

/// Run the terminal UI against any job source (in-process, daemon…).
pub fn run_with_source(source: &mut dyn JobSource) -> Result<(), BoxError> {
    install_panic_hook();
    let mut terminal = TerminalGuard::enter()?;
    let result = event_loop(terminal.terminal(), source);
    drop(terminal);
    source.shutdown();
    result
}

// ----------------------------------------------------------------------------
// Terminal setup and restore
// ----------------------------------------------------------------------------

/// Raw mode + alternate screen + mouse capture, undone on drop (normal exit
/// and `?` errors). Panics are covered by [`install_panic_hook`].
struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn enter() -> Result<Self, BoxError> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(e) = execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        ) {
            restore_terminal();
            return Err(e.into());
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(e) => {
                restore_terminal();
                Err(e.into())
            }
        }
    }

    fn terminal(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
        let _ = self.terminal.show_cursor();
    }
}

/// Leave raw mode and the alternate screen (safe to call more than once).
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen,
        ratatui::crossterm::cursor::Show
    );
}

/// Restore the terminal before the default panic message is printed, so it
/// is readable and the shell is usable afterwards.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
}

// ----------------------------------------------------------------------------
// Event loop
// ----------------------------------------------------------------------------

fn event_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    source: &mut dyn JobSource,
) -> Result<(), BoxError>
where
    B::Error: Send + Sync + 'static,
{
    let mut app = App::new(source.label());
    app.sync(&source.snapshot());
    let mut last_sample = Instant::now();

    loop {
        for ev in source.poll_events() {
            let finished = matches!(
                &ev,
                kget::jobs::JobEvent::State { state, .. } if state.is_finished()
            );
            app.apply(ev);
            if finished && app.tab == Tab::History {
                run_action(&mut app, source, Action::LoadHistory);
            }
        }
        if app.needs_sync {
            app.sync(&source.snapshot());
        }
        if last_sample.elapsed() >= Duration::from_secs(1) {
            app.sample_speed();
            last_sample = Instant::now();
        }

        terminal.draw(|f| ui::render(f, &app))?;

        if !event::poll(TICK)? {
            continue;
        }
        let actions = match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => app.on_key(key),
            Event::Mouse(m) => {
                let size = terminal.size()?;
                on_mouse(&mut app, m, Rect::new(0, 0, size.width, size.height))
            }
            Event::Paste(text) => {
                app.on_paste(&text);
                Vec::new()
            }
            _ => Vec::new(),
        };
        for action in actions {
            if action == Action::Quit {
                return Ok(());
            }
            run_action(&mut app, source, action);
        }
    }
}

/// Run one action against the source; failures go to the footer.
fn run_action(app: &mut App, source: &mut dyn JobSource, action: Action) {
    let result = match action {
        Action::Submit(url) | Action::Redownload(url) => {
            let mut job = DownloadJob::new(url);
            // Continue a `.part` left by an earlier attempt.
            job.resume = true;
            source.submit(job).map(|id| {
                app.tab = Tab::Queue;
                app.focus = Focus::List;
                app.sync(&source.snapshot());
                if let Some(pos) = app.jobs.iter().position(|j| j.id == id) {
                    app.selected = pos;
                }
                app.message = Some(format!("Queued job {id}"));
            })
        }
        Action::Pause(id) => source.pause(id),
        Action::Resume(id) => source.resume(id),
        Action::Cancel(id) => source.cancel(id),
        Action::Remove(id) => source.remove(id),
        Action::Reveal(path) => {
            crate::interactive::open_with_system(std::path::Path::new(&path), true)
                .map_err(|e| e.to_string())
        }
        Action::LoadHistory => source
            .history(&app.history_query())
            .map(|entries| app.set_history(entries)),
        Action::Quit => Ok(()),
    };
    if let Err(e) = result {
        app.message = Some(e);
    }
}

/// Click to select a row or a tab, wheel to scroll the list under the cursor.
fn on_mouse(app: &mut App, m: MouseEvent, screen: Rect) -> Vec<Action> {
    let areas = ui::layout(screen, app.tab);
    let pos = Position::new(m.column, m.row);
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if areas.title.contains(pos) {
                // " KGet  1 Queue | 2 History"
                let tab = match m.column {
                    7..=14 => Some(Tab::Queue),
                    16..=26 => Some(Tab::History),
                    _ => None,
                };
                if let Some(tab) = tab {
                    app.tab = tab;
                    app.focus = Focus::List;
                    if tab == Tab::History {
                        return vec![Action::LoadHistory];
                    }
                }
            } else if areas.list.contains(pos) {
                app.focus = Focus::List;
                // Below the top border and the header row.
                if let Some(row) = m.row.checked_sub(areas.list.y + 2) {
                    let visible = ui::visible_rows(areas.list);
                    let (selected, len) = match app.tab {
                        Tab::Queue => (app.selected, app.jobs.len()),
                        Tab::History => (app.history_selected, app.history.len()),
                    };
                    let idx = ui::scroll_offset(selected, visible) + row as usize;
                    if (row as usize) < visible && idx < len {
                        match app.tab {
                            Tab::Queue => {
                                if app.selected != idx {
                                    app.detail_scroll = 0;
                                }
                                app.selected = idx;
                            }
                            Tab::History => app.history_selected = idx,
                        }
                    }
                }
            } else if app.tab == Tab::Queue && areas.detail.contains(pos) {
                app.focus = Focus::Detail;
            }
        }
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            let delta = if m.kind == MouseEventKind::ScrollDown {
                1
            } else {
                -1
            };
            if app.tab == Tab::Queue && areas.detail.contains(pos) {
                app.focus = Focus::Detail;
            } else if areas.list.contains(pos) {
                app.focus = Focus::List;
            }
            app.move_selection(delta);
        }
        _ => {}
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kget::jobs::{DownloadState, JobEvent, JobId, JobInfo};
    use kget::queue::{HistoryEntry, HistoryQuery};
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;
    use std::cell::RefCell;

    /// In-memory source recording the commands it receives.
    #[derive(Default)]
    struct FakeSource {
        jobs: Vec<JobInfo>,
        events: Vec<JobEvent>,
        calls: RefCell<Vec<String>>,
    }

    impl JobSource for FakeSource {
        fn label(&self) -> String {
            "fake".into()
        }
        fn submit(&self, job: DownloadJob) -> Result<JobId, String> {
            self.calls.borrow_mut().push(format!("submit {}", job.url));
            Ok(1)
        }
        fn pause(&self, id: JobId) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("pause {id}"));
            Ok(())
        }
        fn resume(&self, id: JobId) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("resume {id}"));
            Ok(())
        }
        fn cancel(&self, id: JobId) -> Result<(), String> {
            Err(format!("cannot cancel {id}"))
        }
        fn remove(&self, _id: JobId) -> Result<(), String> {
            Ok(())
        }
        fn snapshot(&self) -> Vec<JobInfo> {
            self.jobs.clone()
        }
        fn poll_events(&mut self) -> Vec<JobEvent> {
            std::mem::take(&mut self.events)
        }
        fn history(&self, _q: &HistoryQuery) -> Result<Vec<HistoryEntry>, String> {
            Ok(vec![HistoryEntry::new("https://e.com/h.zip", "/tmp", None)])
        }
        fn shutdown(&mut self) {}
    }

    #[test]
    fn actions_reach_the_source() {
        let mut src = FakeSource {
            jobs: vec![state::tests::info(
                1,
                "https://e.com/a",
                DownloadState::Running,
            )],
            ..Default::default()
        };
        let mut app = App::new(src.label());
        app.sync(&src.snapshot());
        run_action(&mut app, &mut src, Action::Submit("https://e.com/a".into()));
        run_action(&mut app, &mut src, Action::Pause(1));
        run_action(&mut app, &mut src, Action::Cancel(1));
        assert_eq!(
            *src.calls.borrow(),
            vec!["submit https://e.com/a".to_string(), "pause 1".into()]
        );
        assert_eq!(app.message.as_deref(), Some("cannot cancel 1"));
        run_action(&mut app, &mut src, Action::LoadHistory);
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn mouse_selects_rows_and_tabs() {
        let mut app = App::new("t");
        app.sync(&[
            state::tests::info(1, "https://e.com/a", DownloadState::Queued),
            state::tests::info(2, "https://e.com/b", DownloadState::Queued),
        ]);
        let screen = Rect::new(0, 0, 80, 24);
        let click = |col, row| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        };
        // Row 0 is the title, 1 the border, 2 the header, 3 job 1, 4 job 2.
        on_mouse(&mut app, click(10, 4), screen);
        assert_eq!(app.selected, 1);
        assert_eq!(
            on_mouse(&mut app, click(18, 0), screen),
            vec![Action::LoadHistory]
        );
        assert_eq!(app.tab, Tab::History);
    }

    #[test]
    fn loop_quits_on_q_without_active_jobs() {
        // The event loop itself needs a real terminal for input; check that
        // a draw with the test backend works with an empty source.
        let src = FakeSource::default();
        let mut app = App::new(src.label());
        app.sync(&src.snapshot());
        let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
        term.draw(|f| ui::render(f, &app)).unwrap();
        let key = event::KeyEvent::new(event::KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(app.on_key(key), vec![Action::Quit]);
    }
}
