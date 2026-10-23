<img width="1000" height="500" alt="KGet Banner" src="https://github.com/user-attachments/assets/d0888e3f-90a2-42d6-a9aa-b216dc36f1f4" />

# KGet v1.8.0

**Un gestor de descargas rápido y moderno escrito en Rust.**  
HTTP/HTTPS · FTP/SFTP · WebDAV · BitTorrent · Metalink · yt-dlp · Hugging Face · Ollama · CLI · TUI · GUI · MCP · Biblioteca

[English](../README.md) | [Português](README.pt-BR.md) | [Español](README.es.md)


> **1.8.0** lleva KGet a la era de los agentes: daemon con cola persistente, servidor MCP,
> extensión de navegador, TUI, fuentes de modelos de IA, descargas turbo HTTP/2 e historial
> SQLite — con extracción de archivos reforzada y credenciales en el llavero del sistema.
> Ver el [CHANGELOG](CHANGELOG.es.md).

---

## ¿Por qué KGet?

La mayoría de las herramientas de descarga son demasiado simples (clones de wget con una sola conexión) o demasiado pesadas (apps Electron). KGet es un **motor de descarga nativo en Rust** que ofrece:

- **Descargas turbo con múltiples conexiones** — divide archivos en rangos de bytes paralelos (hasta 32× más rápido en conexiones rápidas)
- **Cobertura completa de protocolos** — HTTP/HTTPS (HTTP/2), FTP/SFTP, WebDAV, magnet links, yt-dlp y modelos de IA de Hugging Face y Ollama en un solo binario
- **Varios frontends, un motor** — app macOS nativa en SwiftUI, GUI multiplataforma egui, interfaz de terminal, una CLI completa, una extensión de navegador y un servidor MCP para agentes de IA, todos con la misma cola de trabajos (opcionalmente en un daemon en segundo plano)
- **Una biblioteca Rust reutilizable** — incorpora el motor de descarga en tu app con una API builder fluida, errores tipados y canales de eventos

---

## Características

### Protocolos de Descarga
| Protocolo | Flag | Notas |
|-----------|------|-------|
| HTTP / HTTPS | *(auto)* | Multi-conexión, reanudable, proxy HTTP/HTTPS/SOCKS5, archivos idénticos a los del servidor (sin descompresión) |
| FTP | `ftp://` *(auto)* o `--ftp` | Autenticado o anónimo, reanudable (`REST`) |
| SFTP | `sftp://` *(auto)* o `--sftp` | Contraseña o clave; verificación de host-key; reanudable |
| WebDAV | `--webdav` o `webdav://` | HTTP Basic auth en la URL |
| Magnet / BitTorrent | *(auto-detectado)* | Cliente torrent nativo (feature `torrent-native`) |
| Metalink `.meta4` / `.metalink` | *(auto)* o `--metalink` | Múltiples mirrors con fallback, SHA-256/512/MD5 verificado (RFC 5854) |
| Sitios de video | `--ytdlp` o *(auto-detectado)* | YouTube, Vimeo, Twitch, TikTok, Instagram… via yt-dlp |
| Hugging Face | `hf://` o `huggingface.co/…` *(auto)* | Modelos, datasets, spaces; `--include`/`--exclude`, `--revision`, `--hf-cache`; SHA-256 de LFS verificado |
| Ollama | `ollama://` *(auto)* | Descarga al directorio de modelos de Ollama (o archivos normales con `-O`); digests verificados |

### Motor de Descarga
- **Modo turbo** (`-a`) — conexiones paralelas por rangos de bytes en un motor asíncrono con work stealing, reanudable
- **HTTP/2 y HTTP/3** (`--http-version`) — HTTP/2 negociado automáticamente; HTTP/3 experimental tras una flag de compilación
- **Descarga en lote** (`--batch urls.txt`) — una URL por línea, `#` = comentario, `-j/--jobs N` descargas simultáneas (4 por defecto)
- **Programación** (`--at "HH:MM"`) — espera hasta la hora local especificada
- **Límite de velocidad** (`-l <bytes/s>`) — throttle global por token-bucket
- **Headers HTTP personalizados** (`-H "Nombre: Valor"`) — inyecta headers arbitrarios
- **Auto-extracción de archivos** (`--extract`) — descomprime después de la descarga (`.zip`, `.tar.gz`, `.7z`, …)
- **Verificación SHA-256** (`--sha256 <hash>`) — error si no coincide; nunca acepta archivos corruptos
- **Cualquier checksum** (`--checksum sha512:<hex>`) y **archivos sidecar** (`--checksum-url …/SHA256SUMS`) — formatos GNU, BSD y solo-hash
- **Content-Disposition** — usa nombres de archivo sugeridos por el servidor
- **Seguridad de nombres de archivo** — rechaza bytes nulos, path traversal, nombres reservados de Windows y nombres >255 bytes

### Integridad y Seguridad
- **Checksums multi-algoritmo** — SHA-256, SHA-512, SHA-1, MD5, BLAKE3
- **Verificación de host-key SFTP** — comprueba `~/.ssh/known_hosts`; error ante divergencia
- **Política de reintentos** — reintenta en 408/429/5xx y errores de red (respeta `Retry-After`); falla inmediata en los demás 4xx
- **Contraseñas guardadas** — contraseñas de FTP/SFTP/WebDAV/proxy y tokens de HF en el llavero del sistema (`--save-credentials`, `--credentials`); nunca en `config.json`
- **Eventos JSONL** (`--jsonl`) — eventos versionados (`"v":1`) legibles por máquina para todos los protocolos

### Historial y Persistencia
- **Historial de descargas** — cada descarga registrada en SQLite (`history.db`, `history.json` se migra); búsqueda con filtros, etiquetas y `--json`
- **Reglas inteligentes** — organiza en carpetas, añade etiquetas, omite o enlaza los duplicados
- **Daemon en segundo plano** — `kget daemon` + `kget queue …`: cola persistente que sobrevive a reinicios
- **REPL interactivo** (`--interactive`) — todas las opciones de descarga de la CLI, lotes, retry, checksums, autocompletado con Tab, historial persistente, Ctrl+C para pausar

### GUIs
- **App macOS nativa** — SwiftUI, sidebar `NavigationSplitView`, monitor de portapapeles, drag-and-drop de URLs, sparkline de velocidad, pestaña de historial, Share Extension, menú bar
- **GUI multiplataforma egui** (`--gui`) — diseño inspirado en Apple, modo oscuro/claro adaptativo, navegación en sidebar, barra de progreso con shimmer
- **Interfaz de terminal** (`kget tui`) — cola, detalles e historial a pantalla completa en cualquier terminal
- **Extensión de navegador** — Chrome, Edge, Brave, Firefox: envía enlaces (o todas las descargas) a KGet

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
- **macOS** — `KGet-1.8.0-macOS-Native.dmg` (app SwiftUI nativa, sin necesidad de Rust)
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

### Modelos de IA (Hugging Face, Ollama)

```bash
kget hf://Qwen/Qwen2.5-0.5B-Instruct                          # repositorio completo en ./Qwen2.5-0.5B-Instruct/
kget hf://TheBloke/Llama-2-7B-GGUF --include "*Q4_K_M.gguf"    # solo los archivos que coinciden
kget hf://org/repo --revision v1.0 --exclude "*fp32*" -j 8     # tag/rama/commit, 8 archivos a la vez
kget hf://datasets/org/nombre/data/train.parquet               # un archivo de un dataset
kget hf://org/repo --hf-cache                                  # en ~/.cache/huggingface/hub para transformers
kget ollama://llama3.2:3b                                      # en ~/.ollama/models, listo para `ollama run`
kget ollama://llama3.2 -O ./modelos/                           # archivos normales
```

Los repositorios restringidos o privados usan `HF_TOKEN`, `HUGGING_FACE_HUB_TOKEN`, el token de `hf auth login` o
un token guardado con `kget --credentials add hf://token@huggingface.co`. Las descargas de modelos interrumpidas
omiten los archivos ya completos en la siguiente ejecución.

### Selección de archivos y seeding de torrents

```bash
kget --list-files "magnet:?xt=urn:btih:HASH"                   # lista numerada de archivos y sale
kget --select 1,3-5 "magnet:?xt=urn:btih:HASH"                 # solo estos archivos
kget --select-glob "*.mkv" ~/Downloads/serie.torrent           # solo las rutas que coinciden
kget --seed-ratio 1.5 --seed-time 2h "magnet:?xt=urn:btih:HASH"  # seguir compartiendo después
```

La selección, `--list-files` y `--seed-time` requieren el cliente nativo (`--features torrent-native`); el backend
Transmission admite `--select` y `--seed-ratio`. Con `--jsonl`, los eventos `torrent_stats` informan de peers,
seeders, velocidades, ratio y trackers.

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

### Búsqueda en el historial

```bash
kget --history                    # lista las últimas 50 descargas
kget --history --status failed --domain example.com --since 7d
kget --history --type video --sort largest --history-limit 10
kget --history --search ubuntu --tag linux --until 2026-10-01
kget --history --json             # array JSON (con --jsonl: un objeto por línea)
kget --history-clear              # elimina todas las entradas
kget --history-clear completed    # conserva solo los fallos
kget --history-clear failed       # elimina solo los fallos
```

Filtros: `--status` (completed, failed, cancelled, handed_off), `--domain` (incluye subdominios), `--type`
(video, audio, image, archive, document, disk, package o extensiones), `--protocol`, `--since`/`--until`
(`today`, `7d`, `12h`, `AAAA-MM-DD`; UTC), `--search`, `--tag`; `--sort newest|oldest|largest|smallest|name|fastest`,
`--history-limit`, `--history-offset`. El comando `history` del REPL acepta los mismos filtros.

### Reglas inteligentes, etiquetas y duplicados

```bash
kget --rules add name=ISOs ext=iso domain=ubuntu.com,debian.org folder="ISOs/{domain}" tags=linux
kget --rules test https://releases.ubuntu.com/24.04/ubuntu.iso   # qué regla coincide y adónde iría
kget --rules enable                     # organiza todas las descargas (o --organize para una ejecución)
kget --rules enable builtins            # usa también las carpetas integradas Videos/Music/Images/…
kget --rules list                       # también: remove <nombre>, disable [nombre]
kget --rules suggest                    # reglas sugeridas a partir del historial (--ai consulta a Claude, --apply las añade)
kget --tag trabajo --tag q4 https://example.com/informe.pdf
kget --duplicates skip https://example.com/archivo.zip   # ¿ya descargado? skip | ask | download | link
kget --dedupe https://example.com/copia.iso              # ¿contenido idéntico ya guardado? enlace duro
```

Las reglas viven en `config.json` y se prueban en orden (gana la primera que coincida). Coinciden por dominio,
regex/glob de la URL, extensión, categoría, tipo MIME, tamaño o protocolo, y pueden fijar carpeta (`{category}`,
`{domain}`, `{ext}`, `{protocol}`, `{date:%Y-%m}`), etiquetas, conexiones, límite de velocidad, extracción o checksum
obligatorio. `-O` siempre gana a la carpeta de la regla. `--rules suggest --ai` requiere `ANTHROPIC_API_KEY` y envía
solo recuentos agregados, tras tu confirmación. Sin `--duplicates`, KGet pregunta en el terminal y vuelve a
descargar en los demás casos.

### Contraseñas guardadas (llavero)

```bash
kget --save-credentials sftp://alice@servidor/backups/db.tar.gz   # pregunta una vez y guarda en el llavero
kget sftp://alice@servidor/backups/db.tar.gz                      # la próxima vez: sin preguntar
kget --credentials add ftp://bob@files.example.com                # guardar de antemano
kget --credentials list                                           # solo los destinos, nunca las contraseñas
kget --credentials delete ftp://bob@files.example.com
kget --credentials status                                         # qué llavero se usa
```

Las contraseñas van al Llavero de macOS, al Administrador de credenciales de Windows o a Secret Service (Linux/BSD).
Los destinos son `esquema://usuario@host[:puerto]` (sftp, ftp, webdav, webdavs o una URL de proxy),
`ssh-key:///ruta/a/la/clave` para la frase de la clave, o `hf://token@huggingface.co`. `--save-config` también guarda
ahí la contraseña del proxy. Define `KGET_KEYCHAIN=off` para desactivarlo.

### HTTP/2 y HTTP/3

```bash
kget -a https://example.com/grande.iso                       # auto: HTTP/2 vía ALPN si se ofrece, si no HTTP/1.1
kget -a --http-version 1.1 https://example.com/grande.iso    # nunca HTTP/2
kget -a --http-version 2 http://h2c.example.com/grande.iso   # HTTP/2 con conocimiento previo (también sin TLS)
```

Define un valor por defecto con `optimization.http_version` en `config.json`. `--http-version 3` (QUIC) es
experimental y requiere `RUSTFLAGS="--cfg reqwest_unstable" cargo build --release --features http3`; los demás
builds lo rechazan con un mensaje claro. Las descargas Metalink piden fragmentos a todos los mirrors a la vez y
descartan los que fallan.

### Daemon y cola

```bash
kget daemon                          # inicia el daemon en segundo plano (--foreground para quedarse en primer plano)
kget queue add -a https://example.com/grande.iso --wait   # encola una descarga y la sigue
kget queue list                      # también: status <id>, pause, resume, cancel, rm, watch [ids]
kget https://example.com/archivo.zip --via-daemon         # cualquier descarga a través del daemon
kget daemon status                   # código de salida 3 si no está en marcha
kget daemon stop                     # los trabajos sin terminar continúan en el siguiente inicio
```

El daemon mantiene una cola persistente (`jobs.db`), ejecuta `daemon.max_concurrent` trabajos a la vez (`-j`) y
guarda en `daemon.download_dir` (por defecto `~/Downloads`); los clientes no pueden escribir fuera de él salvo que
`daemon.allow_outside_download_dir` esté activo. Escucha en un socket Unix `0600` (TCP de loopback con token en
Windows). Consulta [docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md#daemon-and-mcp) para el protocolo y el modelo de
seguridad.

### Uso desde agentes de IA (MCP)

```bash
claude mcp add kget -- kget mcp                           # Claude Code
claude mcp add kget -- kget mcp --root ~/Downloads/agente # limita el agente a una carpeta
```

`kget mcp` habla el Model Context Protocol por stdin/stdout. Herramientas: `download`, `download_status`,
`list_downloads`, `cancel_download`, `pause_download`, `resume_download`, `search_history`, `verify_checksum`,
`get_config`. Toda escritura (y todo archivo que lee `verify_checksum`) queda dentro de `--root` (por defecto, el
directorio de descargas del daemon). Usa el daemon en marcha, así que las descargas sobreviven a la sesión del
agente (`--start-daemon` inicia uno; `--no-daemon` lo mantiene todo en el propio proceso).

### Interfaz de terminal

```bash
kget tui            # usa el daemon si está en marcha; si no, descarga en este proceso
kget tui --daemon   # inicia/usa el daemon: las descargas continúan al salir
kget tui --local    # siempre en el propio proceso
```

Teclas: `a` añade una URL, `p`/Espacio pausa o reanuda, `c` cancela, `r` reintenta, `d` elimina, `o` abre,
`1`/`2` cola o historial, `/` busca en el historial, `f` filtra por estado, `?` ayuda, `q` sale. Incluida en los
builds por defecto (feature `tui`).

### Extensión de navegador

```bash
kget native-host install                    # registra en todos los navegadores instalados
kget native-host install --browser firefox  # o chrome, chromium, edge, brave
kget native-host uninstall
```

Después carga `browser-extension/` como extensión sin empaquetar (Chrome, Edge, Brave, Chromium) o como
complemento temporal (Firefox 128+). Añade "Download with KGet" al menú contextual, un popup con progreso en vivo e
interceptación opcional de las descargas del navegador. Las descargas pasan por el daemon (que se inicia solo).
Consulta [browser-extension/README.md](../browser-extension/README.md).

### REPL Interactivo

```bash
kget --interactive
```

```
kget> https://example.com/archivo.zip       # pegar una URL la descarga
kget> cd ~/Downloads                        # carpeta donde esta sesión guarda los archivos
kget> set turbo on                          # valores de la sesión: turbo, quality, retries, timeout
kget> download -a -o "ubuntu.iso" --checksum-url https://host/SHA256SUMS https://releases.ubuntu.com/...
kget> download -l 1048576 -p socks5://127.0.0.1:1080 https://example.com/grande.iso   # solo esta descarga
kget> download --ytdlp --quality 720p https://youtube.com/watch?v=...
kget> batch urls.txt -j 8                   # descargas en paralelo desde un archivo
kget> info https://example.com/archivo.zip  # tamaño, tipo, ¿reanudable? (sin descargar)
kget> history failed                        # también: history 20 | completed | search <texto> | clear
kget> retry 2                               # repite una entrada; continúa el .part
kget> open last                             # también: reveal, extract, checksum, verify
kget> config set connections 8              # se guarda en config.json (config show | path | reset)
kget> doctor
kget> help download
```

Tab completa comandos, flags, claves de config y rutas; ↑/↓ recorren los comandos anteriores
(guardados entre sesiones). Ctrl+C detiene la descarga en curso y conserva los datos parciales — `retry last` la continúa.

### Eventos JSONL (para scripts y agentes)

```bash
kget --jsonl -a https://example.com/archivo.iso | jq -c 'select(.event=="progress") | .percent'
```

Cada línea es un objeto JSON con `"v": 1` y un campo `event` (`rule`, `started`, `progress`, `status`,
`files`, `file_progress`, `torrent_stats`, `phase`, `verified`, `duplicate`, `saved`, `completed`, `error`). El evento `phase` indica
verificación (`{"event":"phase","phase":"verifying","algorithm":"sha256"}`) o extracción
(`{"event":"phase","phase":"extracting"}`). Use los campos estructurados, nunca el texto de `status`.
Los nuevos eventos y campos no cambian `"v"`; ignore los que no conozca. Vea el formato completo en el
[README en inglés](../README.md#jsonl-events-for-scripts-and-agents).

### Todas las flags CLI

Ejecuta `kget --help` para la lista completa.

| Flag | Descripción |
|------|-------------|
| `-a, --advanced` | Modo turbo — conexiones paralelas, reanudable |
| `--http-version <v>` | Versión HTTP del turbo: `auto` (HTTP/2 vía ALPN), `1.1`, `2`, `3` (requiere build `http3`); config: `optimization.http_version` |
| `-O, --output <ruta>` | Archivo o carpeta de salida |
| `-q, --quiet` | Salida mínima |
| `-c, --continue` | Continúa una descarga parcial (`<archivo>.part`) |
| `--no-clobber` | Omite la descarga si el archivo ya existe |
| `--retries <n>` | Intentos por petición (3 por defecto) |
| `--timeout <s>` | Timeout de conexión / por lectura (60 por defecto) |
| `-p, --proxy <url>` | Proxy HTTP, HTTPS o SOCKS5 |
| `--proxy-user`, `--proxy-pass` | Credenciales del proxy (la contraseña nunca se escribe en `config.json`) |
| `--proxy-type <t>` | `http`, `https`, `socks5` (se deduce de la URL del proxy si se omite) |
| `-l, --limit <bytes/s>` | Límite de velocidad en bytes por segundo (`0` = ilimitado) |
| `--save-config` | Guarda las flags de proxy/límite/torrent en `config.json` (contraseña del proxy → llavero) |
| `--save-credentials` | Guarda en el llavero las contraseñas que usa esta descarga |
| `--credentials <acción> [destino]` | `list`, `status`, `add <destino>`, `delete <destino>` |
| `-H, --header "Nombre: Valor"` | Header HTTP extra (repetible) |
| `--sha256 <hash>` | Verificar SHA-256 después de la descarga |
| `--checksum <alg:hex>` | Verificar con sha256, sha512, sha1, md5 o blake3 |
| `--checksum-url <url>` | Verificar contra un archivo de checksums (GNU, BSD o solo hash) |
| `--extract` | Auto-extraer archivos después de la descarga |
| `--at "HH:MM"` | Programar descarga para hora local específica |
| `--batch <archivo\|->` | Descargar todas las URLs de un archivo (o stdin) |
| `-j, --jobs <n>` | Descargas simultáneas en modo lote; archivos a la vez en `hf://`/`ollama://` (4 por defecto) |
| `--include`, `--exclude <glob>` | `hf://`: solo / omite los archivos que coinciden (repetible) |
| `--revision <rev>` | `hf://`: rama, tag o commit |
| `--hf-cache` | `hf://`: escribe en la caché del hub de Hugging Face |
| `-t, --torrent` | Fuerza torrent |
| `--list-files` | Torrent: lista numerada de archivos y sale (`torrent-native`) |
| `--select <lista>`, `--select-glob <patrón>` | Torrent: descarga solo algunos archivos (repetible) |
| `--seed-ratio <r>`, `--seed-time <d>` | Torrent: sigue compartiendo hasta un ratio / durante un tiempo (`30m`, `2h`) |
| `--max-peers`, `--max-seeds`, `--torrent-port`, `--no-dht` | Ajustes del cliente torrent |
| `--organize`, `--no-organize` | Aplica (u omite) las reglas inteligentes en esta descarga |
| `--rules <acción> …` | `list`, `test <url>`, `add clave=valor…`, `remove <nombre>`, `enable\|disable [nombre\|builtins]`, `suggest` (`--ai`, `--apply`) |
| `--tag <etiqueta>` | Etiqueta guardada en el historial (repetible); con `--history`: filtra por etiqueta |
| `--duplicates <política>` | Ya descargado: `skip`, `ask`, `download`, `link` |
| `--dedupe` | Enlace duro al contenido idéntico que ya está en el historial |
| `--via-daemon` | Ejecuta la descarga a través del daemon en segundo plano |
| `--doctor` | Muestra diagnósticos y sale |
| `--ftp`, `--sftp`, `--webdav` | Fuerza FTP / SFTP / WebDAV |
| `--ytdlp` | Enrutar por yt-dlp (auto-detectado para sitios de video) |
| `--quality <q>` | Calidad yt-dlp: `best`, `1080p`, `720p`, `480p`, `360p`, `audio` |
| `--metalink` | Descargar desde manifiesto Metalink |
| `--history` | Mostrar historial (filtros: `--status`, `--domain`, `--type`, `--protocol`, `--since`, `--until`, `--search`, `--tag`, `--sort`, `--history-limit`, `--history-offset`, `--json`) |
| `--history-clear [completed\|failed]` | Limpiar historial |
| `--jsonl` | Emitir eventos JSON Lines a stdout |
| `--gui` | Abrir interfaz gráfica egui |
| `-i, --interactive` | Modo REPL interactivo |

| Subcomando | Descripción |
|------------|-------------|
| `kget daemon [--foreground] [status\|stop]` | Daemon de descargas en segundo plano |
| `kget queue add\|list\|status\|cancel\|pause\|resume\|rm\|watch` | Gestiona la cola del daemon |
| `kget mcp [--root DIR]` | Servidor MCP para agentes de IA (stdio) |
| `kget tui [--daemon\|--local]` | Interfaz de terminal |
| `kget native-host install\|uninstall` | Registra el host nativo de la extensión de navegador |

---

## Uso como Biblioteca

KGet también es una biblioteca Rust reutilizable. Agrégala a tu proyecto:

```toml
[dependencies]
Kget = "1.8.0"
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
    .spawn();

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

Además del builder, la biblioteca expone la misma cola de trabajos que usan el daemon y las GUIs (`kget::JobManager`),
el despachador compartido (`kget::runner`), la búsqueda en el historial (`kget::queue::HistoryQuery`), el llavero
(`kget::credentials`), descargas de Hugging Face/Ollama (`kget::hf`, `kget::ollama`) y un cliente del daemon.
Consulta [LIB.es.md](LIB.es.md) y el [LIB.md en inglés](../LIB.md) (más actualizado) para la referencia completa.

---

## Build

```bash
# Solo CLI (sin GUI)
cargo build --release

# Con GUI egui (Linux/Windows/macOS)
cargo build --release --features gui

# Con cliente torrent nativo
cargo build --release --features torrent-native

# CLI mínima (sin llavero ni interfaz de terminal)
cargo build --release --no-default-features

# HTTP/3 experimental (QUIC)
RUSTFLAGS="--cfg reqwest_unstable" cargo build --release --features http3

# App macOS nativa + DMG (requiere Xcode)
./build-native-macos.sh

# Cross-compilar Linux/Windows desde macOS (requiere zig)
brew install zig && cargo install cargo-zigbuild
./build-cross.sh
```

### Features

| Flag | Por defecto | Añade |
|------|-------------|-------|
| `keychain` | ✅ | Contraseñas guardadas en el llavero del sistema (Rust puro) |
| `tui` | ✅ | Interfaz de terminal `kget tui` (ratatui, Rust puro) |
| `gui` | | GUI egui, `torrent-native`, notificaciones |
| `torrent-native` | | Cliente BitTorrent integrado (librqbit): selección de archivos, seeding, estadísticas del swarm |
| `torrent-transmission` | | Cliente RPC de Transmission |
| `http3` | | HTTP/3 experimental; requiere `RUSTFLAGS="--cfg reqwest_unstable"` |
| `async` | | Helpers `download_async()` / `download_all_async()` del builder |

## Tests

```bash
cargo test
cargo test --lib --test unit_tests
cargo test --test mock_server_tests
cargo test --test robustness_tests  # disco lleno, cancelar a mitad de fragmento, archivo que cambia, Range ignorado
cargo test --test daemon_tests --test mcp_tests --test native_host_tests
cargo test -p kget-ffi              # pruebas del engine UniFFI
node --test browser-extension/tests/
cargo fuzz run url                  # fuzzing (cargo-fuzz, nightly; ver fuzz/)
./run-tests.sh
```

---

## Soporte de Plataformas

| Plataforma | CLI / TUI / daemon | GUI egui | App Nativa | Extensión de navegador |
|------------|--------------------|----------|------------|------------------------|
| macOS | ✅ | ✅ | ✅ SwiftUI DMG | ✅ Navegadores Chromium, Firefox |
| Linux | ✅ | ✅ | — | ✅ Navegadores Chromium, Firefox |
| Windows | ✅ | ✅ | — | ✅ Navegadores Chromium, Firefox |

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
