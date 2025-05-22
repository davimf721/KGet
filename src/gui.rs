use eframe::egui;
use std::sync::mpsc;

// Comando enviado da GUI para o thread de trabalho
pub enum DownloadCommand {
    Start(String, String), // url, output_path
    Cancel, // Ainda não implementado na UI, mas útil para o futuro
}

// Mensagem enviada do thread de trabalho de volta para a GUI
#[derive(Debug)]
pub enum WorkerToGuiMessage {
    Progress(f32),          // Valor do progresso de 0.0 a 1.0
    StatusUpdate(String),   // Mensagem de status geral
    Completed(String),      // Mensagem de conclusão bem-sucedida
    Error(String),          // Mensagem de erro
}

pub struct KelpsGetGui {
    url: String,
    output_path: String,
    
    status_text: String,
    current_progress: f32,
    is_downloading: bool,

    // Para enviar comandos para o thread de trabalho
    download_tx: mpsc::Sender<DownloadCommand>,
    // Para receber atualizações de status do thread de trabalho
    status_rx: mpsc::Receiver<WorkerToGuiMessage>,
}

impl KelpsGetGui {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        download_tx: mpsc::Sender<DownloadCommand>,
        status_rx: mpsc::Receiver<WorkerToGuiMessage>,
    ) -> Self {
        Self {
            url: String::new(),
            output_path: String::new(),
            status_text: "Ready.".to_string(),
            current_progress: 0.0,
            is_downloading: false,
            download_tx,
            status_rx,
        }
    }

    fn process_status_updates(&mut self) {
        while let Ok(msg) = self.status_rx.try_recv() {
            match msg {
                WorkerToGuiMessage::Progress(p) => {
                    self.current_progress = p;
                    if !self.is_downloading && p < 1.0 && p > 0.0 { // Se o progresso chegar e não estivermos "baixando"
                        self.is_downloading = true; // Pode ter sido iniciado por outro meio ou estado recuperado
                    }
                    if self.is_downloading { // Atualiza o texto de status apenas se estiver baixando
                         self.status_text = format!("Downloading: {:.0}%", p * 100.0);
                    }
                }
                WorkerToGuiMessage::StatusUpdate(s) => {
                    self.status_text = s;
                }
                WorkerToGuiMessage::Completed(s_msg) => {
                    self.status_text = format!("Completed: {}", s_msg);
                    self.current_progress = 1.0;
                    self.is_downloading = false;
                }
                WorkerToGuiMessage::Error(err_msg) => {
                    self.status_text = format!("Error: {}", err_msg);
                    self.current_progress = 0.0; // Ou manter o progresso atual, dependendo da preferência
                    self.is_downloading = false;
                }
            }
        }
    }
}

impl eframe::App for KelpsGetGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_status_updates();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("KelpsGet Downloader");
            
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.label("URL:");
                ui.add_enabled(!self.is_downloading, egui::TextEdit::singleline(&mut self.url).hint_text("Enter download URL"));
            });

            ui.horizontal(|ui| {
                ui.label("Output:");
                ui.add_enabled(!self.is_downloading, egui::TextEdit::singleline(&mut self.output_path).hint_text("Enter output file/folder path"));
            });
            
            ui.add_space(10.0);

            if ui.add_enabled(!self.is_downloading, egui::Button::new("Download")).clicked() {
                if self.url.is_empty() {
                    self.status_text = "URL cannot be empty.".to_string();
                } else if self.output_path.is_empty() {
                    self.status_text = "Output path cannot be empty.".to_string();
                }else {
                    match self.download_tx.send(DownloadCommand::Start(
                        self.url.clone(),
                        self.output_path.clone(),
                    )) {
                        Ok(_) => {
                            self.status_text = "Download command sent...".to_string();
                            self.is_downloading = true;
                            self.current_progress = 0.0; // Reseta o progresso ao iniciar
                        }
                        Err(e) => {
                            self.status_text = format!("Error sending command: {}", e);
                            self.is_downloading = false;
                        }
                    }
                }
            }
            
            ui.add_space(10.0);
            ui.label(&self.status_text);
            ui.add(egui::ProgressBar::new(self.current_progress).show_percentage());
            
            // Para manter a UI atualizando enquanto o download (potencialmente) acontece
            if self.is_downloading {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        });
    }
}