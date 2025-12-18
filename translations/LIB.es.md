# Usando KGet como una Librería

KGet puede ser usado como una biblioteca Rust en sus propios proyectos para agregar potentes funciones de descarga (HTTP, HTTPS, FTP, SFTP, torrents, progreso, proxy, etc).

[English](../LIB.md) | [Português](translations/LIB.pt-BR.md) | [Español](translations/LIB.es.md)

## Agregue a su `Cargo.toml`

Sin GUI (recomendado para servidores/CI/compilaciones mínimas):

```toml
[dependencies]
kget = "1.5.1"
```

Con GUI activada (esto incluirá dependencias opcionales de GUI):

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

## Funciones de conveniencia

El crate también expone funciones top-level simples para que pueda llamarlas directamente
sin crear una instancia de `KGet`:

- `kget::download(url, output_path, quiet_mode)` — descarga estándar HTTP/HTTPS/FTP/SFTP.
- `kget::advanced_download(url, output_path, quiet_mode)` — descarga paralela/retomable.

Ejemplo usando la función top-level `download`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    kget::download("https://example.com/file.txt", Some("file.txt"), false)?;
    Ok(())
}
```

## API de barra de progreso

Si desea renderizar su propia barra de progreso (por ejemplo, integrarla con su UI),
el crate expone una fábrica de barras de progreso:

```rust
let bar = kget::create_progress_bar_factory(false, "Downloading".to_string(), Some(1024u64), false);
// use `bar` como un `indicatif::ProgressBar`
```

## Feature GUI (opcional)

La GUI es opcional y está disponible detrás de una feature de Cargo llamada `gui`. Compile o ejecute con la GUI activada usando:

```bash
cargo build --features gui
cargo run --features gui -- --gui
```

Si la feature `gui` no está activada, el crate y el binario se compilarán sin las dependencias relacionadas con la GUI.


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