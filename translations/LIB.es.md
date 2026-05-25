# Usando KGet como Biblioteca Rust

KGet es un gestor de descargas y también un motor Rust reutilizable para
HTTP/HTTPS, FTP, SFTP, WebDAV, magnet links, callbacks de progreso, reanudación
de descargas, proxy y verificación de checksum multi-algoritmo.

[English](../LIB.md) | [Português](LIB.pt-br.md) | [Español](LIB.es.md)

## Instalación

```toml
[dependencies]
Kget = "1.7.0"

# Opcional: cliente torrent nativo
Kget = { version = "1.7.0", features = ["torrent-native"] }

# Opcional: API async
Kget = { version = "1.7.0", features = ["async"] }
```

## Quick Start — API Builder (recomendada)

```rust,no_run
use kget::KgetError;

fn main() -> Result<(), KgetError> {
    kget::builder("https://example.com/archivo.zip")
        .output("./downloads/")
        .connections(8)
        .sha256("abc123def456...")
        .download()?;
    Ok(())
}
```

## Métodos del Builder

`kget::builder(url)` retorna un `DownloadBuilder`. Todos los métodos son encadenables:

| Método | Descripción |
|--------|-------------|
| `.output(ruta)` | Guardar en archivo o directorio |
| `.connections(n)` | Conexiones paralelas (modo turbo) |
| `.speed_limit(bps)` | Máximo de bytes/s (token bucket global) |
| `.proxy(url)` | URL del proxy HTTP o SOCKS5 |
| `.quiet(bool)` | Suprimir salida de progreso |
| `.sha256(hash)` | Verificar SHA-256 después de la descarga |
| `.sha512(hash)` | Verificar SHA-512 después de la descarga |
| `.sha1(hash)` | Verificar SHA-1 después de la descarga |
| `.md5(hash)` | Verificar MD5 después de la descarga |
| `.blake3(hash)` | Verificar BLAKE3 después de la descarga |
| `.verify_from(url)` | Descargar y analizar archivo de checksum sidecar GNU/BSD |
| `.header(nombre, valor)` | Agregar header HTTP |
| `.retry(config)` | Política de reintentos personalizada (`RetryConfig`) |
| `.range(inicio, fin)` | Solicitar rango de bytes específico |

Métodos terminales:

| Método | Retorna | Descripción |
|--------|---------|-------------|
| `.download()` | `Result<DownloadResult, KgetError>` | Descarga a disco |
| `.download_to_bytes()` | `Result<Vec<u8>, KgetError>` | Descarga en memoria |
| `.download_to_reader()` | `Result<impl Read, KgetError>` | Reader de streaming |
| `.spawn()` | `Result<(JoinHandle, Receiver<DownloadEvent>), KgetError>` | Hilo en background con canal de eventos |
| `.download_async()` | `impl Future<…>` | API async (feature `async`) |

## Descargas en Lote

```rust,no_run
let results = kget::batch([
    "https://mirror1.example.com/archivo.iso",
    "https://mirror2.example.com/otro.tar.gz",
])
.concurrency(4)
.output_dir("./downloads/")
.download_all();

for r in results {
    match r.result {
        Ok(d) => println!("✓ {} — {:.1} MB/s", r.url, d.avg_speed_bps / 1e6),
        Err(e) => eprintln!("✗ {}: {}", r.url, e),
    }
}
```

## Canal de Eventos

```rust,no_run
use kget::{DownloadEvent, KgetError};

let (handle, rx) = kget::builder("https://example.com/grande.iso")
    .connections(4)
    .spawn()?;

for event in rx {
    match event {
        DownloadEvent::Progress { percent, speed_bps, eta_secs } => {
            print!("\r{:.1}%  {:.1} MB/s  eta {}s", percent, speed_bps / 1e6, eta_secs.unwrap_or(0));
        }
        DownloadEvent::Completed { path, bytes, .. } => {
            println!("\nGuardados {bytes} bytes en {path}");
        }
        DownloadEvent::Error(e) => eprintln!("Error: {e}"),
        _ => {}
    }
}
handle.join().ok();
# Ok::<(), KgetError>(())
```

## Errores Tipados

```rust
pub enum KgetError {
    Network(reqwest::Error),
    Io(std::io::Error),
    ChecksumMismatch { algorithm: String, expected: String, got: String },
    Protocol(String),
    Cancelled,
    NotFound(String),
    SidecarError(String),
    Other(String),
}
```

Los errores permanentes (`Cancelled`, `NotFound`, `ChecksumMismatch`) nunca se reintentan.

## Checksums

```rust,no_run
use kget::checksum::{compute_checksum, ChecksumAlgorithm};

let hash = compute_checksum(ChecksumAlgorithm::Blake3, std::path::Path::new("archivo.bin"))?;
println!("BLAKE3: {hash}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Algoritmos soportados: `Sha256`, `Sha512`, `Sha1`, `Md5`, `Blake3`.

Verificación por archivo sidecar:

```rust,no_run
kget::builder("https://example.com/release.tar.gz")
    .verify_from("https://example.com/release.tar.gz.sha256sum")
    .download()?;
# Ok::<(), kget::KgetError>(())
```

## ResumePolicy

```rust,no_run
use kget::{AdvancedDownloader, ResumePolicy, Optimizer, ProxyConfig};

let mut dl = AdvancedDownloader::new(
    "https://example.com/imagen.iso".to_string(),
    "imagen.iso".to_string(),
    true,
    ProxyConfig::default(),
    Optimizer::new(),
)?;
dl.set_resume_policy(ResumePolicy::AlwaysResume); // nunca bloquea en stdin
dl.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

| Variante | Comportamiento |
|----------|---------------|
| `Ask` (defecto) | Solicita via stdin en sesiones interactivas |
| `AlwaysResume` | Ignora el prompt, procede sin preguntar |
| `AlwaysRestart` | Ignora el prompt, reinicia desde cero |

Los llamadores de la biblioteca siempre deben establecer `AlwaysResume` o `AlwaysRestart`.

## Descarga HTTP Avanzada Paralela

```rust,no_run
use kget::{AdvancedDownloader, Optimizer, ProxyConfig};

let mut downloader = AdvancedDownloader::new(
    "https://example.com/grande.iso".to_string(),
    "grande.iso".to_string(),
    false,
    ProxyConfig::default(),
    Optimizer::new(),
)?;  // new() retorna Result — propaga con ?

downloader.set_resume_policy(kget::ResumePolicy::AlwaysResume);
downloader.set_progress_callback(|progress| {
    print!("\r{:.1}%", progress * 100.0);
});
downloader.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## Descarga HTTP Simple

```rust,no_run
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

let options = DownloadOptions {
    output_path: Some("archivo.zip".to_string()),
    ..Default::default()
};

download(
    "https://example.com/archivo.zip",
    ProxyConfig::default(),
    Optimizer::new(),
    options,
    None,
)?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## Descarga en Memoria

```rust,no_run
let bytes: Vec<u8> = kget::builder("https://example.com/datos.json")
    .download_to_bytes()?;
println!("Recibidos {} bytes", bytes.len());
# Ok::<(), kget::KgetError>(())
```

## FTP

```rust,no_run
use kget::ftp::FtpDownloader;
use kget::{Optimizer, ProxyConfig};

let dl = FtpDownloader::new(
    "ftp://ftp.gnu.org/gnu/emacs/emacs-28.2.tar.gz".to_string(),
    "emacs-28.2.tar.gz".to_string(),
    false,
    ProxyConfig::default(),
    Optimizer::new(),
);
dl.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## SFTP

```rust,no_run
use kget::sftp::SftpDownloader;
use kget::{Optimizer, ProxyConfig};

let dl = SftpDownloader::new(
    "sftp://usuario:clave@servidor.example.com/ruta/archivo.tar.gz".to_string(),
    "archivo.tar.gz".to_string(),
    false,
    ProxyConfig::default(),
    Optimizer::new(),
);
dl.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

Prioridad de autenticación:
1. Contraseña en la URL (`sftp://usuario:clave@host/ruta`)
2. SSH agent activo
3. Archivos de clave por defecto (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`, `~/.ssh/id_ecdsa`)

Las host keys se verifican contra `~/.ssh/known_hosts`; las divergencias generan error.

## WebDAV

```rust,no_run
use kget::webdav::WebDavDownloader;
use kget::{Optimizer, ProxyConfig};

let dl = WebDavDownloader::new(
    "webdavs://usuario:clave@nas.local/backups/db.tar.gz".to_string(),
    "db.tar.gz".to_string(),
    false,
    ProxyConfig::default(),
    Optimizer::new(),
);
dl.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## yt-dlp

```rust,no_run
use kget::ytdlp::{download_video, VideoQuality, is_video_url};

if is_video_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ") {
    download_video(
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        VideoQuality::Quality720p,
        "./downloads",
        None,
    )?;
}
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## Magnet Links

```rust,no_run
use kget::{download_magnet, Optimizer, ProxyConfig, TorrentCallbacks};
use std::sync::Arc;

download_magnet(
    "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
    "./downloads",
    true,
    ProxyConfig::default(),
    Optimizer::new(),
    TorrentCallbacks {
        status: Some(Arc::new(|msg| println!("{msg}"))),
        progress: Some(Arc::new(|p| println!("{:.1}%", p * 100.0))),
    },
)?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## Metalink

```rust,no_run
use kget::metalink::download_metalink;
use kget::{Optimizer, ProxyConfig};

download_metalink(
    "ubuntu-24.04.meta4",
    "~/Downloads",
    false,
    ProxyConfig::default(),
    Optimizer::new(),
)?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## Historial de Descargas

```rust,no_run
use kget::queue::{DownloadHistory, EntryStatus, HistoryEntry};

let mut history = DownloadHistory::load();
let entry = HistoryEntry::new("https://example.com/archivo.iso", "/home/user/Downloads", None);
history.record(entry, EntryStatus::Completed, None);
history.save()?;

for e in history.recent(10) {
    println!("{} {} {}", e.created_at_display(), e.status, e.filename);
}
history.clear_completed();
history.save()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## API Async

Con `--features async`:

```rust,no_run
#[tokio::main]
async fn main() -> Result<(), kget::KgetError> {
    kget::builder("https://example.com/archivo.zip")
        .output("./downloads/")
        .connections(4)
        .download_async()
        .await?;
    Ok(())
}
```

## Garantías de la Biblioteca

- Los llamadores de la biblioteca nunca bloquean en `stdin` cuando `ResumePolicy` está configurada.
- Progreso y estado se exponen via callbacks y canales de eventos.
- Los archivos se transmiten a disco (excepto `.download_to_bytes()`).
- Los nombres de archivo se validan: rechaza bytes nulos, path traversal, >255 bytes y nombres reservados de Windows.
- Solo los errores 5xx y de conexión se reintentan; los 4xx fallan inmediatamente.

Ver [examples/lib_usage.rs](../examples/lib_usage.rs) para más ejemplos.
