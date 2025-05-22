/// Imprime uma mensagem no console, a menos que o modo silencioso esteja ativado.
/// 
/// # Arguments
/// 
/// * `msg` - A mensagem a ser impressa
/// * `quiet_mode` - Se true, suprime a impressão da mensagem
pub fn print(msg: &str, quiet_mode: bool) {
    if !quiet_mode {
        println!("{}", msg);
    }
}

/// Tenta extrair o nome do arquivo de uma URL.
/// Se a URL não puder ser analisada ou não contiver um nome de arquivo no caminho,
/// retorna o nome de arquivo padrão fornecido.
///
/// # Arguments
///
/// * `url_str` - A string da URL da qual extrair o nome do arquivo.
/// * `default_filename` - O nome do arquivo a ser retornado se nenhum puder ser extraído da URL.
///
/// # Returns
///
/// Uma `String` contendo o nome do arquivo extraído ou o nome do arquivo padrão.
pub fn get_filename_from_url_or_default(url_str: &str, default_filename: &str) -> String {
    // Tenta analisar a URL
    // Para isso, você pode precisar adicionar o crate `url` ao seu Cargo.toml:
    // url = "2"
    if let Ok(parsed_url) = url::Url::parse(url_str) {
        // Tenta obter o último segmento do caminho
        if let Some(segments) = parsed_url.path_segments() {
            if let Some(last_segment) = segments.last() {
                if !last_segment.is_empty() {
                    return last_segment.to_string();
                }
            }
        }
    }
    // Retorna o nome do arquivo padrão se a análise falhar ou o caminho estiver vazio/inválido
    default_filename.to_string()
}
