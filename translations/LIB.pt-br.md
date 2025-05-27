# Usando o KGet como uma Crate

O KGet pode ser usado como uma biblioteca Rust em seus próprios projetos para adicionar recursos poderosos de download (HTTP, HTTPS, FTP, SFTP, torrents, progresso, proxy, etc).

[English](../LIB.md) | [Português](translations/LIB.pt-BR.md) | [Español](translations/LIB.es.md)

## Adicione ao seu `Cargo.toml`

```toml
[dependencies]
kget = "1.5.0"
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