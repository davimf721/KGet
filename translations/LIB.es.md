# Usando KGet como una Librería

KGet puede ser usado como una biblioteca Rust en sus propios proyectos para agregar potentes funciones de descarga (HTTP, HTTPS, FTP, SFTP, torrents, progreso, proxy, etc).

[English](../LIB.md) | [Português](translations/LIB.pt-BR.md) | [Español](translations/LIB.es.md)

## Agregue a su `Cargo.toml`

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

## Descarga Avanzada (Chunks Paralelos, Reanudación)

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

## Configuración Personalizada

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

## API Simple

Para descargas rápidas sin crear una instancia de KGet:

```rust
use kget::simple;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    simple::download("https://example.com/file.txt", Some("file.txt"))?;
    Ok(())
}
```

## Ejemplo de Callback de Progreso

```rust
use kget::{KGet, DownloadOptions};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let options = DownloadOptions {
        quiet_mode: false,
        progress_callback: Some(Box::new(|current, total, _speed| {
            println!("Progreso: {}/{}", current, total);
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

## Protocolos Soportados

- HTTP/HTTPS
- FTP
- SFTP
- Enlaces Magnet (torrents, requiere `transmission-daemon`)

## Más

Vea [docs.rs/kget](https://docs.rs/kget) para la documentación completa de la API.

---