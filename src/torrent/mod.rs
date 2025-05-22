use std::error::Error;
use std::time::Duration;
use tokio::time::sleep;
use url::Url;
use transmission_rpc::{
    types::{BasicAuth, TorrentAddArgs, Id, TorrentGetField, TorrentStatus},
    TransClient,
};
use crate::config::ProxyConfig;
use crate::optimization::Optimizer;
use crate::progress::create_progress_bar;
use crate::utils::print;
use opener;

pub struct TorrentDownloader {
    url: String,
    output: String,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
}

impl TorrentDownloader {
    pub fn new(
        url: String,
        output: String,
        quiet: bool,
        proxy: ProxyConfig,
        optimizer: Optimizer,
    ) -> Self {
        Self {
            url,
            output,
            quiet,
            proxy,
            optimizer,
        }
    }

    pub async fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Configure transmission URL with proxy if needed
        let transmission_url = if self.proxy.enabled && self.proxy.url.is_some() {
            // Só usa o proxy se estiver habilitado E uma URL de proxy for fornecida
            let base_proxy_url = self.proxy.url.as_ref().unwrap(); // Sabemos que é Some
            if self.proxy.username.is_some() && self.proxy.password.is_some() {
                // Proxy com autenticação
                format!("http://{}:{}@{}/transmission/rpc",
                    self.proxy.username.as_deref().unwrap_or(""),
                    self.proxy.password.as_deref().unwrap_or(""),
                    base_proxy_url.trim_start_matches("http://").trim_start_matches("https://") // Remove esquema se presente
                )
            } else {
                // Proxy sem autenticação
                format!("http://{}/transmission/rpc", 
                    base_proxy_url.trim_start_matches("http://").trim_start_matches("https://") // Remove esquema se presente
                )
            }
        } else {
            // Proxy desabilitado ou URL do proxy não fornecida, usa o endereço padrão do Transmission
            "http://localhost:9091/transmission/rpc".to_string()
        };

        // Create Transmission RPC client
        let mut client = TransClient::with_auth(
            Url::parse(&transmission_url)?,
            BasicAuth {
                user: "transmission".into(), // Usuário padrão do Transmission, ajuste se necessário
                password: "transmission".into(), // Senha padrão do Transmission, ajuste se necessário
            },
        );

        // Print status
        if !self.quiet {
            print(&format!("Adding torrent: {}", self.url), self.quiet);
        }

        // Add torrent
        let args = TorrentAddArgs {
            filename: Some(self.url.clone()),
            download_dir: Some(self.output.clone()),
            paused: Some(false),
            // Apply optimization settings
            peer_limit: Some(self.optimizer.get_peer_limit() as i64),
            ..Default::default()
        };

        let response = client.torrent_add(args).await.map_err(|e| {
            Box::<dyn Error + Send + Sync>::from(format!("Failed to add torrent: {}", e))
        })?;
        
        // Extrai o torrent ID corretamente
        let torrent_id = match &response.arguments {
            transmission_rpc::types::TorrentAddedOrDuplicate::TorrentAdded(added) => {
                added.id.map(Id::Id).ok_or_else(|| Box::<dyn Error + Send + Sync>::from("TorrentAdded response missing ID"))?
            },
            transmission_rpc::types::TorrentAddedOrDuplicate::TorrentDuplicate(duplicate) => {
                duplicate.id.map(Id::Id).ok_or_else(|| Box::<dyn Error + Send + Sync>::from("TorrentDuplicate response missing ID"))?
            },
            _ => { // Este caso pode precisar de ajuste dependendo se há uma variante de Erro explícita
                return Err(Box::<dyn Error + Send + Sync>::from("Failed to get torrent ID from response, unexpected variant"));
            }
        };

        // Abrir a interface web do Transmission
        if !self.quiet {
            let transmission_web_url = "http://localhost:9091/transmission/web/";
            print(&format!("\nStarting the Download!\n"), self.quiet);
            print(&format!("\nOpening Transmission web UI: {}\n", transmission_web_url), self.quiet);
            if let Err(e) = opener::open(transmission_web_url) {
                print(&format!("Warning: Could not open web browser: {}", e), self.quiet);
                // Não retornar um erro aqui, pois o download pode prosseguir
            }
        }

        // Setup progress bar
        let progress = create_progress_bar(
            self.quiet,
            "Downloading torrent".to_string(),
            None,
            false
        );

        // Monitor download progress
        let mut attempt_count = 0;
        let max_attempts = 1800; // 30 minutes timeout (1800 seconds)
        
        loop {
            if attempt_count >= max_attempts {
                progress.finish_with_message("Download timeout or stalled."); // Mensagem mais informativa
                return Err("Download timeout after 30 minutes or torrent stalled".into());
            }
            
            let torrent_info_result = client.torrent_get( // Renomeado para evitar sombreamento
                Some(vec![
                    TorrentGetField::PercentDone,
                    TorrentGetField::Status,
                    TorrentGetField::Name,
                    TorrentGetField::RateDownload,
                    TorrentGetField::Eta,
                    TorrentGetField::Error, // Adicionar campo de erro para verificar erros do daemon
                    TorrentGetField::ErrorString,
                ]),
                Some(vec![torrent_id.clone()])
            ).await;

            if let Err(e) = torrent_info_result {
                // Se falhar ao obter informações do torrent, pode ser um problema de conexão
                progress.abandon_with_message("Failed to get torrent info");
                return Err(e);
            }
            let torrent_info = torrent_info_result.unwrap();


            if let Some(t) = torrent_info.arguments.torrents.first() {
                let percent_done = t.percent_done.unwrap_or(0.0);
                let current_progress = (percent_done * 100.0) as u64;
                progress.set_position(current_progress);
                
                if let Some(name) = &t.name {
                    let speed_kb = t.rate_download.map_or(0, |rate| rate / 1024);
                    progress.set_message(format!(
                        "{} - {:.2}% - {} KB/s", 
                        name, 
                        percent_done * 100.0, // Usar float para precisão na mensagem
                        speed_kb
                    ));
                }

                // Verificar se há um erro reportado pelo daemon para este torrent
                if let Some(error_code) = t.error {
                    if (error_code as i32) != 0 { // 0 geralmente significa sem erro
                        let error_message = t.error_string.as_deref().unwrap_or("Unknown torrent error");
                        progress.abandon_with_message(format!("Torrent error: {}", error_message));
                        return Err(format!("Torrent error (code {:?}): {}", error_code, error_message).into());
                    }
                }

                // Check if download is complete
                if percent_done >= 1.0 {
                    progress.set_message(format!(
                        "{} - Complete",
                        t.name.as_deref().unwrap_or("Torrent")
                    ));
                    break; // Sai do loop se completo
                }
                
                // Check for torrent status
                if let Some(status) = t.status {
                    match status {
                        TorrentStatus::Stopped => {
                            // Se estiver parado, mas não completo, pode ser um problema ou apenas o estado inicial de um duplicado.
                            // Damos algumas tentativas para ver se ele inicia.
                            if attempt_count > 5 && percent_done < 1.0 { // Após 5 segundos, se ainda parado e não completo
                                progress.abandon_with_message("Torrent stopped and not progressing.");
                                return Err(format!(
                                    "Torrent '{}' stopped and not progressing.",
                                    t.name.as_deref().unwrap_or("Unknown")
                                ).into());
                            }
                            // Se estiver parado e completo, o break acima já teria sido acionado.
                        }
                        TorrentStatus::Downloading => { 
                            // Tudo ok, está baixando
                        }
                        TorrentStatus::Seeding => {    
                            // Se estiver semeando, e percent_done < 1.0, algo está estranho, mas vamos deixar o loop de percent_done tratar.
                            // Se percent_done >= 1.0, o break acima trata.
                        }
                        
                        _ => {
                            // Para outros status não explicitamente tratados (como DownloadWait, SeedWait)
                            // você pode querer apenas continuar ou logar, dependendo da sua lógica.
                        }
                    }
                }
            } else {
                // Não deveria acontecer se o ID do torrent for válido
                progress.abandon_with_message("Torrent info not found.");
                return Err("Torrent info not found after adding.".into());
            }

            attempt_count += 1;
            sleep(Duration::from_secs(1)).await;
        }

        progress.finish_with_message(format!(
            "Download of '{}' completed successfully!",
            // Tenta obter o nome do torrent uma última vez para a mensagem final
            client.torrent_get(Some(vec![TorrentGetField::Name]), Some(vec![torrent_id]))
                  .await
                  .ok()
                  .and_then(|resp| resp.arguments.torrents.first().and_then(|t| t.name.clone()))
                  .unwrap_or_else(|| "Torrent".to_string())
        ));
        
        // Apply optimizer if needed
        if self.optimizer.is_compression_enabled() {
            print("Optimizing downloaded files...", self.quiet);
            // Implement compression here if needed
        }

        Ok(())
    }
}
