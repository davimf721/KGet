use indicatif::{ProgressBar, ProgressStyle};

/// Cria uma barra de progresso personalizada para download de arquivos.
/// 
/// # Arguments
/// 
/// * `quiet_mode` - Se true, a barra de progresso será ocultada
/// * `msg` - Mensagem a ser exibida na barra
/// * `length` - Tamanho total do arquivo em bytes (opcional)
/// 
/// # Returns
/// 
/// Uma barra de progresso configurada com estilo personalizado
pub fn create_progress_bar(quiet_mode: bool, msg: String, length: Option<u64>) -> ProgressBar {
    let bar = if quiet_mode {
        ProgressBar::hidden()
    } else {
        match length {
            Some(len) => ProgressBar::new(len),
            None => ProgressBar::new_spinner(),
        }
    };

    bar.set_message(msg);
    
    if let Some(_) = length {
        bar.set_style(
            ProgressStyle::default_bar()
                .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({percent}%) eta: {eta} speed: {binary_bytes_per_sec}")
                .unwrap()
                .progress_chars("=>-")
        );
    } else {
        bar.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg} {elapsed}")
                .unwrap()
        );
    }

    bar
}