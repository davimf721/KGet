# KGet v1.6.3

Un gestor de descargas moderno y rápido escrito en Rust. KGet soporta HTTP/HTTPS, FTP/SFTP y enlaces magnet con cliente torrent nativo.

[English](../README.md) | [Português](README.pt-BR.md) | [Español](README.es.md)

## Funciones

- **Multiprotocolo:** HTTP, HTTPS, FTP, SFTP y enlaces magnet.
- **Cliente torrent nativo:** descargas magnet sin depender de apps externas cuando se compila con `torrent-native`.
- **Modo turbo:** descargas HTTP/HTTPS paralelas con byte ranges, reanudable.
- **Modo REPL interactivo:** `kget --interactive` con historial, todos los protocolos y edición de config en vivo.
- **GUI y CLI:** interfaz gráfica y uso por terminal.
- **Multiplataforma:** macOS, Linux y Windows.
- **Verificación SHA256:** valida ISOs y cualquier archivo con hash esperado.
- **Eventos JSONL:** progreso experimental en formato legible por scripts y agentes.
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

# Modo turbo (paralelo, reanudable)
kget -a https://example.com/grande.iso

# Guardar en ubicación específica
kget -O ~/Downloads/archivo.zip https://example.com/archivo.zip

# Verificar SHA256 esperado
kget --sha256 <hash> https://example.com/imagen.iso

# Enlace magnet (detectado automáticamente)
kget "magnet:?xt=urn:btih:HASH..."

# FTP anónimo
kget --ftp ftp://ftp.gnu.org/gnu/emacs/emacs-28.2.tar.gz

# FTP autenticado
kget --ftp ftp://usuario:clave@servidor/archivo.zip

# SFTP con contraseña en la URL
kget --sftp sftp://usuario:clave@servidor/ruta/archivo.dat

# SFTP con clave SSH (usa agente SSH o ~/.ssh/id_*)
kget --sftp sftp://usuario@servidor/ruta/archivo.dat
```

## Modo Interactivo

```bash
kget --interactive
```

Abre un REPL con banner ASCII, historial de comandos y soporte para todos los protocolos:

```
kget> download -a -o ~/Downloads/ubuntu.iso https://releases.ubuntu.com/...
kget> download --sftp sftp://user@servidor/backups/db.sql.gz
kget> download magnet:?xt=urn:btih:...
kget> config set connections 8
kget> config set speed-limit 1048576
kget> help
```

## Opciones principales

| Flag | Descripción |
|------|-------------|
| `-a, --advanced` | Modo turbo con conexiones paralelas (reanudable) |
| `-O <path>` | Archivo o carpeta de salida |
| `-q, --quiet` | Salida mínima |
| `-p <proxy>` | Proxy HTTP/SOCKS5 |
| `-l <bytes>` | Límite de velocidad en bytes/s |
| `--sha256 <hash>` | Verifica el archivo final contra un hash SHA256 esperado |
| `--jsonl` | Emite eventos JSON Lines experimentales para scripts y agentes |
| `--ftp` | Usar protocolo FTP |
| `--sftp` | Usar protocolo SFTP (contraseña o autenticación por clave) |
| `--gui` | Abre la interfaz gráfica |
| `-i, --interactive` | Abre el modo REPL interactivo |

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
