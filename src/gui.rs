use eframe::egui;
use kget::app::{DownloadCommand, WorkerToGuiMessage};
use kget::queue::DownloadHistory;
use rfd::FileDialog;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

// ============================================================================
// Apple-Inspired Color System
// ============================================================================

#[derive(Clone, Copy)]
struct Colors {
    bg: egui::Color32,
    card: egui::Color32,
    sidebar_bg: egui::Color32,
    input_bg: egui::Color32,
    border: egui::Color32,
    text_primary: egui::Color32,
    text_secondary: egui::Color32,
    text_muted: egui::Color32,
    accent: egui::Color32,
    success: egui::Color32,
    error: egui::Color32,
    warning: egui::Color32,
    purple: egui::Color32,
    is_dark: bool,
}

impl Colors {
    fn light() -> Self {
        use egui::Color32 as C;
        Self {
            bg: C::from_rgb(242, 242, 247),
            card: C::WHITE,
            sidebar_bg: C::from_rgb(246, 246, 251),
            input_bg: C::from_rgb(228, 228, 233),
            border: C::from_rgb(209, 209, 214),
            text_primary: C::from_rgb(0, 0, 0),
            text_secondary: C::from_rgb(60, 60, 67),
            text_muted: C::from_rgb(142, 142, 147),
            accent: C::from_rgb(0, 122, 255),
            success: C::from_rgb(52, 199, 89),
            error: C::from_rgb(255, 59, 48),
            warning: C::from_rgb(255, 149, 0),
            purple: C::from_rgb(175, 82, 222),
            is_dark: false,
        }
    }

    fn dark() -> Self {
        use egui::Color32 as C;
        Self {
            bg: C::from_rgb(0, 0, 0),
            card: C::from_rgb(28, 28, 30),
            sidebar_bg: C::from_rgb(18, 18, 20),
            input_bg: C::from_rgb(44, 44, 46),
            border: C::from_rgb(56, 56, 58),
            text_primary: C::WHITE,
            text_secondary: C::from_rgb(210, 210, 218),
            text_muted: C::from_rgb(142, 142, 147),
            accent: C::from_rgb(10, 132, 255),
            success: C::from_rgb(48, 209, 88),
            error: C::from_rgb(255, 69, 58),
            warning: C::from_rgb(255, 159, 10),
            purple: C::from_rgb(191, 90, 242),
            is_dark: true,
        }
    }

    fn for_mode(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    fn accent_subtle(self) -> egui::Color32 {
        let a = self.accent;
        egui::Color32::from_rgba_unmultiplied(
            a.r(),
            a.g(),
            a.b(),
            if self.is_dark { 30 } else { 20 },
        )
    }

    fn status_color(self, status: &DownloadStatus) -> egui::Color32 {
        match status {
            DownloadStatus::Pending => self.text_muted,
            DownloadStatus::Downloading => self.accent,
            DownloadStatus::Verifying => self.purple,
            DownloadStatus::Completed => self.success,
            DownloadStatus::Failed => self.error,
            DownloadStatus::Cancelled => self.warning,
        }
    }
}

// ============================================================================
// Download Types
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Verifying,
    Completed,
    Failed,
    Cancelled,
}

impl DownloadStatus {
    fn label(&self) -> &str {
        match self {
            Self::Pending => "Pending",
            Self::Downloading => "Downloading",
            Self::Verifying => "Verifying",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    fn is_active(&self) -> bool {
        matches!(self, Self::Downloading | Self::Verifying | Self::Pending)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadFilter {
    All,
    Active,
    Completed,
    Failed,
}

impl DownloadFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All Downloads",
            Self::Active => "Active",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarTab {
    Downloads,
    History,
}

#[derive(Debug, Clone)]
pub struct DownloadItem {
    pub id: u64,
    pub url: String,
    pub output_path: String,
    pub filename: String,
    pub status: DownloadStatus,
    pub progress: f32,
    pub speed: String,
    pub speed_raw: f32,           // bytes/s, 0 if unknown
    pub speed_history: Vec<f32>,  // last 30 speed readings in bytes/s
    pub eta: String,
    pub total_size: String,
    pub is_advanced: bool,
    pub is_iso: bool,
    pub is_torrent: bool,
    pub verify_integrity: bool,
    pub error: Option<String>,
    pub sha256: Option<String>,
    pub expected_sha256: Option<String>,
    pub connections: u8,
    pub start_time: Option<Instant>,
}

impl DownloadItem {
    fn new(
        id: u64,
        url: String,
        output_path: String,
        is_advanced: bool,
        verify_iso: bool,
        expected_sha256: Option<String>,
    ) -> Self {
        let is_torrent = url.starts_with("magnet:");
        let filename = if is_torrent {
            url.split("dn=")
                .nth(1)
                .and_then(|s| s.split('&').next())
                .map(|s| urlencoding::decode(s).unwrap_or_default().to_string())
                .unwrap_or_else(|| "Torrent Download".to_string())
        } else {
            url.split('/')
                .last()
                .unwrap_or("download")
                .split('?')
                .next()
                .unwrap_or("download")
                .to_string()
        };
        let is_iso = url.to_lowercase().ends_with(".iso");
        Self {
            id,
            url,
            output_path,
            filename,
            status: DownloadStatus::Pending,
            progress: 0.0,
            speed: String::new(),
            speed_raw: 0.0,
            speed_history: Vec::new(),
            eta: String::new(),
            total_size: String::new(),
            is_advanced,
            is_iso,
            is_torrent,
            verify_integrity: verify_iso || is_iso,
            error: None,
            sha256: None,
            expected_sha256,
            connections: if is_advanced { 4 } else { 1 },
            start_time: None,
        }
    }
}

// ============================================================================
// Main GUI Application
// ============================================================================

pub struct KGetGui {
    // Input state
    url: String,
    output_path: String,
    is_advanced: bool,
    verify_iso: bool,
    expected_sha256: String,
    filter: DownloadFilter,

    // Downloads
    downloads: Vec<DownloadItem>,
    active_download_id: Option<u64>,
    next_download_id: u64,

    // UI state
    status_text: String,
    validation_error: Option<String>,
    animation_phase: f32,
    dark_mode: bool,
    drag_over: bool,

    // Clipboard monitor
    last_clipboard_text: String,
    last_clipboard_check: Instant,
    clipboard_banner: Option<String>,

    // Sidebar tabs
    sidebar_tab: SidebarTab,

    // History tab
    history_entries: Vec<kget::queue::HistoryEntry>,
    history_loaded: bool,

    // Resources
    logo_texture: Option<egui::TextureHandle>,

    // Communication channels
    download_tx: mpsc::Sender<DownloadCommand>,
    status_rx: mpsc::Receiver<WorkerToGuiMessage>,
}

impl KGetGui {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        download_tx: mpsc::Sender<DownloadCommand>,
        status_rx: mpsc::Receiver<WorkerToGuiMessage>,
    ) -> Self {
        let ctx = &cc.egui_ctx;
        let dark_mode = ctx.style().visuals.dark_mode;

        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (egui::TextStyle::Heading, egui::FontId::proportional(20.0)),
            (egui::TextStyle::Body, egui::FontId::proportional(14.0)),
            (egui::TextStyle::Button, egui::FontId::proportional(13.0)),
            (egui::TextStyle::Monospace, egui::FontId::monospace(12.0)),
            (egui::TextStyle::Small, egui::FontId::proportional(11.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        ctx.set_style(style);

        let logo_texture = {
            let logo_bytes = include_bytes!("../logo.png");
            if let Ok(img) = image::load_from_memory(logo_bytes) {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.into_raw();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                Some(ctx.load_texture("logo", color_image, egui::TextureOptions::LINEAR))
            } else {
                None
            }
        };

        let default_download_dir = dirs::download_dir()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            .to_string_lossy()
            .to_string();

        Self {
            url: String::new(),
            output_path: default_download_dir,
            is_advanced: true,
            verify_iso: true,
            expected_sha256: String::new(),
            filter: DownloadFilter::All,
            downloads: Vec::new(),
            active_download_id: None,
            next_download_id: 1,
            status_text: "Ready".into(),
            validation_error: None,
            animation_phase: 0.0,
            dark_mode,
            drag_over: false,
            last_clipboard_text: String::new(),
            last_clipboard_check: Instant::now(),
            clipboard_banner: None,
            sidebar_tab: SidebarTab::Downloads,
            history_entries: Vec::new(),
            history_loaded: false,
            logo_texture,
            download_tx,
            status_rx,
        }
    }

    // ========================================================================
    // Message Processing
    // ========================================================================

    fn process_status_updates(&mut self) {
        while let Ok(msg) = self.status_rx.try_recv() {
            if let Some(id) = self.active_download_id {
                if let Some(download) = self.downloads.iter_mut().find(|d| d.id == id) {
                    match msg {
                        WorkerToGuiMessage::Progress(p) => {
                            if p > download.progress {
                                download.progress = p;
                            }
                            download.status = DownloadStatus::Downloading;
                            if let Some(start) = download.start_time {
                                let elapsed = start.elapsed().as_secs_f32();
                                if elapsed > 0.5 && p > 0.0 && p < 1.0 {
                                    let speed = p / elapsed;
                                    if speed > 0.0 {
                                        download.eta = Self::format_time((1.0 - p) / speed);
                                    }
                                }
                            }
                            self.status_text = format!("Downloading: {:.1}%", p * 100.0);
                        }
                        WorkerToGuiMessage::StatusUpdate(s) => {
                            self.status_text = s.clone();
                            if s.contains("Verifying")
                                || s.contains("SHA256")
                                || s.contains("Calculating")
                            {
                                download.status = DownloadStatus::Verifying;
                            }
                            if let Some(speed_str) = Self::extract_speed(&s) {
                                download.speed = speed_str.clone();
                                // Track raw speed for sparkline (push up to 30 samples)
                                if let Some(bps) = Self::speed_str_to_bps(&speed_str) {
                                    download.speed_raw = bps;
                                    download.speed_history.push(bps);
                                    if download.speed_history.len() > 30 {
                                        download.speed_history.remove(0);
                                    }
                                }
                            }
                            if let Some(size_str) = Self::extract_size(&s) {
                                download.total_size = size_str;
                            }
                        }
                        WorkerToGuiMessage::Completed(s_msg) => {
                            download.status = DownloadStatus::Completed;
                            download.progress = 1.0;
                            self.status_text = "Download completed!".into();
                            self.active_download_id = None;
                            Self::send_native_notification("KGet", &download.filename);
                            if s_msg.contains("SHA256") {
                                download.sha256 = Some(s_msg.clone());
                            }
                        }
                        WorkerToGuiMessage::Error(err_msg) => {
                            download.status = DownloadStatus::Failed;
                            download.error = Some(err_msg.clone());
                            self.status_text = format!("Error: {err_msg}");
                            self.active_download_id = None;
                            Self::send_native_notification("KGet — Download Failed", &err_msg);
                        }
                    }
                }
            }
        }
    }

    #[cfg(all(
        feature = "notify-rust",
        any(target_os = "linux", target_os = "windows")
    ))]
    fn send_native_notification(summary: &str, body: &str) {
        let _ = notify_rust::Notification::new()
            .summary(summary)
            .body(body)
            .appname("KGet")
            .show();
    }

    #[cfg(not(all(
        feature = "notify-rust",
        any(target_os = "linux", target_os = "windows")
    )))]
    fn send_native_notification(_summary: &str, _body: &str) {}

    fn extract_speed(s: &str) -> Option<String> {
        let patterns = ["MB/s", "KB/s", "B/s", "MiB/s", "KiB/s"];
        for pattern in patterns {
            if let Some(idx) = s.find(pattern) {
                let start = s[..idx]
                    .rfind(|c: char| !c.is_numeric() && c != '.' && c != ' ')
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let speed = s[start..idx + pattern.len()].trim().to_string();
                if !speed.is_empty() {
                    return Some(speed);
                }
            }
        }
        None
    }

    /// Convert a human-readable speed string (e.g. "1.5 MB/s") to bytes/s.
    fn speed_str_to_bps(s: &str) -> Option<f32> {
        let s = s.trim();
        let (num_str, unit) = if let Some(idx) = s.find(|c: char| c.is_alphabetic()) {
            (&s[..idx], s[idx..].trim())
        } else {
            return None;
        };
        let num: f32 = num_str.trim().parse().ok()?;
        let multiplier = match unit {
            "MB/s" | "MiB/s" => 1_048_576.0_f32,
            "KB/s" | "KiB/s" => 1_024.0,
            "B/s" => 1.0,
            _ => return None,
        };
        Some(num * multiplier)
    }

    fn extract_size(s: &str) -> Option<String> {
        if let Some(idx) = s.find("Length:") {
            let rest = &s[idx + 7..];
            let size: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '.' || *c == ' ')
                .collect();
            if !size.trim().is_empty() {
                return Some(size.trim().to_string());
            }
        }
        None
    }

    fn format_time(seconds: f32) -> String {
        let secs = seconds as u64;
        if secs < 60 {
            format!("{secs}s")
        } else if secs < 3600 {
            format!("{}m {}s", secs / 60, secs % 60)
        } else {
            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
        }
    }

    // ========================================================================
    // Clipboard Monitor
    // ========================================================================

    fn check_clipboard(&mut self) {
        // Rate-limit to once per 1.5 seconds.
        if self.last_clipboard_check.elapsed().as_millis() < 1500 {
            return;
        }
        self.last_clipboard_check = Instant::now();

        if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Ok(text) = cb.get_text() {
                let text = text.trim().to_string();
                if text == self.last_clipboard_text {
                    return;
                }
                self.last_clipboard_text = text.clone();

                // Check if it looks like a downloadable URL.
                if Self::is_downloadable_url(&text) {
                    // Don't show banner if URL is already in the list.
                    let already_queued = self.downloads.iter().any(|d| d.url == text);
                    if !already_queued {
                        self.clipboard_banner = Some(text);
                    }
                }
            }
        }
    }

    fn is_downloadable_url(s: &str) -> bool {
        let l = s.to_lowercase();
        l.starts_with("http://")
            || l.starts_with("https://")
            || l.starts_with("ftp://")
            || l.starts_with("sftp://")
            || l.starts_with("webdav://")
            || l.starts_with("webdavs://")
            || l.starts_with("magnet:?")
    }

    // ========================================================================
    // Input Validation
    // ========================================================================

    fn validate_input(&self) -> Result<(), String> {
        if self.url.is_empty() {
            return Err("Enter a URL to download".into());
        }
        let url_lower = self.url.to_lowercase();
        let supported = url_lower.starts_with("http://")
            || url_lower.starts_with("https://")
            || url_lower.starts_with("ftp://")
            || url_lower.starts_with("sftp://")
            || url_lower.starts_with("webdav://")
            || url_lower.starts_with("webdavs://")
            || url_lower.starts_with("magnet:?");
        if !supported {
            return Err(
                "Supported: http://, https://, ftp://, sftp://, webdav://, magnet:".into(),
            );
        }
        if self.output_path.is_empty() {
            return Err("Select a destination folder".into());
        }
        if !std::path::Path::new(&self.output_path).exists() {
            return Err("Destination folder does not exist".into());
        }
        Ok(())
    }

    // ========================================================================
    // Actions
    // ========================================================================

    fn select_output_directory(&mut self) {
        if let Some(path) = FileDialog::new()
            .set_directory(std::path::Path::new(&self.output_path))
            .pick_folder()
        {
            self.output_path = path.to_string_lossy().to_string();
        }
    }

    fn open_download_folder(&self, path: &str) {
        let folder_path = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(&self.output_path));
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&folder_path).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open")
            .arg(&folder_path)
            .spawn();
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("explorer")
            .arg(&folder_path)
            .spawn();
    }

    fn open_download_file(&self, path: &str) {
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(path).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", path])
            .spawn();
    }

    fn copy_to_clipboard(text: &str) {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(text.to_string());
        }
    }

    fn start_download(&mut self) {
        match self.validate_input() {
            Ok(()) => {
                self.validation_error = None;
                let is_magnet = self.url.to_lowercase().starts_with("magnet:?");
                let expected_sha256 = self.expected_sha256.trim().to_string();
                let expected_sha256 = if expected_sha256.is_empty() {
                    None
                } else {
                    Some(expected_sha256)
                };
                let final_output_path = if is_magnet {
                    self.output_path.clone()
                } else {
                    crate::utils::resolve_output_path(
                        Some(self.output_path.clone()),
                        &self.url,
                        "downloaded_file",
                    )
                };
                let download = DownloadItem::new(
                    self.next_download_id,
                    self.url.clone(),
                    final_output_path.clone(),
                    self.is_advanced,
                    self.verify_iso || expected_sha256.is_some(),
                    expected_sha256.clone(),
                );
                let id = download.id;
                self.downloads.insert(0, download);
                self.downloads[0].status = DownloadStatus::Downloading;
                self.downloads[0].start_time = Some(Instant::now());
                self.active_download_id = Some(id);
                self.next_download_id += 1;
                self.download_tx
                    .send(DownloadCommand::Start {
                        url: self.url.clone(),
                        output_path: final_output_path,
                        is_advanced: self.is_advanced,
                        verify_iso: self.verify_iso || expected_sha256.is_some(),
                        expected_sha256,
                    })
                    .ok();
                self.status_text = "Starting download…".into();
                self.url.clear();
                self.expected_sha256.clear();
                // Switch to Downloads tab when a download starts.
                self.sidebar_tab = SidebarTab::Downloads;
            }
            Err(e) => {
                self.validation_error = Some(e);
            }
        }
    }

    fn cancel_download(&mut self, id: u64) {
        if let Some(download) = self.downloads.iter_mut().find(|d| d.id == id) {
            download.status = DownloadStatus::Cancelled;
            if self.active_download_id == Some(id) {
                self.download_tx.send(DownloadCommand::Cancel).ok();
                self.active_download_id = None;
            }
        }
    }

    fn remove_download(&mut self, id: u64) {
        self.downloads.retain(|d| d.id != id);
    }

    fn clear_completed(&mut self) {
        self.downloads.retain(|d| {
            matches!(
                d.status,
                DownloadStatus::Downloading | DownloadStatus::Pending | DownloadStatus::Verifying
            )
        });
    }

    fn retry_download(&mut self, id: u64) {
        if let Some(download) = self.downloads.iter().find(|d| d.id == id).cloned() {
            let new_download = DownloadItem::new(
                self.next_download_id,
                download.url.clone(),
                download.output_path.clone(),
                download.is_advanced,
                download.verify_integrity,
                download.expected_sha256.clone(),
            );
            let new_id = new_download.id;
            self.downloads.insert(0, new_download);
            self.downloads[0].status = DownloadStatus::Downloading;
            self.downloads[0].start_time = Some(Instant::now());
            self.active_download_id = Some(new_id);
            self.next_download_id += 1;
            self.remove_download(id);
            self.download_tx
                .send(DownloadCommand::Start {
                    url: download.url,
                    output_path: download.output_path,
                    is_advanced: download.is_advanced,
                    verify_iso: download.verify_integrity,
                    expected_sha256: download.expected_sha256,
                })
                .ok();
        }
    }

    fn filtered_downloads(&self) -> Vec<DownloadItem> {
        self.downloads
            .iter()
            .filter(|d| match self.filter {
                DownloadFilter::All => true,
                DownloadFilter::Active => d.status.is_active(),
                DownloadFilter::Completed => d.status == DownloadStatus::Completed,
                DownloadFilter::Failed => {
                    matches!(d.status, DownloadStatus::Failed | DownloadStatus::Cancelled)
                }
            })
            .cloned()
            .collect()
    }

    fn count_for_filter(&self, filter: DownloadFilter) -> usize {
        self.downloads
            .iter()
            .filter(|d| match filter {
                DownloadFilter::All => true,
                DownloadFilter::Active => d.status.is_active(),
                DownloadFilter::Completed => d.status == DownloadStatus::Completed,
                DownloadFilter::Failed => {
                    matches!(d.status, DownloadStatus::Failed | DownloadStatus::Cancelled)
                }
            })
            .count()
    }

    fn truncate_path(path: &str, max_len: usize) -> String {
        if path.len() <= max_len {
            return path.to_string();
        }
        let parts: Vec<&str> = path.split(std::path::MAIN_SEPARATOR).collect();
        if parts.len() > 2 {
            format!(
                "…{}{}",
                std::path::MAIN_SEPARATOR,
                parts.last().unwrap_or(&"")
            )
        } else {
            format!("…{}", &path[path.len().saturating_sub(max_len - 1)..])
        }
    }

    // ========================================================================
    // Theme & Visuals
    // ========================================================================

    fn apply_visuals(&self, ctx: &egui::Context, c: &Colors) {
        let mut visuals = if c.is_dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.panel_fill = c.bg;
        visuals.window_fill = c.card;
        visuals.extreme_bg_color = c.input_bg;
        visuals.widgets.noninteractive.bg_fill = c.input_bg;
        visuals.widgets.inactive.bg_fill = c.input_bg;
        visuals.widgets.hovered.bg_fill = if c.is_dark {
            egui::Color32::from_rgb(60, 60, 64)
        } else {
            egui::Color32::from_rgb(212, 212, 218)
        };
        visuals.widgets.active.bg_fill = c.accent;
        visuals.selection.bg_fill =
            egui::Color32::from_rgba_unmultiplied(c.accent.r(), c.accent.g(), c.accent.b(), 50);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, c.border);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.5, c.border);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, c.text_secondary);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, c.text_secondary);
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, c.text_primary);
        visuals.window_stroke = egui::Stroke::new(0.5, c.border);
        ctx.set_visuals(visuals);
    }

    // ========================================================================
    // Sidebar
    // ========================================================================

    fn render_sidebar_item(
        ui: &mut egui::Ui,
        label: &str,
        count: usize,
        is_selected: bool,
        c: &Colors,
    ) -> bool {
        let height = 30.0;
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::click(),
        );

        if ui.is_rect_visible(rect) {
            let bg = if is_selected {
                c.accent_subtle()
            } else if response.hovered() {
                if c.is_dark {
                    egui::Color32::from_white_alpha(10)
                } else {
                    egui::Color32::from_black_alpha(8)
                }
            } else {
                egui::Color32::TRANSPARENT
            };

            let painter = ui.painter_at(rect);
            painter.rect_filled(rect.shrink(2.0), 7.0, bg);

            if is_selected {
                let accent_bar = egui::Rect::from_min_size(
                    rect.min + egui::vec2(4.0, 5.0),
                    egui::vec2(2.5, rect.height() - 10.0),
                );
                painter.rect_filled(accent_bar, 1.5, c.accent);
            }

            let text_color = if is_selected { c.accent } else { c.text_secondary };
            painter.text(
                egui::pos2(rect.min.x + 18.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(13.0),
                text_color,
            );

            if count > 0 {
                let badge_str = if count > 99 { "99+".to_string() } else { count.to_string() };
                let badge_w = (badge_str.len() as f32 * 7.5 + 10.0).max(20.0);
                let badge_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.max.x - badge_w / 2.0 - 10.0, rect.center().y),
                    egui::vec2(badge_w, 15.0),
                );
                let badge_bg = if is_selected { c.accent } else { c.border };
                let badge_fg = if is_selected { egui::Color32::WHITE } else { c.text_muted };
                painter.rect_filled(badge_rect, badge_rect.height() / 2.0, badge_bg);
                painter.text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &badge_str,
                    egui::FontId::proportional(9.0),
                    badge_fg,
                );
            }
        }

        response.clicked()
    }

    fn render_sidebar(&mut self, ui: &mut egui::Ui, c: &Colors) {
        ui.add_space(16.0);

        // App branding
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            if let Some(texture) = &self.logo_texture {
                ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(22.0, 22.0)));
                ui.add_space(6.0);
            }
            ui.label(
                egui::RichText::new("KGet")
                    .size(15.0)
                    .strong()
                    .color(c.text_primary),
            );
        });

        ui.add_space(20.0);

        // ---- LIBRARY section ----
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.label(
                egui::RichText::new("LIBRARY")
                    .size(10.0)
                    .color(c.text_muted)
                    .strong(),
            );
        });
        ui.add_space(4.0);

        let filters = [
            DownloadFilter::All,
            DownloadFilter::Active,
            DownloadFilter::Completed,
            DownloadFilter::Failed,
        ];
        for filter in filters {
            let count = self.count_for_filter(filter);
            let is_selected =
                self.sidebar_tab == SidebarTab::Downloads && self.filter == filter;
            if Self::render_sidebar_item(ui, filter.label(), count, is_selected, c) {
                self.filter = filter;
                self.sidebar_tab = SidebarTab::Downloads;
            }
        }

        ui.add_space(12.0);

        // ---- HISTORY section ----
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.label(
                egui::RichText::new("HISTORY")
                    .size(10.0)
                    .color(c.text_muted)
                    .strong(),
            );
        });
        ui.add_space(4.0);

        let history_count = self.history_entries.len();
        let history_selected = self.sidebar_tab == SidebarTab::History;
        if Self::render_sidebar_item(ui, "All Downloads", history_count, history_selected, c) {
            self.sidebar_tab = SidebarTab::History;
            if !self.history_loaded {
                let h = DownloadHistory::load();
                self.history_entries = h.entries().to_vec();
                self.history_entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                self.history_loaded = true;
            }
        }

        // Bottom: theme toggle + version
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                let version = env!("CARGO_PKG_VERSION");
                ui.label(
                    egui::RichText::new(format!("v{version}"))
                        .size(10.0)
                        .color(c.text_muted),
                );
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                let icon = if self.dark_mode { "☀ Light" } else { "☾ Dark" };
                if ui
                    .small_button(egui::RichText::new(icon).color(c.text_muted))
                    .clicked()
                {
                    self.dark_mode = !self.dark_mode;
                }
            });
            ui.add_space(6.0);
        });
    }

    // ========================================================================
    // Clipboard Banner
    // ========================================================================

    fn render_clipboard_banner(&mut self, ui: &mut egui::Ui, c: &Colors) {
        let banner_url = match self.clipboard_banner.clone() {
            Some(u) => u,
            None => return,
        };

        let truncated = if banner_url.len() > 60 {
            format!("{}…", &banner_url[..60])
        } else {
            banner_url.clone()
        };

        egui::Frame::new()
            .fill(c.card)
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .stroke(egui::Stroke::new(0.5, c.accent))
            .shadow(egui::epaint::Shadow {
                offset: [0, 2],
                blur: 8,
                spread: 0,
                color: egui::Color32::from_rgba_unmultiplied(
                    c.accent.r(), c.accent.g(), c.accent.b(), 30,
                ),
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🔗").size(13.0));
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("URL detected in clipboard")
                            .size(12.0)
                            .strong()
                            .color(c.text_primary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(egui::RichText::new("✕").color(c.text_muted))
                            .on_hover_text("Dismiss")
                            .clicked()
                        {
                            self.clipboard_banner = None;
                        }
                        ui.add_space(4.0);
                        let dl_btn = egui::Button::new(
                            egui::RichText::new("Download").color(egui::Color32::WHITE).size(12.0),
                        )
                        .fill(c.accent)
                        .min_size(egui::vec2(80.0, 22.0));
                        if ui.add(dl_btn).clicked() {
                            self.url = banner_url.clone();
                            self.clipboard_banner = None;
                            self.start_download();
                        }
                    });
                });
                ui.add_space(2.0);
                ui.label(egui::RichText::new(&truncated).size(11.0).color(c.text_muted));
            });

        ui.add_space(8.0);
    }

    // ========================================================================
    // URL Input Bar
    // ========================================================================

    fn render_url_bar(&mut self, ui: &mut egui::Ui, c: &Colors) {
        let available_width = ui.available_width();

        egui::Frame::new()
            .fill(c.card)
            .corner_radius(12.0)
            .inner_margin(egui::Margin::same(14))
            .stroke(egui::Stroke::new(0.5, c.border))
            .shadow(egui::epaint::Shadow {
                offset: [0, 1],
                blur: 4,
                spread: 0,
                color: egui::Color32::from_black_alpha(if c.is_dark { 40 } else { 8 }),
            })
            .show(ui, |ui| {
                ui.set_width(available_width - 28.0);

                // URL input row
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("↓").size(16.0).color(c.accent));
                    ui.add_space(4.0);

                    let url_width = (ui.available_width() - 130.0).max(200.0);
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.url)
                            .hint_text("Paste or drop a URL…")
                            .desired_width(url_width)
                            .font(egui::TextStyle::Body),
                    );
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.start_download();
                    }

                    if ui
                        .button("Paste")
                        .on_hover_text("Paste from clipboard")
                        .clicked()
                    {
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            if let Ok(text) = cb.get_text() {
                                self.url = text.trim().to_string();
                                self.clipboard_banner = None;
                            }
                        }
                    }

                    let dl_btn = egui::Button::new(
                        egui::RichText::new("Download")
                            .color(egui::Color32::WHITE)
                            .size(13.0),
                    )
                    .fill(c.accent)
                    .min_size(egui::vec2(90.0, 26.0));

                    if ui.add_enabled(!self.url.is_empty(), dl_btn).clicked() {
                        self.start_download();
                    }
                });

                ui.add_space(10.0);

                // Options row
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Save to").size(12.0).color(c.text_muted),
                    );
                    let display = Self::truncate_path(&self.output_path, 36);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(&display)
                                    .size(12.0)
                                    .color(c.text_secondary),
                            )
                            .frame(false),
                        )
                        .on_hover_text(&self.output_path)
                        .clicked()
                    {
                        self.select_output_directory();
                    }

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);

                    let turbo_color = if self.is_advanced { c.warning } else { c.text_muted };
                    ui.checkbox(&mut self.is_advanced, "");
                    ui.label(egui::RichText::new("Turbo").size(12.0).color(turbo_color))
                        .on_hover_text("Multi-connection parallel download");

                    ui.add_space(6.0);

                    let verify_color = if self.verify_iso { c.purple } else { c.text_muted };
                    ui.checkbox(&mut self.verify_iso, "");
                    ui.label(egui::RichText::new("Verify").size(12.0).color(verify_color))
                        .on_hover_text("Calculate SHA256 checksum");
                });

                // SHA256 row
                if self.verify_iso || !self.expected_sha256.is_empty() {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("SHA256").size(11.0).color(c.text_muted));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.expected_sha256)
                                .hint_text("Expected checksum (optional)")
                                .desired_width(ui.available_width())
                                .font(egui::TextStyle::Monospace),
                        );
                    });
                }
            });

        if let Some(ref error) = self.validation_error {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("⚠ {error}"))
                    .size(12.0)
                    .color(c.error),
            );
        }
    }

    // ========================================================================
    // Download Cards
    // ========================================================================

    fn render_progress_bar(
        ui: &mut egui::Ui,
        progress: f32,
        color: egui::Color32,
        animation_phase: f32,
    ) {
        let height = 3.0;
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
        let painter = ui.painter_at(rect);

        // Track
        let track_color =
            egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 30);
        painter.rect_filled(rect, height / 2.0, track_color);

        // Fill
        let fill_w = (rect.width() * progress.clamp(0.0, 1.0)).max(0.0);
        if fill_w > 0.0 {
            let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, height));
            painter.rect_filled(fill_rect, height / 2.0, color);

            // Shimmer
            if progress > 0.05 && progress < 0.95 {
                let shim_x = rect.min.x + animation_phase * fill_w;
                let shim_w = (fill_w * 0.25).min(36.0);
                if shim_x + shim_w < rect.min.x + fill_w {
                    let shim_rect = egui::Rect::from_min_size(
                        egui::pos2(shim_x, rect.min.y),
                        egui::vec2(shim_w, height),
                    );
                    painter.rect_filled(
                        shim_rect,
                        height / 2.0,
                        egui::Color32::from_white_alpha(70),
                    );
                }
            }
        }
    }

    /// Render a 44×16pt mini sparkline from a slice of bytes/s readings.
    fn render_sparkline(ui: &mut egui::Ui, history: &[f32], color: egui::Color32) {
        if history.len() < 2 {
            return;
        }
        let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 16.0), egui::Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let painter = ui.painter_at(rect);
        let max_val = history.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        if max_val <= 0.0 {
            return;
        }

        // Background
        painter.rect_filled(
            rect,
            3.0,
            egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 15),
        );

        // Line path
        let n = history.len();
        let step = rect.width() / (n as f32 - 1.0);
        let points: Vec<egui::Pos2> = history
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let x = rect.min.x + i as f32 * step;
                let y = rect.max.y - (v / max_val) * rect.height();
                egui::pos2(x, y.max(rect.min.y + 1.0))
            })
            .collect();

        for w in points.windows(2) {
            painter.line_segment(
                [w[0], w[1]],
                egui::Stroke::new(1.5, color),
            );
        }

        // Gradient fill under the line (translucent)
        let fill_color = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 25);
        let mut fill_pts = points.clone();
        fill_pts.push(egui::pos2(rect.max.x, rect.max.y));
        fill_pts.push(egui::pos2(rect.min.x, rect.max.y));
        painter.add(egui::Shape::convex_polygon(fill_pts, fill_color, egui::Stroke::NONE));
    }

    fn render_type_badge(ui: &mut egui::Ui, label: &str, color: egui::Color32) {
        egui::Frame::new()
            .fill(egui::Color32::from_rgba_unmultiplied(
                color.r(),
                color.g(),
                color.b(),
                28,
            ))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(5, 2))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(label).size(9.0).color(color).strong());
            });
    }

    fn render_download_card(
        &mut self,
        ui: &mut egui::Ui,
        download: &DownloadItem,
        c: &Colors,
    ) -> Option<(u64, &'static str)> {
        let mut action: Option<(u64, &'static str)> = None;
        let item_width = ui.available_width();
        let status_color = c.status_color(&download.status);

        egui::Frame::new()
            .fill(c.card)
            .corner_radius(10.0)
            .inner_margin(egui::Margin::same(14))
            .stroke(egui::Stroke::new(0.5, c.border))
            .shadow(egui::epaint::Shadow {
                offset: [0, 1],
                blur: 6,
                spread: 0,
                color: egui::Color32::from_black_alpha(if c.is_dark { 35 } else { 10 }),
            })
            .show(ui, |ui| {
                ui.set_width(item_width - 28.0);

                // Header row: status dot + filename + badges + actions
                ui.horizontal(|ui| {
                    // Status dot
                    let dot_size = 8.0;
                    let (dot_rect, _) = ui.allocate_exact_size(
                        egui::vec2(dot_size + 6.0, dot_size),
                        egui::Sense::hover(),
                    );
                    ui.painter_at(dot_rect).circle_filled(
                        dot_rect.center(),
                        dot_size / 2.0,
                        status_color,
                    );

                    // Filename
                    let max_chars = ((item_width - 320.0) / 7.5).max(20.0) as usize;
                    let display_name = if download.filename.len() > max_chars {
                        format!("{}…", &download.filename[..max_chars])
                    } else {
                        download.filename.clone()
                    };
                    ui.label(
                        egui::RichText::new(&display_name)
                            .size(14.0)
                            .strong()
                            .color(c.text_primary),
                    );

                    // Type badges
                    if download.is_torrent {
                        ui.add_space(2.0);
                        Self::render_type_badge(ui, "TORRENT", c.success);
                    } else if download.is_iso {
                        ui.add_space(2.0);
                        Self::render_type_badge(ui, "ISO", c.purple);
                    }
                    if download.is_advanced && !download.is_torrent {
                        ui.add_space(2.0);
                        Self::render_type_badge(ui, "TURBO", c.warning);
                    }

                    // Right side: status label + action buttons
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(download.status.label())
                                .size(11.0)
                                .color(status_color),
                        );
                        ui.add_space(8.0);

                        match download.status {
                            DownloadStatus::Downloading | DownloadStatus::Verifying => {
                                if ui.small_button("Cancel").clicked() {
                                    action = Some((download.id, "cancel"));
                                }
                            }
                            DownloadStatus::Completed => {
                                if ui.small_button("Remove").clicked() {
                                    action = Some((download.id, "remove"));
                                }
                                if ui.small_button("Folder").clicked() {
                                    action = Some((download.id, "open"));
                                }
                                if ui.small_button("Open").clicked() {
                                    action = Some((download.id, "open_file"));
                                }
                            }
                            DownloadStatus::Failed | DownloadStatus::Cancelled => {
                                if ui.small_button("Remove").clicked() {
                                    action = Some((download.id, "remove"));
                                }
                                if ui.small_button("Retry").clicked() {
                                    action = Some((download.id, "retry"));
                                }
                            }
                            DownloadStatus::Pending => {
                                if ui.small_button("Remove").clicked() {
                                    action = Some((download.id, "remove"));
                                }
                            }
                        }

                        if ui.small_button("Copy URL").clicked() {
                            action = Some((download.id, "copy_url"));
                        }
                    });
                });

                // URL row
                ui.add_space(2.0);
                let url_display = if download.url.len() > 80 {
                    format!("{}…", &download.url[..80])
                } else {
                    download.url.clone()
                };
                ui.label(
                    egui::RichText::new(&url_display).size(11.0).color(c.text_muted),
                );

                // Active: progress bar + stats + sparkline
                if matches!(
                    download.status,
                    DownloadStatus::Downloading | DownloadStatus::Verifying
                ) {
                    ui.add_space(10.0);
                    Self::render_progress_bar(
                        ui,
                        download.progress,
                        status_color,
                        self.animation_phase,
                    );
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{:.1}%", download.progress * 100.0))
                                .size(11.0)
                                .strong()
                                .color(status_color),
                        );

                        // Sparkline (only when speed history available)
                        if download.speed_history.len() >= 2 {
                            ui.add_space(8.0);
                            Self::render_sparkline(ui, &download.speed_history, c.accent);
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if download.status == DownloadStatus::Verifying {
                                ui.label(
                                    egui::RichText::new("Calculating SHA256…")
                                        .size(11.0)
                                        .color(c.purple),
                                );
                            } else {
                                if download.is_advanced && !download.is_torrent {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{}x connections",
                                            download.connections
                                        ))
                                        .size(11.0)
                                        .color(c.warning),
                                    );
                                    ui.add_space(10.0);
                                }
                                if !download.eta.is_empty() {
                                    ui.label(
                                        egui::RichText::new(format!("ETA {}", download.eta))
                                            .size(11.0)
                                            .color(c.text_muted),
                                    );
                                    ui.add_space(10.0);
                                }
                                if !download.speed.is_empty() {
                                    ui.label(
                                        egui::RichText::new(&download.speed)
                                            .size(11.0)
                                            .color(c.accent),
                                    );
                                }
                            }
                        });
                    });
                }

                // Completed: size + SHA256
                if download.status == DownloadStatus::Completed {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if !download.total_size.is_empty() {
                            ui.label(
                                egui::RichText::new(&download.total_size)
                                    .size(11.0)
                                    .color(c.text_muted),
                            );
                        }
                        if let Some(ref sha) = download.sha256 {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("Copy SHA256").clicked() {
                                        action = Some((download.id, "copy_sha"));
                                    }
                                    let short = if sha.len() > 16 { &sha[..16] } else { sha };
                                    ui.label(
                                        egui::RichText::new(format!("SHA256: {short}…"))
                                            .size(10.0)
                                            .color(c.success),
                                    );
                                },
                            );
                        }
                    });
                }

                // Error
                if let Some(ref error) = download.error {
                    ui.add_space(4.0);
                    let display = if error.len() > 90 {
                        format!("{}…", &error[..90])
                    } else {
                        error.clone()
                    };
                    ui.label(
                        egui::RichText::new(format!("Error: {display}"))
                            .size(11.0)
                            .color(c.error),
                    );
                }
            });

        action
    }

    // ========================================================================
    // History Tab
    // ========================================================================

    fn render_history_tab(&mut self, ui: &mut egui::Ui, c: &Colors) {
        // Reload button at the top
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Download History")
                    .size(16.0)
                    .strong()
                    .color(c.text_primary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Reload").clicked() {
                    let h = DownloadHistory::load();
                    self.history_entries = h.entries().to_vec();
                    self.history_entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                }
            });
        });
        ui.add_space(10.0);

        if self.history_entries.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.label(egui::RichText::new("No history yet").size(16.0).color(c.text_muted));
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Completed downloads will appear here")
                        .size(12.0)
                        .color(c.text_muted),
                );
            });
            return;
        }

        let mut redownload_url: Option<String> = None;
        let item_width = ui.available_width();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in &self.history_entries {
                    let status_color = match entry.status {
                        kget::queue::EntryStatus::Completed => c.success,
                        kget::queue::EntryStatus::Failed => c.error,
                        kget::queue::EntryStatus::Cancelled => c.warning,
                    };

                    egui::Frame::new()
                        .fill(c.card)
                        .corner_radius(10.0)
                        .inner_margin(egui::Margin::same(12))
                        .stroke(egui::Stroke::new(0.5, c.border))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 1],
                            blur: 4,
                            spread: 0,
                            color: egui::Color32::from_black_alpha(if c.is_dark { 30 } else { 8 }),
                        })
                        .show(ui, |ui| {
                            ui.set_width(item_width - 28.0);

                            ui.horizontal(|ui| {
                                // Status dot
                                let (dot_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(14.0, 8.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter_at(dot_rect).circle_filled(
                                    dot_rect.center(),
                                    4.0,
                                    status_color,
                                );

                                // Filename
                                let max_chars =
                                    ((item_width - 280.0) / 7.5).max(20.0) as usize;
                                let display_name = if entry.filename.len() > max_chars {
                                    format!("{}…", &entry.filename[..max_chars])
                                } else {
                                    entry.filename.clone()
                                };
                                ui.label(
                                    egui::RichText::new(&display_name)
                                        .size(13.0)
                                        .strong()
                                        .color(c.text_primary),
                                );

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        // Re-download button
                                        if ui
                                            .small_button(
                                                egui::RichText::new("↓ Re-download")
                                                    .color(c.accent),
                                            )
                                            .on_hover_text(&entry.url)
                                            .clicked()
                                        {
                                            redownload_url = Some(entry.url.clone());
                                        }

                                        // Copy URL
                                        if ui.small_button("Copy URL").clicked() {
                                            Self::copy_to_clipboard(&entry.url);
                                        }

                                        ui.add_space(8.0);

                                        // Status badge
                                        ui.label(
                                            egui::RichText::new(entry.status.to_string())
                                                .size(11.0)
                                                .color(status_color),
                                        );
                                    },
                                );
                            });

                            // URL + date row
                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                let url_trunc = if entry.url.len() > 70 {
                                    format!("{}…", &entry.url[..70])
                                } else {
                                    entry.url.clone()
                                };
                                ui.label(
                                    egui::RichText::new(&url_trunc)
                                        .size(10.0)
                                        .color(c.text_muted),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(
                                                entry.created_at_display(),
                                            )
                                            .size(10.0)
                                            .color(c.text_muted),
                                        );
                                        if let Some(bytes) = entry.bytes_total {
                                            ui.label(
                                                egui::RichText::new(format_bytes(bytes))
                                                    .size(10.0)
                                                    .color(c.text_secondary),
                                            );
                                            ui.add_space(8.0);
                                        }
                                    },
                                );
                            });

                            // Error row (for failed entries)
                            if let Some(ref err) = entry.error {
                                ui.add_space(2.0);
                                let short = if err.len() > 80 {
                                    format!("{}…", &err[..80])
                                } else {
                                    err.clone()
                                };
                                ui.label(
                                    egui::RichText::new(format!("Error: {short}"))
                                        .size(10.0)
                                        .color(c.error),
                                );
                            }
                        });

                    ui.add_space(6.0);
                }
            });

        // Handle re-download after borrow ends
        if let Some(url) = redownload_url {
            self.url = url;
            self.sidebar_tab = SidebarTab::Downloads;
            self.start_download();
        }
    }

    // ========================================================================
    // Empty State
    // ========================================================================

    fn render_empty_state(&self, ui: &mut egui::Ui, c: &Colors) {
        ui.vertical_centered(|ui| {
            ui.add_space(70.0);

            if let Some(texture) = &self.logo_texture {
                ui.add(
                    egui::Image::new(texture)
                        .fit_to_exact_size(egui::vec2(64.0, 64.0))
                        .tint(egui::Color32::from_white_alpha(if c.is_dark { 55 } else { 100 })),
                );
            } else {
                ui.label(egui::RichText::new("↓").size(48.0).color(c.text_muted));
            }

            ui.add_space(20.0);
            ui.label(
                egui::RichText::new("No Downloads")
                    .size(20.0)
                    .strong()
                    .color(c.text_secondary),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Paste a URL above or drag & drop one here")
                    .size(13.0)
                    .color(c.text_muted),
            );
            ui.add_space(28.0);

            // Protocol chips
            let protocols = [
                ("HTTP", "Web"),
                ("FTP", "Files"),
                ("WebDAV", "NAS"),
                ("Torrent", "Magnets"),
                ("yt-dlp", "Video"),
            ];
            ui.horizontal(|ui| {
                let chips_w = 390.0;
                ui.add_space(((ui.available_width() - chips_w) / 2.0).max(0.0));
                for (proto, label) in protocols {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgba_unmultiplied(
                            c.text_muted.r(),
                            c.text_muted.g(),
                            c.text_muted.b(),
                            18,
                        ))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new(proto)
                                        .size(11.0)
                                        .color(c.text_muted)
                                        .strong(),
                                );
                                ui.label(
                                    egui::RichText::new(label).size(9.0).color(c.text_muted),
                                );
                            });
                        });
                    ui.add_space(6.0);
                }
            });
        });
    }

    // ========================================================================
    // Drag-and-Drop overlay
    // ========================================================================

    fn render_drag_overlay(ui: &mut egui::Ui, c: &Colors) {
        let rect = ui.max_rect();
        let painter = ui.painter_at(rect);
        painter.rect_filled(
            rect,
            12.0,
            egui::Color32::from_rgba_unmultiplied(
                c.accent.r(),
                c.accent.g(),
                c.accent.b(),
                35,
            ),
        );
        painter.rect_stroke(rect.shrink(4.0), 12.0, egui::Stroke::new(2.0, c.accent), egui::StrokeKind::Middle);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Drop URL to download",
            egui::FontId::proportional(18.0),
            c.accent,
        );
    }

    // ========================================================================
    // Status Bar
    // ========================================================================

    fn render_status_bar(&self, ui: &mut egui::Ui, c: &Colors) {
        let active = self.downloads.iter().filter(|d| d.status.is_active()).count();
        let completed = self
            .downloads
            .iter()
            .filter(|d| d.status == DownloadStatus::Completed)
            .count();

        if active > 0 {
            ui.label(
                egui::RichText::new(format!("{active} downloading"))
                    .size(11.0)
                    .color(c.accent),
            );
            ui.label(egui::RichText::new("·").size(11.0).color(c.text_muted));
        }
        ui.label(
            egui::RichText::new(format!("{completed} completed"))
                .size(11.0)
                .color(if completed > 0 { c.success } else { c.text_muted }),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(&self.status_text)
                    .size(11.0)
                    .color(c.text_muted),
            );
        });
    }
}

// ============================================================================
// Small formatting helper
// ============================================================================

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.0} KB", bytes as f64 / 1_024.0)
    } else {
        format!("{bytes} B")
    }
}

// ============================================================================
// eframe::App
// ============================================================================

impl eframe::App for KGetGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_status_updates();
        self.animation_phase = (self.animation_phase + 0.018) % 1.0;

        let c = Colors::for_mode(self.dark_mode);
        self.apply_visuals(ctx, &c);

        // Clipboard monitor (rate-limited inside)
        self.check_clipboard();

        // Drag-and-drop: detect hover state
        let hovered = ctx.input(|i| !i.raw.hovered_files.is_empty());
        self.drag_over = hovered;

        // Drag-and-drop: handle dropped content
        ctx.input(|i| {
            for file in &i.raw.dropped_files {
                // Try bytes (URL dragged from browser)
                if let Some(ref bytes) = file.bytes {
                    if let Ok(text) = std::str::from_utf8(bytes) {
                        let text = text.trim().to_string();
                        if Self::is_downloadable_url(&text) {
                            self.url = text;
                        }
                    }
                }
                // Try file path (e.g. .webloc / .url shortcut files)
                if self.url.is_empty() {
                    if let Some(ref path) = file.path {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            for line in content.lines() {
                                let line = line.trim();
                                if Self::is_downloadable_url(line) {
                                    self.url = line.to_string();
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        });

        // Bottom status bar
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(30.0)
            .frame(
                egui::Frame::new()
                    .fill(c.card)
                    .inner_margin(egui::Margin::symmetric(16, 7))
                    .stroke(egui::Stroke::new(0.5, c.border)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    self.render_status_bar(ui, &c);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self
                            .downloads
                            .iter()
                            .any(|d| d.status == DownloadStatus::Completed)
                        {
                            if ui.small_button("Clear Completed").clicked() {
                                self.clear_completed();
                            }
                        }
                    });
                });
            });

        // Left sidebar
        egui::SidePanel::left("sidebar")
            .exact_width(180.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(c.sidebar_bg)
                    .stroke(egui::Stroke::new(0.5, c.border)),
            )
            .show(ctx, |ui| {
                self.render_sidebar(ui, &c);
            });

        // Main content
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(c.bg)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                // Show URL bar + clipboard banner only on downloads tab
                if self.sidebar_tab == SidebarTab::Downloads {
                    self.render_url_bar(ui, &c);
                    ui.add_space(8.0);

                    // Clipboard banner
                    let banner = self.clipboard_banner.clone();
                    if banner.is_some() {
                        self.render_clipboard_banner(ui, &c);
                    }

                    ui.add_space(4.0);

                    let filtered = self.filtered_downloads();

                    if self.downloads.is_empty() {
                        self.render_empty_state(ui, &c);
                    } else if filtered.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(60.0);
                            ui.label(
                                egui::RichText::new("No downloads in this category")
                                    .size(14.0)
                                    .color(c.text_muted),
                            );
                        });
                    } else {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let mut actions: Vec<(u64, &str)> = Vec::new();

                                for download in &filtered {
                                    if let Some(act) =
                                        self.render_download_card(ui, download, &c)
                                    {
                                        actions.push(act);
                                    }
                                    ui.add_space(8.0);
                                }

                                for (id, action) in actions {
                                    match action {
                                        "cancel" => self.cancel_download(id),
                                        "remove" => self.remove_download(id),
                                        "open" => {
                                            if let Some(d) =
                                                self.downloads.iter().find(|d| d.id == id)
                                            {
                                                self.open_download_folder(&d.output_path.clone());
                                            }
                                        }
                                        "open_file" => {
                                            if let Some(d) =
                                                self.downloads.iter().find(|d| d.id == id)
                                            {
                                                self.open_download_file(&d.output_path.clone());
                                            }
                                        }
                                        "copy_url" => {
                                            if let Some(d) =
                                                self.downloads.iter().find(|d| d.id == id)
                                            {
                                                Self::copy_to_clipboard(&d.url.clone());
                                            }
                                        }
                                        "copy_sha" => {
                                            if let Some(d) =
                                                self.downloads.iter().find(|d| d.id == id)
                                            {
                                                if let Some(sha) = d.sha256.clone() {
                                                    Self::copy_to_clipboard(&sha);
                                                }
                                            }
                                        }
                                        "retry" => self.retry_download(id),
                                        _ => {}
                                    }
                                }
                            });
                    }

                    // Drag-and-drop overlay (drawn on top)
                    if self.drag_over {
                        Self::render_drag_overlay(ui, &c);
                    }
                } else {
                    // History tab
                    self.render_history_tab(ui, &c);
                }
            });

        if self.downloads.iter().any(|d| d.status.is_active()) || self.drag_over {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        // Slower repaint for clipboard polling even when idle
        ctx.request_repaint_after(std::time::Duration::from_millis(1500));
    }
}
