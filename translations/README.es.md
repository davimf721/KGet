# KGet v1.6.1

Un gestor de descargas moderno y rápido escrito en Rust. KGet soporta HTTP/HTTPS, FTP/SFTP y enlaces magnet con cliente torrent nativo.

[English](../README.md) | [Português](README.pt-BR.md) | [Español](README.es.md)

## Funciones

- **Multiprotocolo:** HTTP, HTTPS, FTP, SFTP y enlaces magnet.
- **Cliente torrent nativo:** descargas magnet sin depender de apps externas cuando se compila con `torrent-native`.
- **Modo turbo:** descargas HTTP/HTTPS paralelas con byte ranges.
- **GUI y CLI:** interfaz gráfica y uso por terminal.
- **Multiplataforma:** macOS, Linux y Windows.
- **Verificación SHA256:** valida ISOs y cualquier archivo con hash esperado.
- **App macOS nativa:** menú contextual, atajos, acciones Abrir Archivo/Abrir Carpeta y detección de duplicados.
- **Notificaciones nativas:** finalización y errores en la GUI Rust en Linux/Windows.

## Instalación

### Desde código fuente

```bash
# Instale Rust desde https://rustup.rs si es necesario

# Dependencias Linux (Debian/Ubuntu)
sudo apt install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev pkg-config

git clone https://github.com/davimf721/KGet.git
cd KGet
cargo build --release --features gui

./target/release/kget --gui
```

### Desde crates.io

```bash
cargo install Kget --features gui
```

### Binarios precompilados

Descargue versiones para macOS, Linux y Windows desde [Releases](https://github.com/davimf721/KGet/releases).

## Uso

```bash
# Descarga simple
kget https://example.com/archivo.zip

# Modo turbo
kget -a https://example.com/grande.iso

# Guardar en ubicación específica
kget -O ~/Downloads/archivo.zip https://example.com/archivo.zip

# Verificar SHA256 esperado
kget --sha256 <hash> https://example.com/imagen.iso

# Enlace magnet
kget "magnet:?xt=urn:btih:HASH..."

# FTP/SFTP
kget ftp://usuario:clave@servidor/archivo.zip
kget sftp://usuario@servidor/archivo.dat
```

## Opciones principales

| Flag | Descripción |
|------|-------------|
| `-a, --advanced` | Modo turbo con conexiones paralelas |
| `-O <path>` | Archivo o carpeta de salida |
| `-q, --quiet` | Salida mínima |
| `-p <proxy>` | Proxy HTTP/SOCKS5 |
| `-l <bytes>` | Límite de velocidad en bytes/s |
| `--sha256 <hash>` | Verifica el archivo final contra un hash SHA256 esperado |
| `--gui` | Abre la interfaz gráfica |
| `--interactive` | Abre el modo REPL interactivo |

## Biblioteca Rust

KGet también es una biblioteca Rust reutilizable. Vea [LIB.es.md](LIB.es.md) para ejemplos completos de la API actual.

```rust
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

let options = DownloadOptions::default();
download(
    "https://example.com/archivo.zip",
    ProxyConfig::default(),
    Optimizer::new(),
    options,
    None,
)?;
```

## Build y tests

```bash
cargo build --release
cargo build --release --features gui
cargo test
./run-tests.sh
```

## Enlaces

- [Documentación](https://davimf721.github.io/KGet/)
- [Changelog](CHANGELOG.es.md)
- [crates.io](https://crates.io/crates/Kget)
- [Contribución](CONTRIBUTING.es.md)

## Licencia

MIT - vea [LICENSE](../LICENSE).
