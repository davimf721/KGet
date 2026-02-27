use eframe::egui;
use std::sync::mpsc;
use std::path::PathBuf;
use rfd::FileDialog;
use crate::gui_types::{DownloadCommand, WorkerToGuiMessage};

pub struct KGetGui {
    url: String,
    output_path: String,
    is_advanced: bool,
    verify_iso: bool,
    
    status_text: String,
    current_progress: f32,
    is_downloading: bool,
    last_downloaded_file: Option<String>,
    #[allow(dead_code)]
    show_file_dialog: bool,
    validation_error: Option<String>,
    
    // Logo texture
    logo_texture: Option<egui::TextureHandle>,

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
        let ctx = &cc.egui_ctx;

        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (egui::TextStyle::Heading, egui::FontId::proportional(28.0)),
            (egui::TextStyle::Body, egui::FontId::proportional(16.0)),
            (egui::TextStyle::Button, egui::FontId::proportional(16.0)),
            (egui::TextStyle::Monospace, egui::FontId::monospace(14.0)),
            (egui::TextStyle::Small, egui::FontId::proportional(12.0)),
        ].into();

        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.window_margin = egui::Margin::same(15);

        ctx.set_style(style);

        // Load logo texture from embedded bytes
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

        Self {
            url: String::new(),
            output_path: dirs::download_dir()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
                .to_string_lossy()
                .to_string(),
            is_advanced: true,
            verify_iso: false,
            status_text: "Ready to download".into(),
            current_progress: 0.0,
            is_downloading: false,
            last_downloaded_file: None,
            show_file_dialog: false,
            validation_error: None,
            logo_texture,
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
            return Err("Please enter a URL".into());
        }

        // Check if the URL starts with a supported protocol
        let url_lower = self.url.to_lowercase();
        if !url_lower.starts_with("http://") && 
           !url_lower.starts_with("https://") && 
           !url_lower.starts_with("ftp://") && 
           !url_lower.starts_with("sftp://") && 
           !url_lower.starts_with("magnet:?") {
            return Err("Unsupported protocol. Use http://, https://, ftp://, sftp://, or magnet:".into());
        }

        // Check if the output path is valid
        if self.output_path.is_empty() {
            return Err("Please select a destination folder".into());
        }

        // Verify the output path exists
        let path = std::path::Path::new(&self.output_path);
        if !path.exists() {
            return Err("Destination folder does not exist".into());
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

    #[allow(dead_code)]
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

    fn open_download_folder(&self) {
        let path = if let Some(ref file) = self.last_downloaded_file {
            std::path::Path::new(file).parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from(&self.output_path))
        } else {
            PathBuf::from(&self.output_path)
        };
        
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&path).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer").arg(&path).spawn();
        }
    }

    fn reset_download_state(&mut self) {
        self.current_progress = 0.0;
        self.is_downloading = false;
        self.status_text = "Ready to download".into();
        self.validation_error = None;
    }
}

impl eframe::App for KGetGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_status_updates();

        // Custom Dark Theme Colors
        let mut visuals = egui::Visuals::dark();
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(20, 20, 25);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(35, 35, 45);
        visuals.selection.bg_fill = egui::Color32::from_rgb(60, 140, 220);
        ctx.set_visuals(visuals);

        // Footer fixo (altura pequena) - NÃO ocupa a tela toda
        egui::TopBottomPanel::bottom("kget_footer")
            .resizable(false)
            .exact_height(28.0)
            .show(ctx, |ui| {
                let color = if self.status_text.contains("Error") {
                    egui::Color32::LIGHT_RED
                } else if self.status_text.contains("Completed") {
                    egui::Color32::GREEN
                } else {
                    ui.style().visuals.weak_text_color()
                };

                let version = env!("CARGO_PKG_VERSION");

                ui.horizontal(|ui| {
                    ui.colored_label(color, &self.status_text);

                    // Empurra a versão pro canto direito
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("v{}", version))
                                .small()
                                .weak(),
                        );
                    });
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);

                // Logo image
                if let Some(texture) = &self.logo_texture {
                    let size = egui::vec2(80.0, 80.0);
                    ui.add(egui::Image::new(texture).fit_to_exact_size(size));
                } else {
                    // Fallback if logo failed to load
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(40, 45, 55))
                        .corner_radius(15.0)
                        .inner_margin(egui::Margin::same(20))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("⬇")
                                    .size(60.0)
                                    .color(egui::Color32::from_rgb(100, 200, 120))
                            );
                        });
                }

                ui.add_space(10.0);
                ui.heading(egui::RichText::new("KGet Downloader").size(24.0).strong());
                ui.label(egui::RichText::new("Fast & Reliable Download Manager").italics().weak());
                ui.add_space(20.0);
            });

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Frame::group(ui.style())
                    .fill(egui::Color32::from_rgb(30, 30, 35))
                    .corner_radius(8.0)
                    .inner_margin(15.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("🔗").size(16.0));
                            ui.strong("Download Source");
                        });
                        ui.add_space(5.0);

                        ui.horizontal(|ui| {
                            let w = (ui.available_width() - 90.0).max(120.0);
                            ui.add(
                                egui::TextEdit::singleline(&mut self.url)
                                    .hint_text("Paste your link here (HTTP, FTP, Magnet)...")
                                    .desired_width(w),
                            );

                            if ui.button("📋 Paste").clicked() {
                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                    if let Ok(text) = cb.get_text() {
                                        self.url = text;
                                    }
                                }
                            }
                        });

                        ui.add_space(15.0);

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("📂").size(16.0));
                            ui.strong("Destination Folder");
                        });
                        ui.add_space(5.0);

                        ui.horizontal(|ui| {
                            let w = (ui.available_width() - 90.0).max(120.0);
                            ui.add(egui::TextEdit::singleline(&mut self.output_path).desired_width(w));
                            if ui.button("📂 Browse").clicked() {
                                self.select_output_directory();
                            }
                        });
                    });

                ui.add_space(15.0);

                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.strong("⚙ Options");
                    ui.add_space(5.0);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.is_advanced, "⚡ Parallel Download (Faster)")
                            .on_hover_text("Downloads the file using multiple connections for faster speeds");
                        ui.add_space(10.0);
                        ui.checkbox(&mut self.verify_iso, "🔍 Verify Integrity (SHA256)")
                            .on_hover_text("Calculate SHA256 hash after download to verify file integrity");
                    });
                });

                ui.add_space(15.0);

                // Show validation error if any
                if let Some(ref error) = self.validation_error {
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(255, 100, 100), format!("⚠ {}", error));
                    });
                    ui.add_space(5.0);
                }

                ui.vertical_centered(|ui| {
                    let pb = egui::ProgressBar::new(self.current_progress)
                        .show_percentage()
                        .animate(self.is_downloading);
                    ui.add_sized([ui.available_width() * 0.9, 26.0], pb);

                    ui.add_space(12.0);

                    if self.is_downloading {
                        let cancel_btn = egui::Button::new("🛑 Cancel")
                            .fill(egui::Color32::from_rgb(180, 60, 60));
                        if ui.add_sized([150.0, 40.0], cancel_btn).clicked() {
                            self.download_tx.send(DownloadCommand::Cancel).ok();
                            self.reset_download_state();
                            self.status_text = "Download cancelled".into();
                        }
                    } else if self.last_downloaded_file.is_some() && self.current_progress >= 1.0 {
                        // Download completed - show action buttons
                        ui.horizontal(|ui| {
                            let open_btn = egui::Button::new("📂 Open Folder")
                                .fill(egui::Color32::from_rgb(60, 140, 180));
                            if ui.add_sized([140.0, 40.0], open_btn).clicked() {
                                self.open_download_folder();
                            }
                            
                            ui.add_space(10.0);
                            
                            let new_btn = egui::Button::new("➕ New Download")
                                .fill(egui::Color32::from_rgb(39, 174, 96));
                            if ui.add_sized([150.0, 40.0], new_btn).clicked() {
                                self.url.clear();
                                self.reset_download_state();
                                self.last_downloaded_file = None;
                            }
                        });
                    } else {
                        let download_btn = egui::Button::new("🚀 Start Download")
                            .fill(egui::Color32::from_rgb(39, 174, 96));
                        if ui.add_sized([220.0, 45.0], download_btn).clicked() {
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

                                    self.download_tx
                                        .send(DownloadCommand::Start {
                                            url: self.url.clone(),
                                            output_path: final_output_path,
                                            is_advanced: self.is_advanced,
                                            verify_iso: self.verify_iso,
                                        })
                                        .ok();
                                    self.is_downloading = true;
                                    self.status_text = "Starting download...".into();
                                }
                                Err(e) => {
                                    self.validation_error = Some(e);
                                }
                            }
                        }
                    }
                });
            });

            
        });

        if self.is_downloading {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}