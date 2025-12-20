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

        Self {
            url: String::new(),
            output_path: std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            is_advanced: true,
            verify_iso: false,
            status_text: "Ready to download".into(),
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
        self.process_status_updates();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(5.0);
                ui.heading("📥 KGet Downloader");
                ui.add_space(15.0);
            });

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    ui.strong("Download Details");
                    ui.add_space(5.0);

                    ui.label("URL de Origem:");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.url)
                            .hint_text("Cole o link aqui...")
                            .desired_width(ui.available_width() - 90.0));

                        if ui.button("📋 Colar").clicked() {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                if let Ok(text) = cb.get_text() { self.url = text; }
                            }
                        }
                    });

                    ui.add_space(10.0);

                    ui.label("Caminho de Destino:");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.output_path).desired_width(ui.available_width() - 90.0));
                        if ui.button("📂 Abrir").clicked() {
                            self.select_output_directory();
                        }
                    });
                });

                ui.add_space(15.0);

                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.strong("Configurações");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.is_advanced, "⚡ Download Paralelo (Mais Rápido)");
                        ui.checkbox(&mut self.verify_iso, "🔍 Verificar ISO ao finalizar");
                    });
                });

                ui.add_space(20.0);

                // Seção de Progresso
                ui.vertical_centered(|ui| {
                    let pb = egui::ProgressBar::new(self.current_progress)
                        .show_percentage()
                        .animate(self.is_downloading);
                    ui.add_sized([ui.available_width() * 0.9, 30.0], pb);

                    ui.add_space(10.0);

                    if self.is_downloading {
                        if ui.add_sized([120.0, 40.0], egui::Button::new("🛑 Cancelar")).clicked() {
                            self.download_tx.send(DownloadCommand::Cancel).ok();
                        }
                    } else {
                        let download_btn = egui::Button::new("🚀 Iniciar Download")
                            .fill(egui::Color32::from_rgb(39, 174, 96));
                        if ui.add_sized([250.0, 45.0], download_btn).clicked() {
                            if self.validate_input().is_ok() {
                                self.download_tx.send(DownloadCommand::Start {
                                    url: self.url.clone(),
                                    output_path: self.output_path.clone(),
                                    is_advanced: self.is_advanced,
                                    verify_iso: self.verify_iso,
                                }).ok();
                                self.is_downloading = true;
                            }
                        }
                    }
                });
            });

            // Rodapé com Status
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(5.0);
                let color = if self.status_text.contains("Error") { egui::Color32::LIGHT_RED }
                else if self.status_text.contains("Completed") { egui::Color32::GREEN }
                else { ui.style().visuals.weak_text_color() };
                ui.colored_label(color, &self.status_text);
                ui.separator();
            });
        });

        if self.is_downloading { ctx.request_repaint_after(std::time::Duration::from_millis(100)); }
    }
}