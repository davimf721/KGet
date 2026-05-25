<img width="1000" height="500" alt="KGet Banner" src="https://github.com/user-attachments/assets/d0888e3f-90a2-42d6-a9aa-b216dc36f1f4" />

# KGet v1.7.0

**Un gestor de descargas rápido y moderno escrito en Rust.**  
HTTP/HTTPS · FTP/SFTP · WebDAV · BitTorrent · Metalink · yt-dlp · CLI · GUI · Biblioteca

[English](../README.md) | [Português](README.pt-BR.md) | [Español](README.es.md)

---

## ¿Por qué KGet?

La mayoría de las herramientas de descarga son demasiado simples (clones de wget con una sola conexión) o demasiado pesadas (apps Electron). KGet es un **motor de descarga nativo en Rust** que ofrece:

- **Descargas turbo con múltiples conexiones** — divide archivos en rangos de bytes paralelos (hasta 32× más rápido en conexiones rápidas)
- **Cobertura completa de protocolos** — HTTP/HTTPS, FTP/SFTP, WebDAV, magnet links y yt-dlp en un solo binario
- **Tres frontends, un motor** — app macOS nativa en SwiftUI, GUI multiplataforma egui (Linux/Windows) y una CLI completa
- **Una biblioteca Rust reutilizable** — incorpora el motor de descarga en tu app con una API builder fluida, errores tipados y canales de eventos

---

## Características

### Protocolos de Descarga
| Protocolo | Flag | Notas |
|-----------|------|-------|
| HTTP / HTTPS | *(auto)* | Multi-conexión, reanudable, gzip/brotli/lz4, proxy |
| FTP | `--ftp` | Autenticado o anónimo |
| SFTP | `--sftp` | Contraseña o clave; verificación de host-key |
| WebDAV | `--webdav` o `webdav://` | HTTP Basic auth en la URL |
| Magnet / BitTorrent | *(auto-detectado)* | Cliente torrent nativo (feature `torrent-native`) |
| Metalink `.meta4` | `--metalink` | Múltiples mirrors con fallback, SHA-256 verificado (RFC 5854) |
| Sitios de video | `--ytdlp` o *(auto-detectado)* | YouTube, Vimeo, Twitch, TikTok, Instagram… via yt-dlp |

### Motor de Descarga
- **Modo turbo** (`-a`) — conexiones paralelas por rangos de bytes, reanudable
- **Descarga en lote** (`--batch urls.txt`) — una URL por línea, `#` = comentario, todas en paralelo
- **Programación** (`--at "HH:MM"`) — espera hasta la hora local especificada
- **Límite de velocidad** (`-l <bytes/s>`) — throttle global por token-bucket
- **Headers HTTP personalizados** (`-H "Nombre: Valor"`) — inyecta headers arbitrarios
- **Auto-extracción de archivos** (`--extract`) — descomprime después de la descarga (`.zip`, `.tar.gz`, `.7z`, …)
- **Verificación SHA-256** (`--sha256 <hash>`) — error si no coincide; nunca acepta archivos corruptos
- **Archivos de checksum sidecar** — verifica contra archivos GNU/BSD `.sha256sum`
- **Content-Disposition** — usa nombres de archivo sugeridos por el servidor
- **Seguridad de nombres de archivo** — rechaza bytes nulos, path traversal, nombres reservados de Windows y nombres >255 bytes

### Integridad y Seguridad
- **Checksums multi-algoritmo** — SHA-256, SHA-512, SHA-1, MD5, BLAKE3
- **Verificación de host-key SFTP** — comprueba `~/.ssh/known_hosts`; error ante divergencia
- **Política de reintentos** — solo reintenta en 5xx y errores de red; falla inmediata en 4xx
- **Eventos JSONL** (`--jsonl`) — progreso legible por máquina para scripts y agentes

### Historial y Persistencia
- **Historial de descargas** — cada descarga registrada en `history.json`; `--history` / `--history-clear`
- **REPL interactivo** (`--interactive`) — historial completo, todos los protocolos, edición de config en vivo

### GUIs
- **App macOS nativa** — SwiftUI, sidebar `NavigationSplitView`, monitor de portapapeles, drag-and-drop de URLs, sparkline de velocidad, pestaña de historial, Share Extension, menú bar
- **GUI multiplataforma egui** (`--gui`) — diseño inspirado en Apple, modo oscuro/claro adaptativo, navegación en sidebar, barra de progreso con shimmer

### Biblioteca
- **API builder fluida** — `kget::builder(url).connections(8).sha256("…").download()?`
- **Errores tipados** — enum `KgetError` con impls `From` para `reqwest::Error`, `io::Error`
- **Canal de eventos** — `.spawn()` retorna `(JoinHandle, Receiver<DownloadEvent>)`
- **API Async** — `.download_async()` / `.download_all_async()` con `--features async`
- **Descarga en memoria** — `.download_to_bytes()` y `.download_to_reader()`
- **Batch builder** — `kget::batch([…]).concurrency(4).download_all()`

---

## Instalación

### Homebrew (macOS / Linux)

```bash
brew tap davimf721/kget
brew install kget                           # solo CLI
brew install kget --with-gui                # con interfaz gráfica egui
brew install kget --with-torrent            # con cliente BitTorrent nativo
brew install kget --with-gui --with-torrent # todas las características opcionales
```

### Binarios precompilados

Descarga la última versión desde [Releases](https://github.com/davimf721/KGet/releases):
- **macOS** — `KGet-1.7.0-macOS-Native.dmg` (app SwiftUI nativa, sin necesidad de Rust)
- **Linux/Windows** — binario CLI o GUI (ver assets del release)

### Desde crates.io

```bash
cargo install Kget --features gui   # con GUI egui
cargo install Kget                  # solo CLI
```

### Desde código fuente

```bash
# Toolchain Rust: https://rustup.rs

# Dependencias Linux (Debian/Ubuntu)
sudo apt install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
                    libxkbcommon-dev libssl-dev pkg-config

git clone https://github.com/davimf721/KGet.git
cd KGet
cargo build --release --features gui
./target/release/kget --gui
```

---

## Uso

### Descargas básicas

```bash
# HTTP/HTTPS
kget https://example.com/archivo.zip

# Guardar en ubicación específica
kget -O ~/Downloads/miarchivo.zip https://example.com/archivo.zip

# Modo turbo — conexiones paralelas, reanudable
kget -a https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso

# Modo silencioso
kget -q https://example.com/archivo.zip
```

### Protocolos

```bash
# FTP (anónimo o autenticado)
kget --ftp ftp://ftp.gnu.org/gnu/emacs/emacs-28.2.tar.gz
kget --ftp ftp://usuario:clave@servidor/archivo.zip

# SFTP (contraseña o clave)
kget --sftp sftp://usuario:clave@servidor/ruta/archivo.dat
kget --sftp sftp://usuario@servidor/ruta/archivo.dat

# WebDAV (auto-detectado por scheme)
kget webdav://archivos.miservidor.com/compartido/informe.pdf
kget webdavs://usuario:clave@nas.local/backups/db.tar.gz

# Magnet link (auto-detectado)
kget "magnet:?xt=urn:btih:HASH&dn=nombrearchivo"

# Metalink — prueba mirrors en orden de prioridad, verifica SHA-256
kget --metalink ubuntu-24.04.meta4
kget https://releases.ubuntu.com/ubuntu.meta4
```

### Descargas de video

```bash
# Auto-detectado por el host de la URL
kget https://www.youtube.com/watch?v=dQw4w9WgXcQ

# Flag explícita con calidad
kget --ytdlp --quality 1080p https://vimeo.com/123456
kget --ytdlp --quality audio https://www.youtube.com/watch?v=dQw4w9WgXcQ

# Calidades: best, 1080p, 720p, 480p, 360p, audio
```

### Lote y programación

```bash
# Lote — una URL por línea, # = comentario
kget --batch urls.txt -O ~/Downloads/

# Programar para las 23h de hoy
kget --at "23:00" -a https://example.com/archivo-grande.iso
```

### Checksums y verificación

```bash
# Verificar SHA-256 esperado
kget --sha256 abc123def456... https://example.com/archivo.iso

# Auto-extraer después de la descarga
kget --extract https://example.com/archivo.tar.gz

# Headers personalizados
kget -H "Authorization: Bearer token123" -H "Accept: application/json" https://api.ejemplo.com/exportar
```

### Historial

```bash
kget --history                    # lista las últimas 50 descargas
kget --history-clear              # elimina todas las entradas
kget --history-clear completed    # elimina solo completadas/canceladas
```

### REPL Interactivo

```bash
kget --interactive
```

```
kget> download -a -o ~/Downloads/ubuntu.iso https://releases.ubuntu.com/...
kget> download --sftp sftp://usuario@servidor/backups/db.sql.gz
kget> download --ytdlp --quality 720p https://youtube.com/watch?v=...
kget> config set connections 8
kget> config set speed-limit 1048576
kget> history
kget> help
```

### Eventos JSONL (para scripts y agentes)

```bash
kget --jsonl -a https://example.com/archivo.iso | jq '.percent'
```

### Todas las flags CLI

| Flag | Descripción |
|------|-------------|
| `-a, --advanced` | Modo turbo — conexiones paralelas, reanudable |
| `-O <ruta>` | Archivo o carpeta de salida |
| `-q, --quiet` | Salida mínima |
| `-p <proxy>` | Proxy HTTP/SOCKS5 |
| `-l <bytes/s>` | Límite de velocidad en bytes por segundo |
| `-H "Nombre: Valor"` | Header HTTP extra (repetible) |
| `--sha256 <hash>` | Verificar SHA-256 después de la descarga |
| `--extract` | Auto-extraer archivos después de la descarga |
| `--at "HH:MM"` | Programar descarga para hora local específica |
| `--batch <archivo>` | Descargar todas las URLs de un archivo |
| `--ftp` | Usar protocolo FTP |
| `--sftp` | Usar protocolo SFTP |
| `--webdav` | Usar protocolo WebDAV |
| `--ytdlp` | Enrutar por yt-dlp (auto-detectado para sitios de video) |
| `--quality <q>` | Calidad yt-dlp: `best`, `1080p`, `720p`, `480p`, `360p`, `audio` |
| `--metalink` | Descargar desde manifiesto Metalink |
| `--history` | Mostrar historial de descargas |
| `--history-clear [completed]` | Limpiar historial |
| `--jsonl` | Emitir eventos JSON Lines a stdout |
| `--gui` | Abrir interfaz gráfica egui |
| `-i, --interactive` | Modo REPL interactivo |

---

## Uso como Biblioteca

KGet también es una biblioteca Rust reutilizable. Agrégala a tu proyecto:

```toml
[dependencies]
Kget = "1.7.0"
```

### API Builder (recomendada)

```rust
use kget::KgetError;

// Descarga simple
kget::builder("https://example.com/archivo.zip")
    .output("./downloads/")
    .connections(8)
    .sha256("abc123...")
    .download()?;

// Lote paralelo con canal de eventos
let results = kget::batch([
    "https://mirror1.example.com/archivo.iso",
    "https://mirror2.example.com/otro.tar.gz",
])
.concurrency(4)
.output_dir("./downloads/")
.download_all();

// Canal de eventos
let (handle, rx) = kget::builder("https://example.com/grande.iso")
    .connections(4)
    .spawn()?;

for event in rx {
    match event {
        kget::DownloadEvent::Progress { percent, .. } => print!("\r{:.1}%", percent),
        kget::DownloadEvent::Completed { path, .. } => println!("\nGuardado en {}", path),
        kget::DownloadEvent::Error(e) => eprintln!("Error: {}", e),
        _ => {}
    }
}
handle.join().ok();
```

Consulta [LIB.es.md](LIB.es.md) para la referencia completa de la biblioteca.

---

## Build

```bash
# Solo CLI (sin GUI)
cargo build --release

# Con GUI egui (Linux/Windows/macOS)
cargo build --release --features gui

# Con cliente torrent nativo
cargo build --release --features torrent-native

# App macOS nativa + DMG (requiere Xcode)
./build-native-macos.sh

# Cross-compilar Linux/Windows desde macOS (requiere zig)
brew install zig && cargo install cargo-zigbuild
./build-cross.sh
```

## Tests

```bash
cargo test
cargo test --lib --test unit_tests
cargo test --test mock_server_tests
./run-tests.sh
```

---

## Soporte de Plataformas

| Plataforma | CLI | GUI egui | App Nativa |
|------------|-----|----------|------------|
| macOS | ✅ | ✅ | ✅ SwiftUI DMG |
| Linux | ✅ | ✅ | — |
| Windows | ✅ | ✅ | — |

---

## Links

- [Changelog](../CHANGELOG.md)
- [Referencia de Biblioteca (LIB.es.md)](LIB.es.md)
- [Arquitectura](../docs/ARCHITECTURE.md)
- [crates.io](https://crates.io/crates/Kget)
- [Contribuir](../CONTRIBUTING.md)

## Comunidad

- [Dev.to](https://dev.to/davimf7221/kelpsget-v014-modern-download-manager-in-rust-4b9f)
- [r/rust](https://www.reddit.com/r/rust/comments/1kt69vh/after_5_months_of_development_i_finally_released/)
- [PitchHut](https://www.pitchhut.com/project/kelpsget)

## Licencia

MIT — ver [LICENSE](../LICENSE)
