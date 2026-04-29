# Usar KGet como Biblioteca Rust

KGet es un gestor de descargas y también un motor Rust reutilizable para
HTTP/HTTPS, FTP, SFTP, enlaces magnet, callbacks de progreso, reanudación,
proxy y verificación SHA256.

[English](../LIB.md) | [Português](LIB.pt-br.md) | [Español](LIB.es.md)

## Instalación

```toml
[dependencies]
Kget = "1.6.2"
```

Features opcionales:

```toml
Kget = { version = "1.6.2", features = ["torrent-native"] }
Kget = { version = "1.6.2", features = ["gui"] }
```

## API Principal

- `download`: descarga HTTP/HTTPS de flujo único con reintentos, proxy, streaming a disco y SHA256 opcional.
- `AdvancedDownloader`: descarga HTTP/HTTPS paralela y reanudable usando byte ranges.
- `download_magnet`: descarga de magnet links con cliente nativo cuando se compila con `torrent-native`.
- `DownloadOptions`: modo silencioso, ruta de salida, verificación ISO y SHA256 esperado.
- `Config`, `ProxyConfig`, `Optimizer`: configuración reutilizable.
- `verify_file_sha256` y `verify_iso_integrity`: utilidades de checksum.

## Descarga Simple

```rust,no_run
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

    Ok(())
}
```

## SHA256 Esperado

```rust,no_run
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

let options = DownloadOptions {
    output_path: Some("imagen.iso".to_string()),
    verify_iso: true,
    expected_sha256: Some("hash_sha256_esperado".to_string()),
    ..Default::default()
};

download(
    "https://example.com/imagen.iso",
    ProxyConfig::default(),
    Optimizer::new(),
    options,
    Some(&|status| println!("{status}")),
)?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

Si el SHA256 calculado no coincide, la función devuelve un error.

## Descarga Avanzada

```rust,no_run
use kget::{AdvancedDownloader, Optimizer, ProxyConfig};

let mut downloader = AdvancedDownloader::new(
    "https://example.com/grande.iso".to_string(),
    "grande.iso".to_string(),
    true,
    ProxyConfig::default(),
    Optimizer::new(),
);

downloader.set_progress_callback(|progress| {
    println!("{:.1}%", progress * 100.0);
});
downloader.set_expected_sha256("hash_sha256_esperado");
downloader.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

El downloader avanzado usa byte ranges y rechaza servidores que anuncian range
pero responden con contenido completo, evitando corrupción silenciosa.

## Enlaces Magnet

```rust,no_run
use kget::{download_magnet, Optimizer, ProxyConfig, TorrentCallbacks};
use std::sync::Arc;

let callbacks = TorrentCallbacks {
    status: Some(Arc::new(|message| println!("{message}"))),
    progress: Some(Arc::new(|progress| println!("{:.1}%", progress * 100.0))),
};

download_magnet(
    "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
    "./downloads",
    true,
    ProxyConfig::default(),
    Optimizer::new(),
    callbacks,
)?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

Use `download_magnet` con la feature `torrent-native` para el cliente torrent integrado. Sin esa feature, KGet intenta usar la aplicación predeterminada del sistema para enlaces magnet.

## Comportamiento de la Biblioteca

- Las llamadas de biblioteca nunca preguntan nada por `stdin`.
- Progreso y estado se envían por callbacks.
- Los archivos se escriben en streaming en disco.
- Los nombres de salida se validan contra separadores de ruta.
- Los helpers SHA256 devuelven error cuando el hash esperado no coincide.

Vea [examples/lib_usage.rs](../examples/lib_usage.rs) para ejemplos mayores.
