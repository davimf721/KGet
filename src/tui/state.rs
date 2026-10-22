//! TUI model: the job list built from [`JobEvent`]s, the UI state, and the
//! key handling that turns input into [`Action`]s.
//!
//! Nothing here touches the terminal or the job source, so the reducer and
//! the key map are unit tested directly.

use kget::events::{JsonEvent, TorrentStats};
use kget::jobs::{DownloadState, JobEvent, JobId, JobInfo};
use kget::queue::{EntryStatus, HistoryEntry, HistoryQuery};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::VecDeque;

/// Samples kept for the aggregate speed sparkline.
pub const SPEED_SAMPLES: usize = 120;

/// Maximum history entries loaded at once.
pub const HISTORY_LIMIT: usize = 500;

/// One file of a torrent job.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileRow {
    pub name: String,
    pub size: u64,
    pub downloaded: u64,
    /// `Some(false)` when excluded by `--select` / `--select-glob`
    pub selected: Option<bool>,
}

/// One job of the queue.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    pub id: JobId,
    pub url: String,
    pub name: String,
    pub protocol: String,
    pub advanced: bool,
    pub state: DownloadState,
    pub bytes: u64,
    pub total: Option<u64>,
    pub percent: Option<f64>,
    pub speed_bps: u64,
    pub eta_secs: Option<u64>,
    /// Requested output (file or folder)
    pub output: Option<String>,
    /// Where the data was saved
    pub path: Option<String>,
    pub error: Option<String>,
    /// Latest status message from the engine
    pub status: Option<String>,
    pub files: Vec<FileRow>,
    pub torrent: Option<TorrentStats>,
}

impl JobRow {
    pub fn from_info(info: &JobInfo) -> Self {
        Self {
            id: info.id,
            url: info.job.url.clone(),
            name: display_name(&info.job.url),
            protocol: info.protocol.as_str().to_string(),
            advanced: info.job.advanced,
            state: info.state,
            bytes: info.progress.bytes,
            total: info.progress.total,
            percent: info.progress.percent,
            speed_bps: info.progress.speed_bps,
            eta_secs: info.progress.eta_secs,
            output: info.job.output.clone(),
            path: info.path.clone(),
            error: info.error.clone(),
            status: None,
            files: Vec::new(),
            torrent: info.progress.torrent.clone(),
        }
    }

    /// Path to open or reveal: the saved file, else the output.
    pub fn open_target(&self) -> Option<String> {
        self.path.clone().or_else(|| self.output.clone())
    }
}

/// Short name for a URL or magnet link.
pub fn display_name(url: &str) -> String {
    if url.to_ascii_lowercase().starts_with("magnet:") {
        if let Some(dn) = url
            .split(['?', '&'])
            .find_map(|kv| kv.strip_prefix("dn="))
            .filter(|dn| !dn.is_empty())
        {
            let dn = dn.replace('+', " ");
            return urlencoding::decode(&dn)
                .map(|c| c.into_owned())
                .unwrap_or(dn);
        }
        return "magnet link".into();
    }
    match kget::utils::get_filename_from_url_or_default(url, "") {
        n if !n.is_empty() => n,
        _ => url.to_string(),
    }
}

/// Top-level view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Queue,
    History,
}

/// Pane receiving the arrow keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// Queue list (Queue tab) or history list (History tab)
    List,
    /// Detail pane (Queue tab): scrolls the file list
    Detail,
}

/// Modal input state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// Typing a URL to add
    AddUrl(String),
    /// Typing a history search
    Search(String),
    Help,
    /// `q` with active downloads: waiting for confirmation
    ConfirmQuit,
}

/// Something the event loop must do against the job source or the system.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Submit(String),
    Pause(JobId),
    Resume(JobId),
    Cancel(JobId),
    Remove(JobId),
    /// Download a history entry's URL again
    Redownload(String),
    /// Reveal this path in the file manager
    Reveal(String),
    /// Reload the history list
    LoadHistory,
    Quit,
}

/// The whole UI model.
#[derive(Debug, Clone)]
pub struct App {
    pub source_label: String,
    pub jobs: Vec<JobRow>,
    pub tab: Tab,
    pub focus: Focus,
    pub mode: Mode,
    /// Selected row in the queue
    pub selected: usize,
    /// Scroll offset of the detail pane's file list
    pub detail_scroll: usize,
    pub history: Vec<HistoryEntry>,
    pub history_selected: usize,
    pub history_search: String,
    pub history_status: Option<EntryStatus>,
    /// Aggregate download speed, oldest first
    pub speed: VecDeque<u64>,
    /// One-line message in the footer (errors, confirmations)
    pub message: Option<String>,
    /// An event mentioned a job missing from `jobs`
    pub needs_sync: bool,
}

impl App {
    pub fn new(source_label: impl Into<String>) -> Self {
        Self {
            source_label: source_label.into(),
            jobs: Vec::new(),
            tab: Tab::Queue,
            focus: Focus::List,
            mode: Mode::Normal,
            selected: 0,
            detail_scroll: 0,
            history: Vec::new(),
            history_selected: 0,
            history_search: String::new(),
            history_status: None,
            speed: VecDeque::with_capacity(SPEED_SAMPLES),
            message: None,
            needs_sync: false,
        }
    }

    // ------------------------------------------------------------------
    // Reducer
    // ------------------------------------------------------------------

    /// Replace the job list with snapshots (start-up, unknown job ids),
    /// keeping what only events carry (status, files).
    pub fn sync(&mut self, infos: &[JobInfo]) {
        let mut rows: Vec<JobRow> = infos.iter().map(JobRow::from_info).collect();
        for row in &mut rows {
            if let Some(old) = self.jobs.iter().find(|j| j.id == row.id) {
                row.status = old.status.clone();
                row.files = old.files.clone();
                row.path = row.path.clone().or_else(|| old.path.clone());
                row.torrent = row.torrent.clone().or_else(|| old.torrent.clone());
            }
        }
        self.jobs = rows;
        self.needs_sync = false;
        self.clamp_selection();
    }

    /// Apply one job event to the model.
    pub fn apply(&mut self, event: JobEvent) {
        let id = event.id();
        if let JobEvent::Removed { .. } = event {
            self.jobs.retain(|j| j.id != id);
            self.clamp_selection();
            return;
        }
        let Some(row) = self.jobs.iter_mut().find(|j| j.id == id) else {
            self.needs_sync = true;
            return;
        };
        match event {
            JobEvent::State { state, error, .. } => {
                row.state = state;
                if state == DownloadState::Running {
                    row.error = None;
                    row.speed_bps = 0;
                }
                if state.is_finished() {
                    row.speed_bps = 0;
                    row.eta_secs = None;
                }
                if state == DownloadState::Completed {
                    row.percent = Some(100.0);
                }
                if error.is_some() {
                    row.error = error;
                }
            }
            JobEvent::Download { event, .. } => apply_download(row, event),
            JobEvent::Removed { .. } => {}
        }
    }

    /// Record one sample of the aggregate speed (call about once a second).
    pub fn sample_speed(&mut self) {
        let total: u64 = self
            .jobs
            .iter()
            .filter(|j| j.state.is_active())
            .map(|j| j.speed_bps)
            .sum();
        if self.speed.len() == SPEED_SAMPLES {
            self.speed.pop_front();
        }
        self.speed.push_back(total);
    }

    /// Current aggregate download speed.
    pub fn total_speed(&self) -> u64 {
        self.jobs
            .iter()
            .filter(|j| j.state.is_active())
            .map(|j| j.speed_bps)
            .sum()
    }

    pub fn active_count(&self) -> usize {
        self.jobs.iter().filter(|j| !j.state.is_finished()).count()
    }

    pub fn selected_job(&self) -> Option<&JobRow> {
        self.jobs.get(self.selected)
    }

    pub fn selected_history(&self) -> Option<&HistoryEntry> {
        self.history.get(self.history_selected)
    }

    pub fn set_history(&mut self, entries: Vec<HistoryEntry>) {
        self.history = entries;
        self.history_selected = self
            .history_selected
            .min(self.history.len().saturating_sub(1));
    }

    /// The history query for the current search and status filter.
    pub fn history_query(&self) -> HistoryQuery {
        let mut q = HistoryQuery::new().limit(HISTORY_LIMIT);
        if !self.history_search.trim().is_empty() {
            q = q.search(self.history_search.trim());
        }
        if let Some(s) = &self.history_status {
            q = q.status(s.clone());
        }
        q
    }

    fn clamp_selection(&mut self) {
        self.selected = self.selected.min(self.jobs.len().saturating_sub(1));
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    /// Handle a key press; returns the actions to run.
    pub fn on_key(&mut self, key: KeyEvent) -> Vec<Action> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return vec![Action::Quit];
        }
        match self.mode.clone() {
            Mode::Help => {
                self.mode = Mode::Normal;
                Vec::new()
            }
            Mode::ConfirmQuit => match key.code {
                KeyCode::Char('q') | KeyCode::Char('y') | KeyCode::Enter => vec![Action::Quit],
                _ => {
                    self.mode = Mode::Normal;
                    self.message = None;
                    Vec::new()
                }
            },
            Mode::AddUrl(mut text) => match key.code {
                KeyCode::Esc => {
                    self.mode = Mode::Normal;
                    Vec::new()
                }
                KeyCode::Enter => {
                    self.mode = Mode::Normal;
                    let url = text.trim().to_string();
                    if url.is_empty() {
                        Vec::new()
                    } else {
                        vec![Action::Submit(url)]
                    }
                }
                KeyCode::Backspace => {
                    text.pop();
                    self.mode = Mode::AddUrl(text);
                    Vec::new()
                }
                KeyCode::Char(c) => {
                    text.push(c);
                    self.mode = Mode::AddUrl(text);
                    Vec::new()
                }
                _ => Vec::new(),
            },
            Mode::Search(mut text) => match key.code {
                KeyCode::Esc => {
                    self.mode = Mode::Normal;
                    Vec::new()
                }
                KeyCode::Enter => {
                    self.mode = Mode::Normal;
                    self.history_search = text;
                    self.history_selected = 0;
                    vec![Action::LoadHistory]
                }
                KeyCode::Backspace => {
                    text.pop();
                    self.mode = Mode::Search(text);
                    Vec::new()
                }
                KeyCode::Char(c) => {
                    text.push(c);
                    self.mode = Mode::Search(text);
                    Vec::new()
                }
                _ => Vec::new(),
            },
            Mode::Normal => self.on_normal_key(key),
        }
    }

    /// Paste (bracketed paste) into the input box.
    pub fn on_paste(&mut self, text: &str) {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        match &mut self.mode {
            Mode::AddUrl(t) | Mode::Search(t) => t.push_str(&clean),
            Mode::Normal => self.mode = Mode::AddUrl(clean),
            _ => {}
        }
    }

    fn on_normal_key(&mut self, key: KeyEvent) -> Vec<Action> {
        self.message = None;
        match key.code {
            KeyCode::Char('q') => {
                if self.active_count() > 0 {
                    self.mode = Mode::ConfirmQuit;
                    self.message = Some(format!(
                        "{} download(s) active: they will be paused. Quit? (y/n)",
                        self.active_count()
                    ));
                    Vec::new()
                } else {
                    vec![Action::Quit]
                }
            }
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
                Vec::new()
            }
            KeyCode::Char('a') => {
                self.mode = Mode::AddUrl(String::new());
                Vec::new()
            }
            KeyCode::Tab => {
                self.cycle_pane(true);
                self.pane_actions()
            }
            KeyCode::BackTab => {
                self.cycle_pane(false);
                self.pane_actions()
            }
            KeyCode::Char('1') => {
                self.tab = Tab::Queue;
                self.focus = Focus::List;
                Vec::new()
            }
            KeyCode::Char('2') => {
                self.tab = Tab::History;
                self.focus = Focus::List;
                vec![Action::LoadHistory]
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                Vec::new()
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                Vec::new()
            }
            KeyCode::PageUp => {
                self.move_selection(-10);
                Vec::new()
            }
            KeyCode::PageDown => {
                self.move_selection(10);
                Vec::new()
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.move_selection(i64::MIN / 2);
                Vec::new()
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.move_selection(i64::MAX / 2);
                Vec::new()
            }
            _ => match self.tab {
                Tab::Queue => self.on_queue_key(key.code),
                Tab::History => self.on_history_key(key.code),
            },
        }
    }

    fn on_queue_key(&mut self, code: KeyCode) -> Vec<Action> {
        let Some(job) = self.selected_job() else {
            return Vec::new();
        };
        let (id, state) = (job.id, job.state);
        let invalid = |what: &str| format!("Cannot {what} job {id}: it is {state}");
        match code {
            KeyCode::Char('p') | KeyCode::Char(' ') => match state {
                DownloadState::Paused => vec![Action::Resume(id)],
                s if !s.is_finished() => vec![Action::Pause(id)],
                _ => {
                    self.message = Some(invalid("pause"));
                    Vec::new()
                }
            },
            KeyCode::Char('c') => {
                if state.is_finished() {
                    self.message = Some(invalid("cancel"));
                    Vec::new()
                } else {
                    vec![Action::Cancel(id)]
                }
            }
            KeyCode::Char('r') => match state {
                DownloadState::Failed | DownloadState::Cancelled | DownloadState::Paused => {
                    vec![Action::Resume(id)]
                }
                DownloadState::Completed => vec![Action::Submit(job.url.clone())],
                _ => {
                    self.message = Some(invalid("retry"));
                    Vec::new()
                }
            },
            KeyCode::Char('d') | KeyCode::Delete => {
                if state.is_finished() {
                    vec![Action::Remove(id)]
                } else {
                    self.message = Some(format!(
                        "Job {id} is {state}: pause or cancel it before removing"
                    ));
                    Vec::new()
                }
            }
            KeyCode::Char('o') => match job.open_target() {
                Some(p) => vec![Action::Reveal(p)],
                None => {
                    self.message = Some("Nothing saved yet".into());
                    Vec::new()
                }
            },
            _ => Vec::new(),
        }
    }

    fn on_history_key(&mut self, code: KeyCode) -> Vec<Action> {
        match code {
            KeyCode::Char('/') => {
                self.mode = Mode::Search(self.history_search.clone());
                Vec::new()
            }
            KeyCode::Char('f') => {
                self.history_status = match self.history_status {
                    None => Some(EntryStatus::Completed),
                    Some(EntryStatus::Completed) => Some(EntryStatus::Failed),
                    Some(EntryStatus::Failed) => Some(EntryStatus::Cancelled),
                    Some(EntryStatus::Cancelled) => Some(EntryStatus::HandedOff),
                    Some(EntryStatus::HandedOff) => None,
                };
                self.history_selected = 0;
                vec![Action::LoadHistory]
            }
            KeyCode::Char('r') | KeyCode::Enter => match self.selected_history() {
                Some(e) => vec![Action::Redownload(e.url.clone())],
                None => Vec::new(),
            },
            KeyCode::Char('o') => match self.selected_history() {
                Some(e) => {
                    let path = std::path::Path::new(&e.output_dir).join(&e.filename);
                    vec![Action::Reveal(path.to_string_lossy().into_owned())]
                }
                None => Vec::new(),
            },
            KeyCode::Char('R') => vec![Action::LoadHistory],
            _ => Vec::new(),
        }
    }

    /// Tab order: queue list → detail pane → history → queue list.
    fn cycle_pane(&mut self, forward: bool) {
        let order = [
            (Tab::Queue, Focus::List),
            (Tab::Queue, Focus::Detail),
            (Tab::History, Focus::List),
        ];
        let pos = order
            .iter()
            .position(|p| *p == (self.tab, self.focus))
            .unwrap_or(0);
        let next = if forward {
            (pos + 1) % order.len()
        } else {
            (pos + order.len() - 1) % order.len()
        };
        (self.tab, self.focus) = order[next];
    }

    fn pane_actions(&self) -> Vec<Action> {
        if self.tab == Tab::History {
            vec![Action::LoadHistory]
        } else {
            Vec::new()
        }
    }

    /// Move the cursor of the focused list by `delta` rows.
    pub fn move_selection(&mut self, delta: i64) {
        let step = |cur: usize, len: usize| -> usize {
            if len == 0 {
                return 0;
            }
            (cur as i64).saturating_add(delta).clamp(0, len as i64 - 1) as usize
        };
        match (self.tab, self.focus) {
            (Tab::Queue, Focus::List) => {
                let before = self.selected;
                self.selected = step(self.selected, self.jobs.len());
                if before != self.selected {
                    self.detail_scroll = 0;
                }
            }
            (Tab::Queue, Focus::Detail) => {
                let files = self.selected_job().map_or(0, |j| j.files.len());
                self.detail_scroll = step(self.detail_scroll, files);
            }
            (Tab::History, _) => {
                self.history_selected = step(self.history_selected, self.history.len());
            }
        }
    }
}

/// Apply one download event (the `--jsonl` objects) to a job row.
fn apply_download(row: &mut JobRow, event: JsonEvent) {
    match event {
        JsonEvent::Started {
            protocol, output, ..
        } => {
            row.protocol = protocol;
            if !output.is_empty() {
                row.output = Some(output);
            }
        }
        JsonEvent::Meta {
            total, filename, ..
        } => {
            if total.is_some() {
                row.total = total;
            }
            if !filename.is_empty() {
                row.name = filename;
            }
        }
        JsonEvent::Progress {
            bytes,
            total,
            percent,
            speed_bps,
            eta_secs,
            ..
        } => {
            // Bare-fraction torrent updates carry no byte counts.
            if bytes > 0 || total.is_some() {
                row.bytes = bytes;
                row.total = total.or(row.total);
            }
            row.percent = percent.or(row.percent);
            row.speed_bps = speed_bps;
            row.eta_secs = eta_secs;
        }
        JsonEvent::Status { message } => row.status = Some(message),
        JsonEvent::Files { files } => {
            row.files = files
                .into_iter()
                .map(|f| FileRow {
                    name: f.name,
                    size: f.size,
                    downloaded: 0,
                    selected: f.selected,
                })
                .collect();
        }
        JsonEvent::FileProgress { files } => {
            for f in files {
                if let Some(r) = row.files.get_mut(f.idx) {
                    r.downloaded = f.downloaded;
                }
            }
        }
        JsonEvent::TorrentStats(stats) => {
            if stats.seeding {
                row.status = Some("Seeding".into());
                row.speed_bps = 0;
            }
            row.torrent = Some(*stats);
        }
        JsonEvent::Phase { phase, algorithm } => {
            row.status = Some(match (phase, algorithm) {
                (kget::events::Phase::Verifying, Some(a)) => format!("Verifying {a}"),
                (kget::events::Phase::Verifying, None) => "Verifying".into(),
                (kget::events::Phase::Extracting, _) => "Extracting".into(),
            });
        }
        JsonEvent::Verified { algorithm, ok, .. } => {
            row.status = Some(if ok {
                format!("{algorithm} verified")
            } else {
                format!("{algorithm} mismatch")
            });
        }
        JsonEvent::Saved { path, bytes } => {
            if let Some(name) = std::path::Path::new(&path).file_name() {
                row.name = name.to_string_lossy().into_owned();
            }
            row.path = Some(path);
            row.bytes = bytes;
        }
        JsonEvent::Completed => {
            row.percent = Some(100.0);
        }
        JsonEvent::Rule { name, output, .. } => {
            row.status = Some(format!("Rule '{name}'"));
            if !output.is_empty() {
                row.output = Some(output);
            }
        }
        JsonEvent::Duplicate { kind, action, .. } => {
            row.status = Some(format!("Duplicate ({kind}): {action}"));
        }
        JsonEvent::Error { message, .. } => row.error = Some(message),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use kget::events::{FileEntry, FileProgressEntry};
    use kget::runner::DownloadJob;

    pub(crate) fn info(id: JobId, url: &str, state: DownloadState) -> JobInfo {
        let job = DownloadJob::new(url);
        JobInfo {
            id,
            protocol: job.protocol(),
            job,
            state,
            created_at: 0,
            started_at: None,
            finished_at: None,
            progress: Default::default(),
            path: None,
            error: None,
            handed_off: false,
        }
    }

    fn dl(id: JobId, event: JsonEvent) -> JobEvent {
        JobEvent::Download { id, event }
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn events_update_rows() {
        let mut app = App::new("test");
        app.sync(&[info(1, "https://example.com/a.iso", DownloadState::Queued)]);
        assert_eq!(app.jobs[0].name, "a.iso");

        app.apply(JobEvent::State {
            id: 1,
            state: DownloadState::Running,
            error: None,
        });
        app.apply(dl(
            1,
            JsonEvent::Progress {
                bytes: 50,
                total: Some(100),
                percent: Some(50.0),
                speed_bps: 10,
                eta_secs: Some(5),
                progress: Some(0.5),
            },
        ));
        let row = &app.jobs[0];
        assert_eq!(row.state, DownloadState::Running);
        assert_eq!((row.bytes, row.total, row.speed_bps), (50, Some(100), 10));
        assert_eq!(app.total_speed(), 10);

        // A bare torrent fraction keeps the byte counts.
        app.apply(dl(
            1,
            JsonEvent::Progress {
                bytes: 0,
                total: None,
                percent: Some(60.0),
                speed_bps: 0,
                eta_secs: None,
                progress: Some(0.6),
            },
        ));
        assert_eq!((app.jobs[0].bytes, app.jobs[0].percent), (50, Some(60.0)));

        app.apply(dl(
            1,
            JsonEvent::Saved {
                path: "/tmp/dl/a.iso".into(),
                bytes: 100,
            },
        ));
        app.apply(JobEvent::State {
            id: 1,
            state: DownloadState::Completed,
            error: None,
        });
        let row = &app.jobs[0];
        assert_eq!(row.path.as_deref(), Some("/tmp/dl/a.iso"));
        assert_eq!((row.speed_bps, row.percent), (0, Some(100.0)));

        app.apply(JobEvent::Removed { id: 1 });
        assert!(app.jobs.is_empty());
    }

    #[test]
    fn torrent_events_fill_files_and_stats() {
        let mut app = App::new("test");
        app.sync(&[info(
            7,
            "magnet:?xt=urn:btih:abc&dn=Big+Show",
            DownloadState::Running,
        )]);
        assert_eq!(app.jobs[0].name, "Big Show");
        app.apply(dl(
            7,
            JsonEvent::Files {
                files: vec![
                    FileEntry {
                        name: "a.mkv".into(),
                        size: 10,
                        selected: Some(true),
                    },
                    FileEntry {
                        name: "b.nfo".into(),
                        size: 1,
                        selected: Some(false),
                    },
                ],
            },
        ));
        app.apply(dl(
            7,
            JsonEvent::FileProgress {
                files: vec![FileProgressEntry {
                    idx: 0,
                    downloaded: 4,
                    size: 10,
                    pct: 40.0,
                }],
            },
        ));
        app.apply(dl(
            7,
            JsonEvent::TorrentStats(Box::new(TorrentStats {
                peers: 3,
                seeding: true,
                ..Default::default()
            })),
        ));
        let row = &app.jobs[0];
        assert_eq!(row.files[0].downloaded, 4);
        assert_eq!(row.files[1].selected, Some(false));
        assert_eq!(row.torrent.as_ref().unwrap().peers, 3);
        assert_eq!(row.status.as_deref(), Some("Seeding"));
    }

    #[test]
    fn unknown_jobs_request_a_sync() {
        let mut app = App::new("test");
        app.apply(JobEvent::State {
            id: 9,
            state: DownloadState::Queued,
            error: None,
        });
        assert!(app.needs_sync);
        app.sync(&[info(9, "https://e.com/x", DownloadState::Queued)]);
        assert!(!app.needs_sync);
        assert_eq!(app.jobs.len(), 1);
    }

    #[test]
    fn keys_map_to_actions() {
        let mut app = App::new("test");
        app.sync(&[
            info(1, "https://e.com/a", DownloadState::Running),
            info(2, "https://e.com/b", DownloadState::Failed),
        ]);
        assert_eq!(app.on_key(key('p')), vec![Action::Pause(1)]);
        assert_eq!(app.on_key(key('c')), vec![Action::Cancel(1)]);
        assert!(app.on_key(key('d')).is_empty());
        assert!(app.message.is_some());
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected, 1);
        assert_eq!(app.on_key(key('r')), vec![Action::Resume(2)]);
        assert_eq!(app.on_key(key('d')), vec![Action::Remove(2)]);

        // Add a URL through the input box.
        assert!(app.on_key(key('a')).is_empty());
        for c in "https://x.y/z".chars() {
            app.on_key(key(c));
        }
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            app.on_key(enter),
            vec![Action::Submit("https://x.y/z".into())]
        );
        assert_eq!(app.mode, Mode::Normal);

        // Quitting with an active job asks first.
        assert!(app.on_key(key('q')).is_empty());
        assert_eq!(app.mode, Mode::ConfirmQuit);
        assert_eq!(app.on_key(key('y')), vec![Action::Quit]);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.on_key(ctrl_c), vec![Action::Quit]);
    }

    #[test]
    fn panes_and_history_filters() {
        let mut app = App::new("test");
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        app.on_key(tab);
        assert_eq!((app.tab, app.focus), (Tab::Queue, Focus::Detail));
        assert_eq!(app.on_key(tab), vec![Action::LoadHistory]);
        assert_eq!(app.tab, Tab::History);

        assert_eq!(app.on_key(key('f')), vec![Action::LoadHistory]);
        assert_eq!(app.history_status, Some(EntryStatus::Completed));
        app.on_key(key('/'));
        for c in "iso".chars() {
            app.on_key(key(c));
        }
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.on_key(enter), vec![Action::LoadHistory]);
        let q = app.history_query();
        assert_eq!(q.search.as_deref(), Some("iso"));
        assert_eq!(q.statuses, vec![EntryStatus::Completed]);

        app.on_key(tab);
        assert_eq!((app.tab, app.focus), (Tab::Queue, Focus::List));
    }

    #[test]
    fn speed_samples_are_bounded() {
        let mut app = App::new("test");
        let mut running = info(1, "https://e.com/a", DownloadState::Running);
        running.progress.speed_bps = 7;
        app.sync(&[running]);
        for _ in 0..SPEED_SAMPLES + 5 {
            app.sample_speed();
        }
        assert_eq!(app.speed.len(), SPEED_SAMPLES);
        assert_eq!(app.speed.back(), Some(&7));
    }
}
