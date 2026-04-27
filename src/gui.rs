use eframe::egui;
use kget::app::{DownloadCommand, WorkerToGuiMessage};
use rfd::FileDialog;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

// ============================================================================
// Color Constants
// ============================================================================

mod colors {
    use eframe::egui::Color32;

    pub const BG_DARK: Color32 = Color32::from_rgb(18, 18, 24);
    pub const BG_CARD: Color32 = Color32::from_rgb(28, 28, 38);
    pub const BG_INPUT: Color32 = Color32::from_rgb(35, 35, 48);
    pub const BG_HOVER: Color32 = Color32::from_rgb(45, 45, 60);

    pub const ACCENT_GREEN: Color32 = Color32::from_rgb(46, 204, 113);
    pub const ACCENT_BLUE: Color32 = Color32::from_rgb(52, 152, 219);
    pub const ACCENT_ORANGE: Color32 = Color32::from_rgb(243, 156, 18);
    pub const ACCENT_PURPLE: Color32 = Color32::from_rgb(155, 89, 182);
    pub const ACCENT_RED: Color32 = Color32::from_rgb(231, 76, 60);
    pub const ACCENT_TORRENT: Color32 = Color32::from_rgb(76, 175, 80);

    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(236, 240, 241);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(149, 165, 166);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(99, 110, 114);

    pub const BORDER: Color32 = Color32::from_rgb(52, 52, 68);
}

// ============================================================================
// Download Status
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
    fn color(&self) -> egui::Color32 {
        match self {
            DownloadStatus::Pending => colors::TEXT_MUTED,
            DownloadStatus::Downloading => colors::ACCENT_BLUE,
            DownloadStatus::Verifying => colors::ACCENT_PURPLE,
            DownloadStatus::Completed => colors::ACCENT_GREEN,
            DownloadStatus::Failed => colors::ACCENT_RED,
            DownloadStatus::Cancelled => colors::ACCENT_ORANGE,
        }
    }

    fn icon(&self) -> &str {
        match self {
            DownloadStatus::Pending => "⏳",
            DownloadStatus::Downloading => "⬇",
            DownloadStatus::Verifying => "🔒",
            DownloadStatus::Completed => "✓",
            DownloadStatus::Failed => "✗",
            DownloadStatus::Cancelled => "⊘",
        }
    }

    fn label(&self) -> &str {
        match self {
            DownloadStatus::Pending => "Pending",
            DownloadStatus::Downloading => "Downloading",
            DownloadStatus::Verifying => "Verifying",
            DownloadStatus::Completed => "Completed",
            DownloadStatus::Failed => "Failed",
            DownloadStatus::Cancelled => "Cancelled",
        }
    }
}

// ============================================================================
// Download Item
// ============================================================================

#[derive(Debug, Clone)]
pub struct DownloadItem {
    pub id: u64,
    pub url: String,
    pub output_path: String,
    pub filename: String,
    pub status: DownloadStatus,
    pub progress: f32,
    pub speed: String,
    pub eta: String,
    pub total_size: String,
    pub is_advanced: bool,
    pub is_iso: bool,
    pub is_torrent: bool,
    pub verify_integrity: bool,
    pub error: Option<String>,
    pub sha256: Option<String>,
    pub connections: u8,
    pub start_time: Option<Instant>,
}

impl DownloadItem {
    fn new(id: u64, url: String, output_path: String, is_advanced: bool, verify_iso: bool) -> Self {
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
            output_path: output_path.clone(),
            filename,
            status: DownloadStatus::Pending,
            progress: 0.0,
            speed: String::new(),
            eta: String::new(),
            total_size: String::new(),
            is_advanced,
            is_iso,
            is_torrent,
            verify_integrity: verify_iso || is_iso,
            error: None,
            sha256: None,
            connections: if is_advanced { 4 } else { 1 },
            start_time: None,
        }
    }

    fn get_type_badge(&self) -> Option<(&str, egui::Color32)> {
        if self.is_torrent {
            Some(("TORRENT", colors::ACCENT_TORRENT))
        } else if self.is_iso {
            Some(("ISO", colors::ACCENT_PURPLE))
        } else if self.is_advanced {
            Some(("TURBO", colors::ACCENT_ORANGE))
        } else {
            None
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

    // Downloads
    downloads: Vec<DownloadItem>,
    active_download_id: Option<u64>,
    next_download_id: u64,

    // UI State
    status_text: String,
    validation_error: Option<String>,
    animation_phase: f32,

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

        // Configure fonts
        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (egui::TextStyle::Heading, egui::FontId::proportional(22.0)),
            (egui::TextStyle::Body, egui::FontId::proportional(14.0)),
            (egui::TextStyle::Button, egui::FontId::proportional(13.0)),
            (egui::TextStyle::Monospace, egui::FontId::monospace(12.0)),
            (egui::TextStyle::Small, egui::FontId::proportional(11.0)),
        ]
        .into();

        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(16);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);

        ctx.set_style(style);

        // Load logo texture
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
            downloads: Vec::new(),
            active_download_id: None,
            next_download_id: 1,
            status_text: "Ready".into(),
            validation_error: None,
            animation_phase: 0.0,
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
                            // Only update if progress increases
                            if p > download.progress {
                                download.progress = p;
                            }
                            download.status = DownloadStatus::Downloading;

                            if let Some(start) = download.start_time {
                                let elapsed = start.elapsed().as_secs_f32();
                                if elapsed > 0.5 && p > 0.0 {
                                    let speed = p / elapsed;
                                    if p < 1.0 && speed > 0.0 {
                                        let remaining = (1.0 - p) / speed;
                                        download.eta = Self::format_time(remaining);
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

                            // Parse speed if available
                            if let Some(speed_str) = Self::extract_speed(&s) {
                                download.speed = speed_str;
                            }

                            // Parse size if available
                            if let Some(size_str) = Self::extract_size(&s) {
                                download.total_size = size_str;
                            }
                        }
                        WorkerToGuiMessage::Completed(s_msg) => {
                            download.status = DownloadStatus::Completed;
                            download.progress = 1.0;
                            self.status_text = "Download completed!".into();
                            self.active_download_id = None;
                            Self::send_native_notification(
                                "KGet download complete",
                                &download.filename,
                            );

                            if s_msg.contains("SHA256") {
                                download.sha256 = Some(s_msg.clone());
                            }
                        }
                        WorkerToGuiMessage::Error(err_msg) => {
                            download.status = DownloadStatus::Failed;
                            download.error = Some(err_msg.clone());
                            self.status_text = format!("Error: {}", err_msg);
                            self.active_download_id = None;
                            Self::send_native_notification("KGet download failed", &err_msg);
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
                let end = idx + pattern.len();
                let speed = s[start..end].trim().to_string();
                if !speed.is_empty() {
                    return Some(speed);
                }
            }
        }
        None
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
            format!("{}s", secs)
        } else if secs < 3600 {
            format!("{}m {}s", secs / 60, secs % 60)
        } else {
            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
        }
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
            || url_lower.starts_with("magnet:?");

        if !supported {
            return Err("Supported: http://, https://, ftp://, sftp://, magnet:".into());
        }

        if self.output_path.is_empty() {
            return Err("Select a destination folder".into());
        }

        let path = std::path::Path::new(&self.output_path);
        if !path.exists() {
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
        {
            let _ = std::process::Command::new("open").arg(&folder_path).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open")
                .arg(&folder_path)
                .spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer")
                .arg(&folder_path)
                .spawn();
        }
    }

    fn start_download(&mut self) {
        match self.validate_input() {
            Ok(()) => {
                self.validation_error = None;

                let is_magnet = self.url.to_lowercase().starts_with("magnet:?");
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
                    self.verify_iso,
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
                        verify_iso: self.verify_iso,
                    })
                    .ok();

                self.status_text = "Starting download...".into();
                self.url.clear();
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
                })
                .ok();
        }
    }

    // ========================================================================
    // UI Rendering
    // ========================================================================

    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Logo
            if let Some(texture) = &self.logo_texture {
                ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(40.0, 40.0)));
            } else {
                ui.label(
                    egui::RichText::new("⬇")
                        .size(28.0)
                        .color(colors::ACCENT_GREEN),
                );
            }

            ui.add_space(8.0);

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    egui::RichText::new("KGet")
                        .size(20.0)
                        .strong()
                        .color(colors::TEXT_PRIMARY),
                );
                ui.label(
                    egui::RichText::new("Modern Download Manager")
                        .size(11.0)
                        .color(colors::TEXT_MUTED),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let version = env!("CARGO_PKG_VERSION");
                ui.label(
                    egui::RichText::new(format!("v{}", version))
                        .size(11.0)
                        .color(colors::TEXT_MUTED),
                );
            });
        });
    }

    fn render_input_section(&mut self, ui: &mut egui::Ui) {
        let available_width = ui.available_width();

        egui::Frame::new()
            .fill(colors::BG_CARD)
            .corner_radius(12.0)
            .inner_margin(egui::Margin::same(16))
            .stroke(egui::Stroke::new(1.0, colors::BORDER))
            .show(ui, |ui| {
                ui.set_width(available_width - 32.0);

                // URL Input Row
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("🔗")
                            .size(14.0)
                            .color(colors::TEXT_SECONDARY),
                    );

                    // Calculate available width for URL input
                    let button_space = 150.0;
                    let url_width = (ui.available_width() - button_space).max(200.0);

                    let text_edit = egui::TextEdit::singleline(&mut self.url)
                        .hint_text("Enter URL or paste link...")
                        .desired_width(url_width)
                        .font(egui::TextStyle::Body);

                    let response = ui.add(text_edit);

                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.start_download();
                    }

                    // Paste button
                    if ui
                        .add(egui::Button::new("📋").min_size(egui::vec2(32.0, 28.0)))
                        .on_hover_text("Paste from clipboard")
                        .clicked()
                    {
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            if let Ok(text) = cb.get_text() {
                                self.url = text;
                            }
                        }
                    }

                    // Download button
                    let download_btn = egui::Button::new(
                        egui::RichText::new("⬇ Download").color(egui::Color32::WHITE),
                    )
                    .fill(colors::ACCENT_GREEN)
                    .min_size(egui::vec2(100.0, 28.0));

                    if ui.add_enabled(!self.url.is_empty(), download_btn).clicked() {
                        self.start_download();
                    }
                });

                ui.add_space(10.0);

                // Options Row
                ui.horizontal(|ui| {
                    // Output folder
                    ui.label(
                        egui::RichText::new("📁")
                            .size(14.0)
                            .color(colors::TEXT_SECONDARY),
                    );

                    let folder_width = (ui.available_width() - 200.0).max(100.0);
                    let display_path = Self::truncate_path(&self.output_path, 40);

                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(&display_path)
                                    .size(12.0)
                                    .color(colors::TEXT_SECONDARY),
                            )
                            .frame(true)
                            .min_size(egui::vec2(folder_width.min(300.0), 24.0)),
                        )
                        .on_hover_text(&self.output_path)
                        .clicked()
                    {
                        self.select_output_directory();
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    // Turbo mode checkbox
                    ui.checkbox(&mut self.is_advanced, "");
                    ui.label(egui::RichText::new("⚡ Turbo").size(12.0).color(
                        if self.is_advanced {
                            colors::ACCENT_ORANGE
                        } else {
                            colors::TEXT_SECONDARY
                        },
                    ))
                    .on_hover_text("Multi-connection download");

                    ui.add_space(8.0);

                    // Verify ISO checkbox
                    ui.checkbox(&mut self.verify_iso, "");
                    ui.label(egui::RichText::new("🔒 Verify").size(12.0).color(
                        if self.verify_iso {
                            colors::ACCENT_PURPLE
                        } else {
                            colors::TEXT_SECONDARY
                        },
                    ))
                    .on_hover_text("Verify SHA256 for ISO files");
                });
            });

        // Validation error
        if let Some(ref error) = self.validation_error {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("⚠ {}", error))
                    .size(12.0)
                    .color(colors::ACCENT_RED),
            );
        }
    }

    fn truncate_path(path: &str, max_len: usize) -> String {
        if path.len() <= max_len {
            path.to_string()
        } else {
            let parts: Vec<&str> = path.split(std::path::MAIN_SEPARATOR).collect();
            if parts.len() > 2 {
                format!(
                    "...{}{}",
                    std::path::MAIN_SEPARATOR,
                    parts.last().unwrap_or(&"")
                )
            } else {
                format!("...{}", &path[path.len().saturating_sub(max_len - 3)..])
            }
        }
    }

    fn render_download_item(
        &mut self,
        ui: &mut egui::Ui,
        download: &DownloadItem,
    ) -> Option<(u64, &'static str)> {
        let mut action: Option<(u64, &'static str)> = None;
        let item_width = ui.available_width();

        egui::Frame::new()
            .fill(colors::BG_CARD)
            .corner_radius(10.0)
            .inner_margin(egui::Margin::same(14))
            .stroke(egui::Stroke::new(1.0, colors::BORDER))
            .show(ui, |ui| {
                ui.set_width(item_width - 28.0);

                // Header row
                ui.horizontal(|ui| {
                    // Status indicator
                    ui.label(
                        egui::RichText::new(download.status.icon())
                            .size(16.0)
                            .color(download.status.color()),
                    );

                    ui.add_space(4.0);

                    // Filename (with truncation based on available width)
                    let max_name_len = ((item_width - 250.0) / 8.0).max(15.0) as usize;
                    let display_name = if download.filename.len() > max_name_len {
                        format!(
                            "{}...",
                            &download.filename[..max_name_len.min(download.filename.len())]
                        )
                    } else {
                        download.filename.clone()
                    };

                    ui.label(
                        egui::RichText::new(&display_name)
                            .size(14.0)
                            .strong()
                            .color(colors::TEXT_PRIMARY),
                    );

                    // Type badge
                    if let Some((badge_text, badge_color)) = download.get_type_badge() {
                        ui.add_space(6.0);
                        egui::Frame::new()
                            .fill(badge_color)
                            .corner_radius(4.0)
                            .inner_margin(egui::Margin::symmetric(6, 2))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(badge_text)
                                        .size(9.0)
                                        .color(egui::Color32::WHITE)
                                        .strong(),
                                );
                            });
                    }

                    // Right side: status and actions
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match download.status {
                            DownloadStatus::Downloading | DownloadStatus::Verifying => {
                                if ui
                                    .add(egui::Button::new("✕").min_size(egui::vec2(24.0, 24.0)))
                                    .on_hover_text("Cancel")
                                    .clicked()
                                {
                                    action = Some((download.id, "cancel"));
                                }
                            }
                            DownloadStatus::Completed => {
                                if ui
                                    .add(egui::Button::new("📂").min_size(egui::vec2(24.0, 24.0)))
                                    .on_hover_text("Open folder")
                                    .clicked()
                                {
                                    action = Some((download.id, "open"));
                                }
                                if ui
                                    .add(egui::Button::new("🗑").min_size(egui::vec2(24.0, 24.0)))
                                    .on_hover_text("Remove")
                                    .clicked()
                                {
                                    action = Some((download.id, "remove"));
                                }
                            }
                            DownloadStatus::Failed | DownloadStatus::Cancelled => {
                                if ui
                                    .add(egui::Button::new("↻").min_size(egui::vec2(24.0, 24.0)))
                                    .on_hover_text("Retry")
                                    .clicked()
                                {
                                    action = Some((download.id, "retry"));
                                }
                                if ui
                                    .add(egui::Button::new("🗑").min_size(egui::vec2(24.0, 24.0)))
                                    .on_hover_text("Remove")
                                    .clicked()
                                {
                                    action = Some((download.id, "remove"));
                                }
                            }
                            DownloadStatus::Pending => {}
                        }

                        // Status label
                        ui.label(
                            egui::RichText::new(download.status.label())
                                .size(11.0)
                                .color(download.status.color()),
                        );
                    });
                });

                // Progress section (only for active downloads)
                if matches!(
                    download.status,
                    DownloadStatus::Downloading | DownloadStatus::Verifying
                ) {
                    ui.add_space(10.0);

                    // Progress bar
                    let progress_height = 8.0;
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), progress_height),
                        egui::Sense::hover(),
                    );

                    let painter = ui.painter();

                    // Background
                    painter.rect_filled(rect, 4.0, colors::BG_INPUT);

                    // Progress fill
                    let fill_width = rect.width() * download.progress;
                    let fill_rect =
                        egui::Rect::from_min_size(rect.min, egui::vec2(fill_width, rect.height()));

                    let color = if download.status == DownloadStatus::Verifying {
                        colors::ACCENT_PURPLE
                    } else if download.is_torrent {
                        colors::ACCENT_TORRENT
                    } else if download.is_advanced {
                        colors::ACCENT_ORANGE
                    } else {
                        colors::ACCENT_BLUE
                    };

                    painter.rect_filled(fill_rect, 4.0, color);

                    // Shimmer effect
                    if download.progress > 0.05 && download.progress < 0.95 {
                        let shimmer_x = fill_rect.left() + (self.animation_phase * fill_width);
                        let shimmer_width = 30.0_f32.min(fill_width * 0.2);
                        if shimmer_x + shimmer_width < fill_rect.right() {
                            let shimmer_rect = egui::Rect::from_min_size(
                                egui::pos2(shimmer_x, fill_rect.top()),
                                egui::vec2(shimmer_width, fill_rect.height()),
                            );
                            painter.rect_filled(
                                shimmer_rect,
                                4.0,
                                egui::Color32::from_white_alpha(40),
                            );
                        }
                    }

                    ui.add_space(8.0);

                    // Stats row
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{:.1}%", download.progress * 100.0))
                                .size(12.0)
                                .strong()
                                .color(color),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if download.status == DownloadStatus::Verifying {
                                ui.label(
                                    egui::RichText::new("🔒 Calculating SHA256...")
                                        .size(11.0)
                                        .color(colors::ACCENT_PURPLE),
                                );
                            } else {
                                if download.is_advanced && !download.is_torrent {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "⚡ {}x",
                                            download.connections
                                        ))
                                        .size(11.0)
                                        .color(colors::ACCENT_ORANGE),
                                    );
                                    ui.add_space(10.0);
                                }

                                if !download.eta.is_empty() {
                                    ui.label(
                                        egui::RichText::new(format!("⏱ {}", download.eta))
                                            .size(11.0)
                                            .color(colors::TEXT_SECONDARY),
                                    );
                                    ui.add_space(10.0);
                                }

                                if !download.speed.is_empty() {
                                    ui.label(
                                        egui::RichText::new(format!("↓ {}", download.speed))
                                            .size(11.0)
                                            .color(colors::ACCENT_BLUE),
                                    );
                                }
                            }
                        });
                    });
                }

                // Completed info
                if download.status == DownloadStatus::Completed {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if let Some(ref sha) = download.sha256 {
                            let short_sha = if sha.len() > 24 { &sha[..24] } else { sha };
                            ui.label(
                                egui::RichText::new(format!("✓ SHA256: {}...", short_sha))
                                    .size(10.0)
                                    .color(colors::ACCENT_GREEN),
                            );
                        }
                    });
                }

                // Error message
                if let Some(ref error) = download.error {
                    ui.add_space(4.0);
                    let display_error = if error.len() > 80 {
                        format!("{}...", &error[..80])
                    } else {
                        error.clone()
                    };
                    ui.label(
                        egui::RichText::new(format!("⚠ {}", display_error))
                            .size(11.0)
                            .color(colors::ACCENT_RED),
                    );
                }
            });

        action
    }

    fn render_empty_state(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);

            if let Some(texture) = &self.logo_texture {
                ui.add(
                    egui::Image::new(texture)
                        .fit_to_exact_size(egui::vec2(80.0, 80.0))
                        .tint(egui::Color32::from_white_alpha(60)),
                );
            }

            ui.add_space(20.0);

            ui.label(
                egui::RichText::new("No Downloads")
                    .size(20.0)
                    .color(colors::TEXT_SECONDARY),
            );

            ui.add_space(8.0);

            ui.label(
                egui::RichText::new("Paste a URL above to start downloading")
                    .size(13.0)
                    .color(colors::TEXT_MUTED),
            );

            ui.add_space(30.0);

            // Supported protocols
            ui.horizontal(|ui| {
                let center_offset = (ui.available_width() - 280.0) / 2.0;
                ui.add_space(center_offset.max(0.0));

                Self::render_protocol_badge(ui, "🌐", "HTTP/HTTPS");
                ui.add_space(16.0);
                Self::render_protocol_badge(ui, "💾", "FTP/SFTP");
                ui.add_space(16.0);
                Self::render_protocol_badge(ui, "🧲", "Magnet Links");
            });
        });
    }

    fn render_protocol_badge(ui: &mut egui::Ui, icon: &str, label: &str) {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(egui::RichText::new(icon).size(22.0));
            ui.label(
                egui::RichText::new(label)
                    .size(10.0)
                    .color(colors::TEXT_MUTED),
            );
        });
    }

    fn render_footer(&self, ui: &mut egui::Ui) {
        let active_count = self
            .downloads
            .iter()
            .filter(|d| {
                matches!(
                    d.status,
                    DownloadStatus::Downloading | DownloadStatus::Verifying
                )
            })
            .count();
        let completed_count = self
            .downloads
            .iter()
            .filter(|d| d.status == DownloadStatus::Completed)
            .count();

        ui.horizontal(|ui| {
            // Active downloads
            let active_color = if active_count > 0 {
                colors::ACCENT_BLUE
            } else {
                colors::TEXT_MUTED
            };
            ui.label(
                egui::RichText::new(format!("⬇ {} active", active_count))
                    .size(11.0)
                    .color(active_color),
            );

            ui.add_space(16.0);

            // Completed downloads
            let completed_color = if completed_count > 0 {
                colors::ACCENT_GREEN
            } else {
                colors::TEXT_MUTED
            };
            ui.label(
                egui::RichText::new(format!("✓ {} completed", completed_count))
                    .size(11.0)
                    .color(completed_color),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Status text
                ui.label(
                    egui::RichText::new(&self.status_text)
                        .size(11.0)
                        .color(colors::TEXT_MUTED),
                );
            });
        });
    }
}

// ============================================================================
// eframe::App Implementation
// ============================================================================

impl eframe::App for KGetGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_status_updates();

        // Update animation
        self.animation_phase = (self.animation_phase + 0.015) % 1.0;

        // Apply custom visuals
        let mut visuals = egui::Visuals::dark();
        visuals.widgets.noninteractive.bg_fill = colors::BG_INPUT;
        visuals.widgets.inactive.bg_fill = colors::BG_INPUT;
        visuals.widgets.hovered.bg_fill = colors::BG_HOVER;
        visuals.widgets.active.bg_fill = colors::ACCENT_BLUE;
        visuals.selection.bg_fill = colors::ACCENT_BLUE;
        visuals.window_fill = colors::BG_DARK;
        visuals.panel_fill = colors::BG_DARK;
        visuals.extreme_bg_color = colors::BG_CARD;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, colors::TEXT_PRIMARY);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, colors::TEXT_SECONDARY);
        ctx.set_visuals(visuals);

        // Footer panel
        egui::TopBottomPanel::bottom("footer")
            .exact_height(36.0)
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_CARD)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    self.render_footer(ui);

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

        // Main content
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(colors::BG_DARK)
                    .inner_margin(egui::Margin::same(20)),
            )
            .show(ctx, |ui| {
                // Header
                self.render_header(ui);

                ui.add_space(16.0);

                // Input section
                self.render_input_section(ui);

                ui.add_space(16.0);

                // Downloads list or empty state
                if self.downloads.is_empty() {
                    self.render_empty_state(ui);
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let downloads_snapshot: Vec<_> =
                                self.downloads.iter().cloned().collect();
                            let mut actions: Vec<(u64, &str)> = Vec::new();

                            for download in &downloads_snapshot {
                                if let Some(action) = self.render_download_item(ui, download) {
                                    actions.push(action);
                                }
                                ui.add_space(8.0);
                            }

                            // Process actions
                            for (id, action) in actions {
                                match action {
                                    "cancel" => self.cancel_download(id),
                                    "remove" => self.remove_download(id),
                                    "open" => {
                                        if let Some(d) = self.downloads.iter().find(|d| d.id == id)
                                        {
                                            self.open_download_folder(&d.output_path);
                                        }
                                    }
                                    "retry" => self.retry_download(id),
                                    _ => {}
                                }
                            }
                        });
                }
            });

        // Request repaint for animations
        if self.downloads.iter().any(|d| {
            matches!(
                d.status,
                DownloadStatus::Downloading | DownloadStatus::Verifying
            )
        }) {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}
