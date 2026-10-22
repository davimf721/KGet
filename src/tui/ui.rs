//! Rendering of the [`App`] model.
//!
//! Only the terminal's default colors plus a few named accent colors are
//! used, and the selection is shown reversed, so the UI reads the same on
//! light and dark themes.

use super::state::{App, Focus, JobRow, Mode, Tab};
use kget::jobs::DownloadState;
use kget::queue::{EntryStatus, HistoryEntry};
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Cell, Clear, Paragraph, Row, Sparkline, Table, TableState, Tabs, Wrap,
};

/// Screen regions, shared by rendering and mouse hit-testing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Areas {
    pub title: Rect,
    /// Queue list or history list
    pub list: Rect,
    /// Detail pane (job or history entry)
    pub detail: Rect,
    /// Speed sparkline (Queue tab only)
    pub spark: Rect,
    pub footer: Rect,
}

/// Split the screen for the current tab.
pub fn layout(area: Rect, tab: Tab) -> Areas {
    let [title, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(area);
    let detail_h = if body.height >= 24 { 12 } else { 9 };
    match tab {
        Tab::Queue => {
            let [list, detail, spark] = Layout::vertical([
                Constraint::Min(4),
                Constraint::Length(detail_h),
                Constraint::Length(4),
            ])
            .areas(body);
            Areas {
                title,
                list,
                detail,
                spark,
                footer,
            }
        }
        Tab::History => {
            let [list, detail] =
                Layout::vertical([Constraint::Min(4), Constraint::Length(6)]).areas(body);
            Areas {
                title,
                list,
                detail,
                spark: Rect::default(),
                footer,
            }
        }
    }
}

/// Rows of a bordered table with a header that fit in `area`.
pub fn visible_rows(area: Rect) -> usize {
    area.height.saturating_sub(3) as usize
}

/// First row shown so that `selected` stays visible.
pub fn scroll_offset(selected: usize, visible: usize) -> usize {
    selected.saturating_sub(visible.saturating_sub(1))
}

pub fn render(f: &mut Frame, app: &App) {
    let areas = layout(f.area(), app.tab);
    render_title(f, app, areas.title);
    match app.tab {
        Tab::Queue => {
            render_queue(f, app, areas.list);
            render_detail(f, app, areas.detail);
            render_speed(f, app, areas.spark);
        }
        Tab::History => {
            render_history(f, app, areas.list);
            render_history_detail(f, app, areas.detail);
        }
    }
    render_footer(f, app, areas.footer);
    match &app.mode {
        Mode::AddUrl(text) => render_input(f, "Add download (Enter to start, Esc to cancel)", text),
        Mode::Search(text) => render_input(f, "Search history (Enter to apply)", text),
        Mode::Help => render_help(f),
        _ => {}
    }
}

// ----------------------------------------------------------------------------
// Title and footer
// ----------------------------------------------------------------------------

fn render_title(f: &mut Frame, app: &App, area: Rect) {
    let selected = match app.tab {
        Tab::Queue => 0,
        Tab::History => 1,
    };
    let [tabs_area, info_area] =
        Layout::horizontal([Constraint::Length(34), Constraint::Min(0)]).areas(area);
    let tabs = Tabs::new(vec!["1 Queue", "2 History"])
        .select(selected)
        .highlight_style(accent().add_modifier(Modifier::BOLD | Modifier::REVERSED))
        .divider("|");
    let title = Line::from(vec![Span::styled(" KGet ", bold())]);
    let [name_area, rest] =
        Layout::horizontal([Constraint::Length(7), Constraint::Min(0)]).areas(tabs_area);
    f.render_widget(Paragraph::new(title), name_area);
    f.render_widget(tabs, rest);
    let mut info = format!(
        "↓ {}/s  {} active  {} jobs  ({})  ? help ",
        size(app.total_speed()),
        app.active_count(),
        app.jobs.len(),
        app.source_label
    );
    if info.chars().count() > info_area.width as usize {
        // Narrow terminal: drop the source description.
        info = format!(
            "↓ {}/s  {} active  ? help ",
            size(app.total_speed()),
            app.active_count()
        );
    }
    f.render_widget(
        Paragraph::new(info).alignment(ratatui::layout::Alignment::Right),
        info_area,
    );
}

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let line = if let Some(msg) = &app.message {
        Line::from(Span::styled(msg.clone(), accent()))
    } else {
        let hints: &[(&str, &str)] = match app.tab {
            Tab::Queue => &[
                ("a", "add"),
                ("p", "pause/resume"),
                ("c", "cancel"),
                ("r", "retry"),
                ("d", "remove"),
                ("o", "reveal"),
                ("tab", "pane"),
                ("q", "quit"),
            ],
            Tab::History => &[
                ("/", "search"),
                ("f", "filter"),
                ("r", "download again"),
                ("o", "reveal"),
                ("tab", "pane"),
                ("q", "quit"),
            ],
        };
        let mut spans = Vec::new();
        for (k, v) in hints {
            spans.push(Span::styled(format!(" {k}"), bold()));
            spans.push(Span::raw(format!(" {v} ")));
        }
        Line::from(spans)
    };
    f.render_widget(Paragraph::new(line), area);
}

// ----------------------------------------------------------------------------
// Queue
// ----------------------------------------------------------------------------

fn render_queue(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.tab == Tab::Queue && app.focus == Focus::List;
    let block = pane(" Queue ", focused);
    if app.jobs.is_empty() {
        f.render_widget(
            Paragraph::new("No downloads yet. Press a to add a URL, magnet link or .torrent.")
                .block(block)
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let header = Row::new(["#", "State", "Progress", "Speed", "ETA", "Name"]).style(bold());
    let rows: Vec<Row> = app
        .jobs
        .iter()
        .map(|j| {
            Row::new(vec![
                Cell::from(j.id.to_string()),
                Cell::from(Span::styled(j.state.as_str(), state_style(j.state))),
                Cell::from(progress_cell(j)),
                Cell::from(if j.state.is_active() {
                    format!("{}/s", size(j.speed_bps))
                } else {
                    String::new()
                }),
                Cell::from(if j.state.is_active() {
                    j.eta_secs.map(eta).unwrap_or_else(|| "--".into())
                } else {
                    String::new()
                }),
                Cell::from(j.name.clone()),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(4),
        Constraint::Length(10),
        Constraint::Length(17),
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Fill(1),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut state = TableState::default()
        .with_offset(scroll_offset(app.selected, visible_rows(area)))
        .with_selected(Some(app.selected));
    f.render_stateful_widget(table, area, &mut state);
}

fn progress_cell(j: &JobRow) -> String {
    match j.percent {
        Some(p) => format!("{} {:>3.0}%", bar(p, 10), p.clamp(0.0, 100.0)),
        None if j.bytes > 0 => size(j.bytes),
        None => String::new(),
    }
}

/// Text progress bar of `width` cells.
pub fn bar(percent: f64, width: usize) -> String {
    let filled = ((percent.clamp(0.0, 100.0) / 100.0) * width as f64).round() as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.tab == Tab::Queue && app.focus == Focus::Detail;
    let block = pane(" Details ", focused);
    let Some(j) = app.selected_job() else {
        f.render_widget(Paragraph::new("").block(block), area);
        return;
    };
    let mut lines = vec![
        kv("Name", &j.name),
        kv("URL", &j.url),
        kv(
            "Saved",
            j.path
                .as_deref()
                .or(j.output.as_deref())
                .unwrap_or("(not yet)"),
        ),
        Line::from(vec![
            Span::styled("Protocol ", bold()),
            Span::raw(if j.advanced {
                format!("{} (turbo: parallel connections)", j.protocol)
            } else {
                j.protocol.clone()
            }),
            Span::styled("  State ", bold()),
            Span::styled(j.state.as_str(), state_style(j.state)),
            Span::raw(match (j.bytes, j.total) {
                (b, Some(t)) => format!("  {} / {}", size(b), size(t)),
                (b, None) if b > 0 => format!("  {}", size(b)),
                _ => String::new(),
            }),
        ]),
    ];
    if let Some(s) = &j.status {
        lines.push(kv("Status", s));
    }
    if let Some(e) = &j.error {
        lines.push(Line::from(vec![
            Span::styled("Error ", bold()),
            Span::styled(e.clone(), Style::new().fg(Color::Red)),
        ]));
    }
    if let Some(t) = &j.torrent {
        let mut peers = format!("{}", t.peers);
        if let Some(c) = t.peers_connecting {
            peers.push_str(&format!(" (+{c} connecting"));
            if let Some(k) = t.peers_known {
                peers.push_str(&format!(", {k} known"));
            }
            peers.push(')');
        }
        let mut swarm = vec![
            Span::styled("Peers ", bold()),
            Span::raw(peers),
            Span::styled("  Seeders ", bold()),
            Span::raw(
                t.seeders
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "–".into()),
            ),
            Span::styled("  ↓ ", bold()),
            Span::raw(format!("{}/s", size(t.download_bps))),
            Span::styled("  ↑ ", bold()),
            Span::raw(format!("{}/s", size(t.upload_bps))),
        ];
        swarm.push(Span::styled("  Ratio ", bold()));
        swarm.push(Span::raw(match t.seed_ratio_limit {
            Some(l) => format!("{:.2} / {l}", t.ratio),
            None => format!("{:.2}", t.ratio),
        }));
        if t.seeding {
            swarm.push(Span::styled("  Seeding ", bold()));
            swarm.push(Span::raw(match (t.seeding_secs, t.seed_time_limit_secs) {
                (Some(s), Some(l)) => format!("{} / {}", eta(s), eta(l)),
                (Some(s), None) => eta(s),
                _ => String::new(),
            }));
        }
        lines.push(Line::from(swarm));
        for tr in &t.trackers {
            lines.push(Line::from(format!(
                "  tracker {} [{}] {}",
                tr.url,
                tr.status,
                tr.message.clone().unwrap_or_default()
            )));
        }
    }
    if !j.files.is_empty() {
        let shown = j.files.iter().filter(|f| f.selected != Some(false)).count();
        lines.push(Line::from(Span::styled(
            format!("Files ({shown} of {} selected)", j.files.len()),
            bold(),
        )));
        for (i, file) in j.files.iter().enumerate().skip(app.detail_scroll) {
            let pct = if file.size > 0 {
                file.downloaded as f64 / file.size as f64 * 100.0
            } else {
                100.0
            };
            let skipped = file.selected == Some(false);
            let text = format!(
                " {:>3} {} {:>9} {}",
                i + 1,
                if skipped {
                    "  skipped ".to_string()
                } else {
                    format!("{} {:>3.0}%", bar(pct, 5), pct)
                },
                size(file.size),
                file.name
            );
            lines.push(if skipped {
                Line::from(Span::styled(text, Style::new().add_modifier(Modifier::DIM)))
            } else {
                Line::from(text)
            });
        }
    }
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_speed(f: &mut Frame, app: &App, area: Rect) {
    let data: Vec<u64> = app.speed.iter().copied().collect();
    let peak = data.iter().copied().max().unwrap_or(0);
    let title = format!(
        " Speed ↓ {}/s  peak {}/s ",
        size(app.total_speed()),
        size(peak)
    );
    // Show the most recent samples that fit.
    let width = area.width.saturating_sub(2) as usize;
    let start = data.len().saturating_sub(width);
    let spark = Sparkline::default()
        .block(Block::bordered().title(title))
        .data(data[start..].to_vec())
        .style(accent());
    f.render_widget(spark, area);
}

// ----------------------------------------------------------------------------
// History
// ----------------------------------------------------------------------------

fn render_history(f: &mut Frame, app: &App, area: Rect) {
    let mut title = String::from(" History");
    if let Some(s) = &app.history_status {
        title.push_str(&format!(" [{}]", s.as_str()));
    }
    if !app.history_search.is_empty() {
        title.push_str(&format!(" /{}", app.history_search));
    }
    title.push_str(&format!(" ({}) ", app.history.len()));
    let block = pane(&title, app.tab == Tab::History);
    if app.history.is_empty() {
        f.render_widget(
            Paragraph::new("No matching history entries.").block(block),
            area,
        );
        return;
    }
    let header = Row::new(["Date (UTC)", "Status", "Size", "Speed", "File"]).style(bold());
    let rows: Vec<Row> = app
        .history
        .iter()
        .map(|e| {
            Row::new(vec![
                Cell::from(e.created_at_display()),
                Cell::from(Span::styled(e.status.as_str(), history_style(&e.status))),
                Cell::from(e.bytes_total.map(size).unwrap_or_default()),
                Cell::from(
                    e.avg_speed
                        .map(|s| format!("{}/s", size(s)))
                        .unwrap_or_default(),
                ),
                Cell::from(e.filename.clone()),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(20),
        Constraint::Length(11),
        Constraint::Length(11),
        Constraint::Length(12),
        Constraint::Fill(1),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut state = TableState::default()
        .with_offset(scroll_offset(app.history_selected, visible_rows(area)))
        .with_selected(Some(app.history_selected));
    f.render_stateful_widget(table, area, &mut state);
}

fn render_history_detail(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title(" Entry ");
    let lines = match app.selected_history() {
        Some(e) => history_lines(e),
        None => Vec::new(),
    };
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn history_lines(e: &HistoryEntry) -> Vec<Line<'static>> {
    let mut lines = vec![
        kv("URL", &e.url),
        kv(
            "Saved",
            &std::path::Path::new(&e.output_dir)
                .join(&e.filename)
                .to_string_lossy(),
        ),
    ];
    let mut meta = Vec::new();
    if let Some(p) = &e.protocol {
        meta.push(p.clone());
    }
    if let Some(d) = e.duration_ms {
        meta.push(format!("took {}", eta(d / 1000)));
    }
    if let Some(ok) = e.checksum_ok {
        meta.push(
            if ok {
                "checksum ok"
            } else {
                "checksum mismatch"
            }
            .into(),
        );
    }
    if !meta.is_empty() {
        lines.push(kv("Info", &meta.join(", ")));
    }
    if let Some(err) = &e.error {
        lines.push(Line::from(vec![
            Span::styled("Error ", bold()),
            Span::styled(err.clone(), Style::new().fg(Color::Red)),
        ]));
    }
    lines
}

// ----------------------------------------------------------------------------
// Popups
// ----------------------------------------------------------------------------

fn popup(area: Rect, width: u16, height: u16) -> Rect {
    let [v] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [h] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(v);
    h
}

fn render_input(f: &mut Frame, title: &str, text: &str) {
    let area = popup(f.area(), f.area().width.saturating_sub(8).max(20), 3);
    f.render_widget(Clear, area);
    let inner_w = area.width.saturating_sub(3) as usize;
    // Keep the end of long input visible.
    let chars: Vec<char> = text.chars().collect();
    let shown: String = chars[chars.len().saturating_sub(inner_w)..]
        .iter()
        .collect();
    f.render_widget(
        Paragraph::new(shown.clone()).block(Block::bordered().title(format!(" {title} "))),
        area,
    );
    f.set_cursor_position((area.x + 1 + shown.chars().count() as u16, area.y + 1));
}

/// Keymap shown by `?`.
pub const HELP: &[(&str, &str)] = &[
    ("a", "add a URL, magnet link or .torrent path"),
    ("p / space", "pause or resume the selected job"),
    ("c", "cancel the selected job (partial data kept)"),
    ("r", "retry a failed/cancelled job; history: download again"),
    ("d / del", "remove a finished job from the queue"),
    ("o", "reveal the file in the file manager"),
    (
        "tab / S-tab",
        "next / previous pane (queue, details, history)",
    ),
    ("1 / 2", "queue / history tab"),
    ("↑↓ j k", "move; PgUp PgDn g G jump"),
    ("/", "history: search URL or file name"),
    ("f", "history: cycle status filter"),
    ("mouse", "click to select, wheel to scroll"),
    ("q / ctrl-c", "quit (active downloads are paused)"),
    ("?", "this help"),
];

fn render_help(f: &mut Frame) {
    let area = popup(f.area(), 64, HELP.len() as u16 + 2);
    f.render_widget(Clear, area);
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(format!(" {k:<12}"), bold()),
                Span::raw(*v),
            ])
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Keys (any key closes) ")),
        area,
    );
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

fn pane(title: &str, focused: bool) -> Block<'static> {
    let block = Block::bordered().title(title.to_string());
    if focused {
        block.border_style(accent())
    } else {
        block
    }
}

fn kv(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key} "), bold()),
        Span::raw(value.to_string()),
    ])
}

fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

fn accent() -> Style {
    Style::new().fg(Color::Cyan)
}

fn state_style(s: DownloadState) -> Style {
    match s {
        DownloadState::Running | DownloadState::Verifying => Style::new().fg(Color::Cyan),
        DownloadState::Completed => Style::new().fg(Color::Green),
        DownloadState::Failed => Style::new().fg(Color::Red),
        DownloadState::Paused | DownloadState::Cancelled => Style::new().fg(Color::Yellow),
        DownloadState::Queued => Style::new(),
    }
}

fn history_style(s: &EntryStatus) -> Style {
    match s {
        EntryStatus::Completed => Style::new().fg(Color::Green),
        EntryStatus::Failed => Style::new().fg(Color::Red),
        EntryStatus::Cancelled => Style::new().fg(Color::Yellow),
        EntryStatus::HandedOff => Style::new(),
    }
}

fn size(bytes: u64) -> String {
    humansize::format_size(bytes, humansize::BINARY)
}

/// `1h 2m`, `3m 4s`, `5s`.
pub fn eta(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m {}s", secs / 60, secs % 60),
        _ => format!("{}h {}m", secs / 3600, (secs % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::tests::info;
    use kget::events::{FileEntry, JsonEvent, TorrentStats};
    use kget::jobs::JobEvent;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn screen(app: &App, w: u16, h: u16) -> Vec<String> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    fn sample_app() -> App {
        let mut app = App::new("local, 3 at a time");
        let mut running = info(1, "https://example.com/big.iso", DownloadState::Running);
        running.progress.bytes = 512 * 1024;
        running.progress.total = Some(1024 * 1024);
        running.progress.percent = Some(50.0);
        running.progress.speed_bps = 2048;
        running.progress.eta_secs = Some(256);
        app.sync(&[
            running,
            info(2, "magnet:?xt=urn:btih:abc&dn=Show", DownloadState::Queued),
        ]);
        app
    }

    #[test]
    fn queue_screen_snapshot() {
        let app = sample_app();
        let lines = screen(&app, 80, 24);
        let expected = [
            " KGet   1 Queue | 2 History",
            "┌ Queue ───────────────────────────────────────────────────────────────────────┐",
            "│#    State      Progress          Speed        ETA      Name                  │",
            "│1    running    █████░░░░░  50%   2 KiB/s      4m 16s   big.iso               │",
            "│2    queued                                             Show                  │",
        ];
        for (i, want) in expected.iter().enumerate() {
            assert!(
                lines[i].starts_with(want),
                "line {i}:\n got: {:?}\nwant: {:?}\n{}",
                lines[i],
                want,
                lines.join("\n")
            );
        }
        assert!(lines[0].ends_with("? help"), "{}", lines[0]);
        let all = lines.join("\n");
        assert!(all.contains("Name big.iso"), "{all}");
        assert!(all.contains("URL https://example.com/big.iso"), "{all}");
        assert!(all.contains("State running  512 KiB / 1 MiB"), "{all}");
        assert!(all.contains(" Speed ↓ 2 KiB/s"), "{all}");
        assert!(lines[23].contains("a add"), "{}", lines[23]);
    }

    #[test]
    fn torrent_details_render() {
        let mut app = sample_app();
        app.selected = 1;
        app.apply(JobEvent::Download {
            id: 2,
            event: JsonEvent::Files {
                files: vec![
                    FileEntry {
                        name: "ep1.mkv".into(),
                        size: 1024,
                        selected: Some(true),
                    },
                    FileEntry {
                        name: "info.nfo".into(),
                        size: 10,
                        selected: Some(false),
                    },
                ],
            },
        });
        app.apply(JobEvent::Download {
            id: 2,
            event: JsonEvent::TorrentStats(Box::new(TorrentStats {
                peers: 12,
                peers_connecting: Some(3),
                peers_known: Some(40),
                upload_bps: 1024,
                ratio: 0.5,
                seed_ratio_limit: Some(1.5),
                ..Default::default()
            })),
        });
        let all = screen(&app, 100, 30).join("\n");
        assert!(
            all.contains("Peers 12 (+3 connecting, 40 known)  Seeders –"),
            "{all}"
        );
        assert!(all.contains("↑ 1 KiB/s  Ratio 0.50 / 1.5"), "{all}");
        assert!(all.contains("Files (1 of 2 selected)"), "{all}");
        assert!(all.contains("skipped"), "{all}");
    }

    #[test]
    fn help_and_input_popups_render() {
        let mut app = sample_app();
        app.mode = Mode::Help;
        let all = screen(&app, 80, 24).join("\n");
        assert!(all.contains("Keys (any key closes)"), "{all}");
        app.mode = Mode::AddUrl("https://x.y/z".into());
        let all = screen(&app, 80, 24).join("\n");
        assert!(all.contains("│https://x.y/z"), "{all}");
    }

    #[test]
    fn history_screen_renders_filters() {
        let mut app = App::new("local");
        app.tab = Tab::History;
        app.history_status = Some(EntryStatus::Failed);
        app.history_search = "iso".into();
        let mut e = HistoryEntry::new("https://e.com/x.iso", "/tmp", None);
        e.status = EntryStatus::Failed;
        e.error = Some("HTTP 404".into());
        app.set_history(vec![e]);
        let all = screen(&app, 90, 20).join("\n");
        assert!(all.contains("History [failed] /iso (1)"), "{all}");
        assert!(all.contains("x.iso"), "{all}");
        assert!(all.contains("Error HTTP 404"), "{all}");
    }

    #[test]
    fn helpers_format() {
        assert_eq!(bar(50.0, 4), "██░░");
        assert_eq!(bar(150.0, 2), "██");
        assert_eq!(eta(3661), "1h 1m");
        assert_eq!(scroll_offset(10, 5), 6);
        assert_eq!(scroll_offset(2, 5), 0);
    }
}
