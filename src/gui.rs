use eframe::egui;
use std::sync::mpsc;
use std::path::PathBuf;
use rfd::FileDialog;

// Command sent from the GUI to the worker thread
pub enum DownloadCommand {
    Start(String, String), // url, output_path
    Cancel,
}

// Message sent from the worker thread back to the GUI
#[derive(Debug)]
pub enum WorkerToGuiMessage {
    Progress(f32),          // Progress value from 0.0 to 1.0
    StatusUpdate(String),   // General status message
    Completed(String),      // Successful completion message
    Error(String),          // Error message
}

pub struct KGetGui {
    url: String,
    output_path: String,
    
    status_text: String,
    current_progress: f32,
    is_downloading: bool,
    last_downloaded_file: Option<String>,
    show_file_dialog: bool,

    // To send commands to the worker thread
    download_tx: mpsc::Sender<DownloadCommand>,
    // To receive status updates from the worker thread
    status_rx: mpsc::Receiver<WorkerToGuiMessage>,
}

impl KGetGui {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        download_tx: mpsc::Sender<DownloadCommand>,
        status_rx: mpsc::Receiver<WorkerToGuiMessage>,
    ) -> Self {
        // Increase font sizes
        if let Some(ctx) = cc.egui_ctx.clone().into() {
            let mut style = (*ctx.style()).clone();
            style.text_styles = [
                (egui::TextStyle::Heading, egui::FontId::proportional(32.0)),
                (egui::TextStyle::Body, egui::FontId::proportional(22.0)),
                (egui::TextStyle::Button, egui::FontId::proportional(22.0)),
                (egui::TextStyle::Monospace, egui::FontId::monospace(20.0)),
                (egui::TextStyle::Small, egui::FontId::proportional(18.0)),
            ].into();
            ctx.set_style(style);
        }

        Self {
            url: String::new(),
            output_path: String::from(std::env::current_dir().unwrap_or_default().to_string_lossy()),
            status_text: "Ready. Enter a URL and output path to begin.".to_string(),
            current_progress: 0.0,
            is_downloading: false,
            last_downloaded_file: None,
            show_file_dialog: false,
            download_tx,
            status_rx,
        }
    }

    fn process_status_updates(&mut self) {
        // Process all messages from the worker thread
        while let Ok(msg) = self.status_rx.try_recv() {
            match msg {
                WorkerToGuiMessage::Progress(p) => {
                    self.current_progress = p;
                    // If we received progress but weren't in the downloading state, update
                    if !self.is_downloading && p > 0.0 && p < 1.0 {
                        self.is_downloading = true;
                    }
                    if self.is_downloading {
                        self.status_text = format!("Downloading: {:.1}%", p * 100.0);
                    }
                }
                WorkerToGuiMessage::StatusUpdate(s) => {
                    self.status_text = s;
                }
                WorkerToGuiMessage::Completed(s_msg) => {
                    self.status_text = format!("Completed: {}", s_msg);
                    self.current_progress = 1.0;
                    self.is_downloading = false;
                    // Save the last downloaded file for later use
                    self.last_downloaded_file = Some(s_msg);
                }
                WorkerToGuiMessage::Error(err_msg) => {
                    self.status_text = format!("Error: {}", err_msg);
                    // Keep the progress to show where it failed
                    self.is_downloading = false;
                }
            }
        }
    }

    fn validate_input(&self) -> Result<(), String> {
        // Basic input validation
        if self.url.is_empty() {
            return Err("URL cannot be empty".into());
        }

        // Check if the URL starts with a supported protocol
        if !self.url.starts_with("http://") && 
           !self.url.starts_with("https://") && 
           !self.url.starts_with("ftp://") && 
           !self.url.starts_with("sftp://") && 
           !self.url.starts_with("magnet:?") {
            return Err("URL must start with http://, https://, ftp://, sftp:// or magnet:?".into());
        }

        // Check if the output path is valid
        if self.output_path.is_empty() {
            return Err("Output path cannot be empty".into());
        }

        Ok(())
    }

    fn select_output_directory(&mut self) {
        if let Some(path) = FileDialog::new()
            .set_directory(std::path::Path::new(&self.output_path))
            .pick_folder() {
            self.output_path = path.to_string_lossy().to_string();
        }
    }

    fn select_output_file(&mut self) {
        // Try to extract a suggested filename from the URL
        let suggested_filename = if let Some(file_path) = url::Url::parse(&self.url).ok().and_then(|u| u.path_segments().and_then(|p| p.last().map(|s| s.to_string()))) {
            if !file_path.is_empty() {
                file_path
            } else {
                "download".to_string()
            }
        } else {
            "download".to_string()
        };

        if let Some(path) = FileDialog::new()
            .set_file_name(&suggested_filename)
            .set_directory(std::path::Path::new(&self.output_path))
            .save_file() {
            self.output_path = path.to_string_lossy().to_string();
        }
    }
}

impl eframe::App for KGetGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process all messages from the worker thread
        self.process_status_updates();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("KGet Downloader");
            ui.add_space(10.0);

            // URL input
            ui.horizontal(|ui| {
                ui.label("URL:");
                let url_field = ui.add_enabled(
                    !self.is_downloading, 
                    egui::TextEdit::singleline(&mut self.url)
                        .hint_text("Enter download URL (http://, https://, ftp://, sftp://, magnet:?)")
                        .desired_width(400.0)
                );
                if ui.add_enabled(!self.is_downloading && !self.url.is_empty(), egui::Button::new("✖")).clicked() {
                    self.url.clear();
                }
                if ui.add_enabled(!self.is_downloading, egui::Button::new("📋")).clicked() {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        if let Ok(text) = clipboard.get_text() {
                            self.url = text;
                        }
                    }
                }
                if self.url.is_empty() && !self.is_downloading {
                    url_field.request_focus();
                }
            });

            // Output path selection
            ui.horizontal(|ui| {
                ui.label("Output:");
                ui.add_enabled(
                    !self.is_downloading,
                    egui::TextEdit::singleline(&mut self.output_path)
                        .hint_text("Enter output path or click Browse")
                        .desired_width(300.0)
                );
                if ui.add_enabled(!self.is_downloading, egui::Button::new("📁 Browse")).clicked() {
                    self.select_output_directory();
                }
                if ui.add_enabled(!self.is_downloading, egui::Button::new("💾 Save As")).clicked() {
                    self.select_output_file();
                }
            });

            ui.add_space(10.0);

            // Progress bar and status
            ui.horizontal(|ui| {
                let progress_bar = egui::ProgressBar::new(self.current_progress)
                    .animate(self.is_downloading)
                    .show_percentage();
                ui.add_sized([200.0, 24.0], progress_bar);

                // Cancel button (available during download)
                if ui.add_enabled(self.is_downloading, egui::Button::new("❌ Cancel")).clicked() {
                    self.download_tx.send(DownloadCommand::Cancel).ok();
                    self.status_text = "Canceling download...".to_string();
                }

                // Download button (disabled during download)
                if ui.add_enabled(!self.is_downloading, egui::Button::new("⬇️ Download").fill(egui::Color32::from_rgb(0, 120, 210))).clicked() {
                    match self.validate_input() {
                        Ok(_) => {
                            match self.download_tx.send(DownloadCommand::Start(
                                self.url.clone(),
                                self.output_path.clone(),
                            )) {
                                Ok(_) => {
                                    self.status_text = "Starting download...".to_string();
                                    self.is_downloading = true;
                                    self.current_progress = 0.0;
                                }
                                Err(e) => {
                                    self.status_text = format!("Error sending command: {}", e);
                                }
                            }
                        },
                        Err(error_msg) => {
                            self.status_text = error_msg;
                        }
                    }
                }
            });
            
            ui.add_space(5.0);
            
            // Status text with color coding
            let status_color = if self.status_text.starts_with("Error:") {
                egui::Color32::from_rgb(200, 0, 0)
            } else if self.status_text.starts_with("Completed:") {
                egui::Color32::from_rgb(0, 150, 0)
            } else {
                ui.style().visuals.text_color()
            };
            
            ui.colored_label(status_color, &self.status_text);
            
            // Actions after download completion
            if let Some(file) = &self.last_downloaded_file {
                if self.current_progress >= 1.0 && !self.is_downloading {
                    ui.horizontal(|ui| {
                        if ui.button("📂 Open Folder").clicked() {
                            let path = PathBuf::from(&self.output_path);
                            if let Some(parent) = path.parent() {
                                #[cfg(target_os = "windows")]
                                {
                                    std::process::Command::new("explorer")
                                        .arg(parent)
                                        .spawn()
                                        .ok();
                                }
                                
                                #[cfg(target_os = "macos")]
                                {
                                    std::process::Command::new("open")
                                        .arg(parent)
                                        .spawn()
                                        .ok();
                                }
                                
                                #[cfg(target_os = "linux")]
                                {
                                    std::process::Command::new("xdg-open")
                                        .arg(parent)
                                        .spawn()
                                        .ok();
                                }
                            }
                        }
                        
                        ui.label(format!("File: {}", file));
                    });
                }
            }
            
            ui.add_space(10.0);
            
            // Show tips and additional information
            ui.horizontal(|ui| {
                ui.label("💡");
                ui.label("Tip: You can also use KGet from command line with 'kget <URL>'");
            });
        });

        // Keep the UI updating while the download is in progress
        if self.is_downloading {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}