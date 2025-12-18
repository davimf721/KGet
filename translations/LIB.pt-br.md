# Usando o KGet como uma Crate

O KGet pode ser usado como uma biblioteca Rust em seus próprios projetos para adicionar recursos poderosos de download (HTTP, HTTPS, FTP, SFTP, torrents, progresso, proxy, etc).

[English](../LIB.md) | [Português](translations/LIB.pt-BR.md) | [Español](translations/LIB.es.md)

## Adicione ao seu `Cargo.toml`

Sem GUI (recomendado para servidores/CI/builds mínimos):

```toml
[dependencies]
kget = "1.5.1"
```

Com GUI ativada (isso puxa dependências opcionais de GUI):

```toml
[dependencies]
kget = { version = "1.5.1", features = ["gui"] }
```

## Uso Básico

```rust
use kget::KGet;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let kget = KGet::new()?;
    kget.download(
        "https://example.com/file.zip",
        Some("file.zip".to_string()),
        false, // modo silencioso
    )?;
    Ok(())
}
```

## Funções de conveniência

O crate também expõe funções top-level simples para que você possa chamá-las diretamente
sem criar uma instância de `KGet`:

- `kget::download(url, output_path, quiet_mode)` — download padrão HTTP/HTTPS/FTP/SFTP.
- `kget::advanced_download(url, output_path, quiet_mode)` — download paralelo/retomável.

Exemplo usando a função top-level `download`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    kget::download("https://example.com/file.txt", Some("file.txt"), false)?;
    Ok(())
}
```

## API de barra de progresso

Se você quiser renderizar sua própria barra de progresso (por exemplo, integrar com sua UI),
o crate expõe uma fábrica de barras de progresso:

```rust
let bar = kget::create_progress_bar_factory(false, "Downloading".to_string(), Some(1024u64), false);
// use `bar` como um `indicatif::ProgressBar`
```

## Recurso GUI (opcional)

A GUI é opcional e fornecida por meio de uma feature do Cargo chamada `gui`. Compile ou execute com a GUI ativada usando:

```bash
cargo build --features gui
cargo run --features gui -- --gui
```

Quando a feature `gui` estiver desabilitada, o crate e o binário compilam sem dependências da GUI.


## Download Avançado (Chunks Paralelos, Retomada)

```rust
use kget::KGet;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let kget = KGet::new()?;
    kget.advanced_download(
        "https://example.com/largefile.iso",
        Some("largefile.iso".to_string()),
        false,
    )?;
    Ok(())
}
```

## Configuração Personalizada

```rust
use kget::{KGet, Config};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut config = Config::load()?;
    config.optimization.speed_limit = Some(1024 * 1024); // 1 MB/s
    let kget = KGet::with_config(config);
    kget.download("https://example.com/file.zip", None, false)?;
    Ok(())
}
```

## API Simples

Para downloads rápidos sem criar uma instância do KGet:

```rust
use kget::simple;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    simple::download("https://example.com/file.txt", Some("file.txt"))?;
    Ok(())
}
```

## Exemplo de Callback de Progresso

```rust
use kget::{KGet, DownloadOptions};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let options = DownloadOptions {
        quiet_mode: false,
        progress_callback: Some(Box::new(|current, total, _speed| {
            println!("Progresso: {}/{}", current, total);
        })),
        ..Default::default()
    };
    kget::simple::download_with_options(
        "https://example.com/file.txt",
        Some("file.txt"),
        options,
    )?;
    Ok(())
}
```

## Protocolos Suportados

- HTTP/HTTPS
- FTP
- SFTP
- Links Magnet (torrents, requer `transmission-daemon`)

## Mais

Veja [docs.rs/kget](https://docs.rs/kget) para a documentação completa da API.

---