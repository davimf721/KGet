use indicatif::{ProgressBar, ProgressStyle};

/// Cria uma barra de progresso personalizada para download de arquivos.
/// 
/// # Arguments
/// 
/// * `quiet_mode` - Se true, a barra de progresso será ocultada
/// * `msg` - Mensagem a ser exibida na barra
/// * `length` - Tamanho total do arquivo em bytes (opcional)
/// * `is_parallel` - Se true, mostra informações de download paralelo
/// 
/// # Returns
/// 
/// Uma barra de progresso configurada com estilo personalizado
pub fn create_progress_bar(quiet_mode: bool, msg: String, length: Option<u64>, is_parallel: bool) -> ProgressBar {
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
        let template = if is_parallel {
            "{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({percent}%) eta: {eta} speed: {binary_bytes_per_sec}\nChunks: {chunks} active"
        } else {
            "{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({percent}%) eta: {eta} speed: {binary_bytes_per_sec}"
        };

        bar.set_style(
            ProgressStyle::default_bar()
                .template(template)
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