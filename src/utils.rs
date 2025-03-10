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
