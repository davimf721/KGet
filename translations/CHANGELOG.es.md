# Registro de cambios

[English](../CHANGELOG.md) | [Português](CHANGELOG.pt-BR.md) | [Español](CHANGELOG.es.md)

Todos los cambios notables en este proyecto serán documentados en este archivo.

El formato está basado en [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
y este proyecto se adhiere al [Versionado Semántico](https://semver.org/spec/v2.0.0.html).

## [Sin publicar]

### Añadido
- **Descarga de modelos de IA (`hf://`, `ollama://`):** `kget hf://org/repo[@rev][/ruta]` descarga un modelo,
  dataset (`hf://datasets/…`) o space (`hf://spaces/…`) de Hugging Face, y las URLs `huggingface.co/…/resolve|blob|tree/…`
  también funcionan. Filtra archivos con globs en `--include`/`--exclude`, elige rama, tag o commit con `--revision`
  y usa `--hf-cache` para escribir en la caché estándar del hub, de modo que `transformers` reutilice los archivos.
  Los archivos LFS se verifican con su SHA-256. El token se lee de `HF_TOKEN`, `HUGGING_FACE_HUB_TOKEN`, el archivo
  de token de `hf auth login` o el llavero (`hf://token@huggingface.co`).
  `kget ollama://llama3.2:3b` descarga un modelo al directorio de modelos de Ollama (`OLLAMA_MODELS`), para que
  `ollama run` lo encuentre, y verifica el digest de cada blob. Con `-O <dir>` guarda archivos normales. Ambos
  descargan `-j` archivos a la vez con el motor turbo, y una descarga interrumpida omite los archivos ya completos.
- **Daemon y cola en segundo plano:** `kget daemon` (`--foreground`, `status`, `stop`) ejecuta un servicio local de
  descargas con cola persistente (`jobs.db`): los trabajos sin terminar continúan tras reiniciar. Gestiónalo con
  `kget queue add|list|status|cancel|pause|resume|rm|watch`, o envía una sola descarga con `kget <url> --via-daemon`.
  Los clientes hablan JSON-RPC 2.0 delimitado por líneas sobre un socket Unix (TCP de loopback con token en Windows).
  Se configura en la nueva sección `daemon` de `config.json`.
- **Servidor MCP para agentes de IA (`kget mcp`):** Claude Code y otros clientes MCP pueden descargar, consultar el
  progreso, pausar, reanudar, cancelar, buscar en el historial, verificar checksums y leer la configuración
  (9 herramientas). Regístralo con `claude mcp add kget -- kget mcp`. Las escrituras quedan limitadas a `--root`
  (por defecto, el directorio de descargas del daemon). Usa el daemon si hay uno en marcha (`--no-daemon`,
  `--start-daemon`).
- **Interfaz de terminal (`kget tui`):** vistas de cola, detalle e historial a pantalla completa. Añade URLs,
  pausa/reanuda, cancela, reintenta, elimina, abre archivos y busca o filtra el historial con el teclado. Usa el
  daemon si hay uno en marcha (`--daemon` inicia uno, `--local` ejecuta las descargas en el propio proceso).
- **Extensión de navegador (`browser-extension/`):** extensión Manifest V3 para Chrome, Chromium, Edge, Brave y
  Firefox. Añade el menú contextual "Download with KGet", un popup con progreso en vivo, interceptación opcional de
  las descargas del navegador y envío opcional de cookies. `kget native-host install|uninstall [--browser …]
  [--extension-id …]` registra el puente de native messaging con el daemon.
- **Reglas inteligentes, etiquetas y duplicados:** las reglas de `config.json` ordenan descargas en carpetas
  (`{category}`, `{domain}`, `{ext}`, `{protocol}`, `{date:%Y-%m}`), añaden etiquetas y fijan conexiones, límite de
  velocidad, extracción o checksum obligatorio. Las reglas pueden coincidir por dominio, regex, glob, extensión,
  categoría, tipo MIME, tamaño o protocolo, y gana la primera que coincida. Hay carpetas por categoría integradas y
  opcionales. Gestiona las reglas con `--rules list|test|add|remove|enable|disable` y aplícalas con
  `--organize`/`--no-organize`. `--rules suggest` propone reglas a partir del historial; con `--ai`, consulta a
  Claude y envía solo recuentos agregados, tras tu confirmación.
  `--tag` guarda etiquetas en el historial. `--duplicates skip|ask|download|link` decide qué pasa cuando una URL ya
  se descargó. `--dedupe` sustituye el contenido idéntico por un enlace duro.
- **Contraseñas guardadas en el llavero del sistema:** Llavero de macOS, Administrador de credenciales de Windows o
  Secret Service en Linux/BSD (feature `keychain`, activa por defecto, Rust puro). Las contraseñas de FTP, SFTP
  (contraseña y frase de la clave), WebDAV y proxy se buscan cuando la URL tiene usuario pero no contraseña. KGet
  pregunta en el terminal y ofrece guardar la contraseña. `--save-credentials` la guarda sin preguntar. Gestiona las
  contraseñas con `--credentials list|status|add|delete` (también el comando `credentials` del REPL).
  `KGET_KEYCHAIN=off` lo desactiva.
- **Búsqueda en el historial:** `--history` filtra por `--status`, `--domain`, `--type`, `--protocol`,
  `--since`/`--until` (`7d`, `12h`, `today`, fechas), `--search` y `--tag`. Ordena con
  `--sort newest|oldest|largest|smallest|name|fastest`, pagina con `--history-limit`/`--history-offset` e imprime
  JSON con `--json` (JSON Lines con `--jsonl`). El comando `history` del REPL acepta los mismos filtros. Las entradas
  ahora registran protocolo, duración, velocidad media, resultado del checksum y etiquetas.
- **Selección de archivos y seeding de torrents:** `--list-files` muestra el contenido numerado de un enlace magnet
  o archivo `.torrent`. `--select 1,3-5` y `--select-glob "*.mkv"` descargan solo una parte. `--seed-ratio 1.5` y
  `--seed-time 2h` siguen compartiendo después (valores por defecto: `torrent.seed_ratio` / `torrent.seed_time_secs`).
  Las estadísticas del swarm (peers, seeders, velocidades, ratio, trackers) llegan como eventos `torrent_stats` y se
  muestran en la TUI, la GUI egui y la app de macOS. La selección, el listado de archivos y el tiempo de seeding
  requieren el cliente nativo (`torrent-native`); Transmission admite la selección y el ratio de seeding.
- **HTTP/2, HTTP/3 y mirrors en paralelo:** `--http-version auto|1.1|2|3` (o `optimization.http_version`) para las
  descargas turbo. `auto` negocia HTTP/2 mediante ALPN. HTTP/3 es experimental y requiere compilar con la feature
  `http3` y `RUSTFLAGS="--cfg reqwest_unstable"`. Las descargas Metalink piden fragmentos a todos los mirrors a la
  vez y descartan los que fallan.
- **Eventos JSONL** (sigue siendo `"v":1`): `phase` (`verifying` con `algorithm`, o `extracting`), `rule`,
  `duplicate` y `torrent_stats`. Las entradas de `files` incluyen `selected` cuando hay una selección.
- **Biblioteca:** `jobs::JobManager` (cola con concurrencia limitada, pausar/reanudar, `JobEvent`s por suscriptor),
  `runner::{DownloadJob, RunContext, Reporter, run_job}`, `queue::{HistoryQuery, HistoryStore, HistorySort}`,
  `credentials`, `hf`, `ollama`, `fetch`, `rules`, `dedupe`, `daemon::client::DaemonClient`, `http::HttpVersion`,
  `AdvancedDownloader::{set_mirrors, set_http_version}`, los métodos del builder `.include()`, `.exclude()`,
  `.revision()`, `.hf_cache()` y `.parallel_files()`, y `config::set_config_dir_override`. `JsonEvent`/`JobEvent`
  también implementan `Deserialize`.
- **App de macOS:** lee el historial con `kget --history --json`, muestra el estado "extrayendo" y el algoritmo del
  checksum a partir de los eventos `phase`, y muestra un resumen del swarm del torrent.
- **El modo interactivo (`kget -i`) ahora cubre la CLI:** todas las flags de descarga (`--checksum`, `--checksum-url`,
  `--extract`, `-c`, `--no-clobber`, `-H`, `--retries`, `--timeout`, `--at`, `-j`, protocolos forzados) y
  `-l`/`-p` por descarga, sin guardarse. Nuevos comandos: `batch`, `info`, `retry`, `open`, `reveal`,
  `checksum`, `verify`, `extract`, `doctor`, `cd`/`pwd`, `set` (valores de la sesión), `history N|failed|completed|search|clear`,
  `config path|reset`, `version`, `help <tema>`. Autocompletado con Tab, historial guardado entre sesiones,
  varias URLs por línea, y Ctrl+C detiene la descarga en curso (conservando el `.part`) en lugar de ignorarse.

### Cambiado
- **Nuevo motor turbo:** las peticiones de rango ahora se ejecutan como tareas tokio con reqwest asíncrono, en lugar
  de un pool rayon de clientes bloqueantes. Una conexión ociosa divide el rango que terminaría último (work stealing,
  hasta 512 KiB), y HTTP/2 multiplexa los rangos sobre pocas conexiones. El formato de reanudación `.kget` no cambia.
- **El historial se guarda en SQLite** (`history.db`, WAL, seguro con varios procesos) en lugar de `history.json`, y
  conserva las últimas 10.000 entradas. `--history` ganó la columna Size.
- Un único camino de descarga (`kget::runner`) para la CLI, el modo batch, el REPL, la GUI egui, el daemon y MCP.
  Las credenciales guardadas, los eventos de fase y los detalles del historial (duración, velocidad) funcionan igual
  en todos.
- `--save-config` guarda la contraseña del proxy en el llavero del sistema. Sigue sin escribirse nunca en `config.json`.
- En un terminal, KGet pregunta antes de volver a descargar una URL cuyo archivo anterior sigue existiendo (elige con
  `--duplicates`). Las ejecuciones no interactivas vuelven a descargar, como antes.
- SIGTERM y SIGHUP detienen limpiamente las descargas de la CLI, como Ctrl+C: los datos parciales se conservan para
  reanudar.
- `tokio` es ahora una dependencia central. La feature `async` solo controla los helpers asíncronos del builder.
- Las features por defecto son ahora `keychain` y `tui`.

### Corregido
- Reanudar una descarga simple después de que cambiara el archivo remoto podía mezclar bytes antiguos y nuevos.
  KGet ahora guarda los validadores en `<archivo>.kget`, envía `If-Range` y reinicia cuando el archivo cambió.
- Una descarga simple que reanuda un `.part` del modo turbo ahora solo confía en el prefijo completo.
- Los errores de escritura (disco lleno, cuota, archivo demasiado grande) se reintentaban con un offset
  desincronizado. Ahora la descarga falla de inmediato y conserva el archivo parcial para `-c`.
- Un estado `.kget` corrupto o inconsistente (fragmentos solapados o desordenados) se rechaza en lugar de usarse.
- Cuando un servidor responde al rango de un fragmento con `200` o `416`, KGet ahora prueba otro mirror o reinicia
  la descarga como flujo único en lugar de fallar.
- Reanudar (`-c`) en un servidor que ignora `Range` y reenvía el archivo completo hacía fallar los builds de debug
  y rompía el límite de velocidad en los builds de release.
- `sanitize_filename` podía dejar puntos o espacios al final tras truncar (`. . a`).
- El parser de archivos de checksum entraba en pánico con líneas como `) (` y aceptaba hashes no hexadecimales.
- Una descarga fallida o cancelada guardada con `-O archivo` queda registrada con ese nombre, para poder reanudarse.

### Seguridad
- `ProxyConfig` nunca serializa la contraseña del proxy y la oculta en la salida de `Debug`. Las contraseñas nunca
  aparecen en `config.json`, el historial, los logs ni los eventos JSONL.
- Daemon: el socket Unix tiene modo `0600` en un directorio `0700`. El modo TCP solo acepta loopback, con un token de
  256 bits en un archivo accesible solo para el dueño. Las peticiones tienen un límite de 1 MiB, y se validan
  cabeceras y URLs. Las salidas quedan dentro del directorio de descargas (sin `..`, escapes por symlink ni nombres
  anidados) salvo que `daemon.allow_outside_download_dir` esté activo. Los secretos se ocultan en las respuestas y en
  `daemon.log`.
- MCP: toda escritura, y todo archivo leído por `verify_checksum`, queda limitado a `--root`.
- Host nativo: comprueba la extensión que llama contra `allowed_origins.json`, reenvía solo nombres de archivo y
  elimina `Cookie`/`Authorization`/`Proxy-Authorization` salvo que se activen las cookies. Solo acepta enlaces
  http(s), ftp(s) y magnet, y `open` rechaza ejecutables.
- Los tokens de Hugging Face nunca se registran y se descartan en redirecciones a otro host.

### Desarrollo
- Workspace de Cargo con `crates/kget-ffi`: bindings UniFFI 0.32 (Swift, Kotlin) sobre `JobManager` (`KgetEngine`,
  listener de eventos). `scripts/gen-bindings.sh`, `scripts/build-xcframework.sh` y `scripts/swift-smoke-test.sh`
  los generan y prueban. `default-members = ["."]` mantiene los comandos `cargo` simples en el paquete `Kget`.
- Nuevas suites de pruebas: `job_manager_tests`, `history_tests`, `robustness_tests`, `fuzz_regressions`,
  `model_tests`, `model_env_tests`, `smart_rules_tests`, `daemon_tests`, `mcp_tests`, `native_host_tests`,
  `bench_turbo` (benchmark ignorado por defecto), pruebas del engine de `kget-ffi` y la suite `node --test` de la
  extensión.
- `fuzz/`: crate cargo-fuzz con 12 objetivos sobre `kget::fuzz_api`. La CI ganó un job rápido de fuzzing y un job FFI
  en macOS (bindings y un smoke test en Swift).
- Variables para pruebas: `KGET_HF_ENDPOINT`, `KGET_OLLAMA_REGISTRY`, `KGET_KEYCHAIN=mock`, `KGET_RUNTIME_DIR`,
  `KGET_DAEMON_TRANSPORT=tcp`, `KGET_NATIVE_HOST_DIR`, `KGET_ANTHROPIC_BASE_URL`.

### Notas de actualización
- **Historial:** la primera ejecución importa `history.json` a `history.db` y conserva el archivo antiguo como
  `history.json.bak`. Una base de datos ilegible se mueve a `history.db.corrupt`. Los binarios `kget` antiguos siguen
  leyendo `history.json` y no ven las entradas nuevas. La app de macOS vuelve a `history.json` con un binario antiguo.
- **Configuración:** nuevos campos opcionales, todos con valor por defecto, así que los archivos existentes cargan
  sin cambios: `rules_enabled` (`false`), `builtin_rules` (`true`), `organize_dir`, `rules`,
  `daemon.{download_dir, allow_outside_download_dir, max_concurrent}`, `optimization.http_version` y
  `torrent.{seed_ratio, seed_time_secs}`.
- **Biblioteca (puede romper):** `http::ClientOptions` tiene el nuevo campo público `http_version`, así que añade
  `..Default::default()` a los literales. Los `match` exhaustivos necesitan nuevos brazos:
  `Protocol::{HuggingFace, Ollama}`, `JsonEvent::{Phase, Rule, Duplicate, TorrentStats}` y
  `WorkerToGuiMessage::TorrentStats`. `FileEntry` tiene el nuevo campo `selected`. `app::JobId` es ahora un
  re-export de `jobs::JobId` (sigue siendo `u64`).
- **Features:** `keychain` y `tui` vienen activas por defecto (`--no-default-features` compila sin ellas). `http3`
  es experimental y requiere `RUSTFLAGS="--cfg reqwest_unstable"`. `async` ahora solo activa `download_async()` /
  `download_all_async()`.
- **Consumidores de JSONL:** el protocolo sigue siendo `"v":1`. Ignora los tipos de evento y campos que no conozcas.

## [1.7.1] - 2026-10-18

### Corregido
- **El modo turbo podía generar archivos corruptos en silencio:** reanudar una descarga paralela interrumpida
  trataba el archivo preasignado como completo y los fragmentos cortados por el servidor se aceptaban como completos.
  Ahora las descargas usan `<archivo>.part` + un estado `<archivo>.kget` validado contra el servidor; cada fragmento
  debe estar completo y los reintentos continúan desde el byte exacto.
- **Las flags de la CLI se guardaban para siempre en `config.json`.** Ahora solo valen para la ejecución actual;
  usa `--save-config` para guardarlas (la contraseña del proxy nunca se guarda).
- **El proxy HTTP no cubría descargas HTTPS y SOCKS5 no funcionaba;** los proxies inválidos se ignoraban.
- **La app de macOS no encontraba yt-dlp instalado con Homebrew** al abrirse desde el Dock/Finder, y marcaba los vídeos descargados como fallidos.
- yt-dlp podía bloquearse con muchos avisos; ahora se muestra su mensaje de error.
- `--sha256` se ignoraba en modo silencioso/biblioteca y cuando el servidor no admitía Range.
- La descarga simple ya no deja un archivo truncado con el nombre final; HTTP 429/5xx respetan `Retry-After`.
- Las respuestas brotli se descomprimían, alterando el archivo guardado.
- `cargo test`, el ejemplo de la biblioteca y 10 doctests no compilaban; los tests ya no modifican la configuración real.
- El historial podía borrarse o perder entradas; las credenciales de las URLs ya no se guardan.
- Un `config.json` antiguo o parcial ya no impide iniciar KGet.
- FTP/SFTP/WebDAV/Metalink/torrent se detectan por el esquema; FTP y SFTP tienen timeouts y reanudación.
- Se leen manifiestos Metalink 3.x y se verifican SHA-512/MD5.
- Modo batch con concurrencia limitada (`-j`) y nombres únicos.
- GUI egui: las descargas simultáneas ya no comparten progreso ni cancelación.
- macOS: la Share Extension ya no corta URLs con `&`; se acepta `webdav://`; se encuentra el `kget` de Homebrew en Apple Silicon.

### Añadido
- `-c/--continue`, `--no-clobber`, `--retries`, `--timeout`, `-j/--jobs`, `--save-config`, `--doctor`, `--checksum`, `--checksum-url`.
- `KGET_YTDLP_PATH` y campo "yt-dlp path" en la app de macOS; Pausar/Reanudar en la app.
- Protocolo JSONL versionado (`"v":1`) para todos los protocolos.
- Archivos `.torrent` (locales y URLs) con el cliente nativo.
- CI en GitHub Actions y workflow de release (binarios, crates.io, Homebrew).

### Eliminado / Obsoleto
- Código de caché/compresión sin uso (obsoleto); `--no-cache` oculto; dependencias sin uso eliminadas.

## [1.7.0] - 2026-05-24

### Añadido
- **Descarga en lote (`--batch urls.txt`):** descarga múltiples archivos en paralelo desde un archivo de texto plano — una URL por línea, las líneas que empiezan con `#` se ignoran. Todas las URLs se ejecutan concurrentemente via hilos del SO. Estado `[OK]`/`[FAIL]` por URL; resumen al final.
- **Pestaña de historial en la app macOS:** nuevo elemento "History" en la sidebar lee el `history.json` persistente generado por la CLI Rust. Muestra todas las descargas con fecha, tamaño y badge de estado. Pasa el cursor sobre una fila para revelar el botón de re-descarga.
- **Drag-and-drop de URLs en la ventana macOS:** arrastra cualquier enlace HTTP/HTTPS/FTP/magnet desde Safari, Chrome o cualquier app y suéltalo en la ventana de KGet. Un overlay azul translúcido aparece durante el hover; al soltar, la URL aterriza en la barra de entrada lista para iniciar.
- **Monitor de portapapeles en la app macOS:** la app vigila el portapapeles cada 1,5 s. Cuando se detecta una nueva URL HTTP, HTTPS, FTP, SFTP o magnet, aparece un banner descartable con un botón "Download" de un clic. El banner se suprime si la URL ya está en la lista actual.
- **Headers HTTP personalizados (`-H "Nombre: Valor"`):** pasa una o más flags `-H` para inyectar headers arbitrarios en descargas simples y turbo. Se pueden apilar múltiples headers. Funciona en los modos URL única, lote e interactivo.
- **Sparkline de velocidad en la app macOS:** cada fila de descarga activa muestra un gráfico de velocidad en tiempo real de 44×16pt que acumula las últimas 30 lecturas. Construido con SwiftUI `Path` + gradiente mediante el nuevo componente `SparklineView`.
- **Auto-extracción de archivos:** `kget --extract` ejecuta automáticamente `unzip`, `tar` o `7z` en el archivo descargado cuando la extensión es `.zip`, `.tar.gz`, `.tgz`, `.tar.bz2`, `.tar.xz` o `.7z`. Controlable mediante la nueva opción "Auto-extraer archivos" en Ajustes macOS → Descargas.
- **Programación de descarga (`--at "HH:MM"`):** pospone una descarga CLI a una hora local específica. El proceso duerme hasta que se alcanza el minuto objetivo, luego inicia la descarga.
- **Integración yt-dlp (`--ytdlp`, auto-detectado):** si `yt-dlp` (o `youtube-dl`) está instalado, las URLs de YouTube, Vimeo, Twitch, TikTok, Instagram, Bilibili y 10+ otras plataformas se enrutan automáticamente por él. Calidad seleccionable via `--quality best|1080p|720p|480p|360p|audio`.
- **Soporte WebDAV (`webdav://`, `webdavs://`):** nuevo adaptador `WebDavDownloader` en `src/webdav/mod.rs` convierte `webdav://` → `http://` y `webdavs://` → `https://`, extrae credenciales incrustadas e inyecta header `Authorization: Basic`. Detectado automáticamente por el scheme de la URL; flag explícita `--webdav` también disponible.
- **Share Extension (`Compartir > KGet`):** la Share Extension de macOS ya está completa. `ShareViewController` codifica la URL compartida como `kget://download?url=<encoded>` y abre la app principal. Compilada e incrustada en `KGet.app/Contents/PlugIns/KGetShareExtension.appex` por `build-native-macos.sh`.
- **Revisión de la API pública de la biblioteca (`src/builder.rs`, `src/error.rs`, `src/events.rs`, `src/checksum.rs`):**
  - **Patrón Builder** — `kget::builder(url)` y `kget::batch([...])` reemplazan los argumentos posicionales. Métodos fluidos: `.output()`, `.connections()`, `.speed_limit()`, `.proxy()`, `.sha256/sha512/sha1/md5/blake3()`, `.verify_from()`, `.header()`, `.retry()`, `.range()`, `.quiet()`.
  - **Errores tipados** — enum `KgetError` (`Network`, `Io`, `ChecksumMismatch`, `Protocol`, `Cancelled`, `NotFound`, `SidecarError`, `Other`) con impls `From` para `reqwest::Error`, `std::io::Error` y `Box<dyn Error>`.
  - **Canal de eventos** — `.spawn()` retorna `(JoinHandle, Receiver<DownloadEvent>)` con variantes `Progress { percent, speed_bps, eta_secs }`, `Status`, `Completed`, `Error`.
  - **Métricas de descarga** — struct `DownloadResult` con `path`, `bytes_downloaded`, `avg_speed_bps`, `duration`, `connections_used`, `checksums`.
  - **Descarga en memoria** — `.download_to_bytes() -> Vec<u8>` y `.download_to_reader() -> impl Read`.
  - **Checksums multi-algoritmo** — SHA-256, SHA-512, SHA-1 (crate `sha1`), MD5 (crate `md-5`), BLAKE3 (crate `blake3`). Enum `ChecksumAlgorithm` + `compute_checksum()` en `src/checksum.rs`.
  - **Reintento configurable** — `RetryConfig { max_attempts, backoff: Backoff::Exponential { base_ms, max_ms }, retry_on_status }`.
  - **Lote con control de concurrencia** — `BatchBuilder::concurrency(n)` usa pool de hilos Rayon; retorna `Vec<BatchResult>`.
  - **API Async** — `DownloadBuilder::download_async()` y `BatchBuilder::download_all_async()` con `--features async`. Ambos usan `tokio::task::spawn_blocking`.

### Corregido
- **`AdvancedDownloader::new()` entraba en pánico si fallaba el build del cliente HTTP** — tipo de retorno cambiado de `Self` a `Result<Self, …>` para que el error se propague en lugar de crashear.
- **El throttle paralelo era por hilo** — con N conexiones y límite de 1 MB/s, el throughput real era N×1 MB/s. Reemplazado por `Arc<Mutex<TokenBucket>>` global compartido entre todos los workers rayon; la tasa agregada ahora está correctamente limitada.
- **`file.set_len(total_size)` ocurría antes de confirmar soporte de ranges** — si el servidor devolvía 200 en lugar de 206, el archivo era preasignado y luego sobreescrito por una descarga completa, produciendo resultado corrupto. La preasignación ahora solo ocurre cuando `supports_range` está confirmado.
- **El prompt de integridad ISO leía de stdin en contexto de biblioteca/automatización** — `AdvancedDownloader` ahora tiene campo `ResumePolicy` (`Ask` / `AlwaysResume` / `AlwaysRestart`). Los llamadores de la biblioteca establecen `AlwaysResume` para evitar bloqueos.
- **Tipo MIME ISO incorrecto** — `"application/x-iso9001"` (norma ISO 9001) corregido a `"application/x-iso9660-image"`.
- **`verify_file_sha256` imprimía en stdout incondicionalmente** — todas las llamadas `println!` eliminadas; los mensajes ahora se envían solo via callback opcional.
- **El descargador simple reintentaba en 404/403/410** — los errores 4xx permanentes ahora fallan inmediatamente; solo las respuestas 5xx transitorias y los errores de conexión se reintentan.
- **`validate_filename` era insuficiente** — ahora también rechaza: bytes nulos (`\0`), secuencias de path traversal (`..`), nombres de archivo de más de 255 bytes y nombres de dispositivos reservados de Windows (CON, PRN, AUX, NUL, COM1–COM9, LPT1–LPT9) — case-insensitive, con o sin extensión.
- **`sftp/mod.rs`: `CheckResult::Failure` continuaba silenciosamente** — el caso de error interno de libssh2 ahora devuelve un error grave y aborta la conexión en lugar de eludir la verificación de host-key.

### Cambiado
- **App macOS SwiftUI — rediseño completo:** layout `NavigationSplitView` con sidebar colapsable para navegación por filtros (Todos / Activos / Completados / Con Fallo con badges de recuento en tiempo real); barra de entrada URL limpia con toggle Turbo inline; barra de progreso fina de 3px con animación shimmer reemplazando la barra multi-segmento anterior; filas de descarga con status dot, type badges (Turbo / ISO / Torrent) e iconos de acción compactos.
- **GUI egui (Linux/Windows) — rediseño completo:** paleta de colores inspirada en Apple iOS adaptativa al sistema; sidebar izquierda (180px) con navegación Library y badges de recuento por categoría; botón de toggle oscuro/claro; barra de progreso fina de 3px con shimmer; tarjetas de descarga con status dot, type badges, botones de acción limpios; sombras en las tarjetas; barra de estado con estadísticas en tiempo real.
- **El tema egui ahora es adaptativo al sistema:** lee la preferencia de oscuro/claro del SO al inicio; botón de anulación manual en la sidebar.

## [1.6.3] - 2026-05-21

### Añadido
- **Soporte Metalink (`.meta4` / `.metalink`):** `kget --metalink archivo.meta4` analiza el manifiesto RFC 5854, prueba mirrors en orden de prioridad y verifica SHA-256 tras la descarga. Funciona en la CLI (`--metalink`) y en el modo interactivo (`download --metalink`). Auto-detectado por la extensión del archivo.
- **Historial persistente de descargas:** cada descarga en CLI y modo interactivo queda registrada en `history.json` en el directorio de configuración del SO. Consulte con `kget --history`; limpie con `kget --history-clear` (o `--history-clear completed`). El modo interactivo gana los comandos `history`, `history clear` e `history clear completed`.

## [1.6.3] - 2026-05-10

### Añadido
- **Eventos JSONL experimentales en la CLI:** `--jsonl` emite eventos `started`, `progress`, `status`, `completed` y `error` para agentes, scripts y futuros frontends.
- **Filtros en la GUI:** la app macOS y la GUI Rust ahora filtran descargas por todas, activas, completadas y fallidas/canceladas.
- **Más acciones de descarga:** macOS y la GUI Rust exponen Copiar URL, Abrir Archivo, Abrir Carpeta y Copiar SHA256 cuando hay checksum.
- **SHA256 esperado en la GUI Rust:** la GUI Linux/Windows puede enviar un hash SHA256 esperado al worker de descarga.

### Cambiado
- **Las preferencias de macOS ahora afectan el comportamiento real:** modo avanzado por defecto, notificaciones y límite de velocidad se guardan y se aplican a nuevas descargas.
- **Versión de macOS corregida:** los metadatos de la app/extensión usan 1.6.3 y las etiquetas visibles leen la versión del bundle en vez de strings hardcoded.
- **Los límites de velocidad ahora regulan descargas HTTP:** las descargas HTTP simples y avanzadas respetan el límite configurado en bytes/s.

### Corregido
- **Fallback de metadatos en descarga avanzada:** cuando `HEAD` falla o no devuelve `Content-Length`, KGet prueba `Range: bytes=0-0` antes de rendirse.
- **Progreso correcto al reanudar:** las descargas avanzadas inicializan el progreso con el tamaño parcial existente en vez de reiniciar visualmente desde cero.
- **El modo JSONL ya no mezcla líneas humanas de progreso avanzado en la salida de máquina.**

## [1.6.2] - 2026-04-28

### Corregido
- **Las descargas SFTP eran completamente no funcionales.** La implementación anterior pasaba la cadena completa `sftp://…` directamente a `TcpStream::connect` y la usaba como ruta remota, provocando un fallo inmediato en cada llamada SFTP. El módulo fue reescrito por completo:
  - La URL se parsea correctamente para extraer `host`, `puerto`, `usuario` y `ruta remota`.
  - Autenticación en orden de prioridad: contraseña en la URL → agente SSH activo → archivos de clave predeterminados (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`, `~/.ssh/id_ecdsa`).
  - Archivo transmitido en chunks de 32 KB con barra de progreso en tiempo real.
  - Mensajes de error claros y accionables en cada punto de fallo.
- **El login anónimo FTP fallaba cuando la URL no contenía usuario.** `url.username()` de la crate `url` devuelve una cadena vacía `""` (no `None`) cuando la URL no tiene segmento de usuario. Pasar `""` a `ftp.login()` hacía que los servidores FTP anónimos rechazaran la conexión. El downloader ahora usa `"anonymous"` como fallback.

### Añadido
- **Modo interactivo completamente implementado.** Anteriormente `kget --interactive` abría un REPL que solo imprimía `"Would download: …"` sin realizar ninguna descarga. El modo ahora está completo:
  - Banner de entrada con arte ASCII en fuente de bloques Unicode.
  - Editor de línea `rustyline` con historial de comandos persistente.
  - `download [flags] <url>` — activa el downloader correcto según los flags:
    - Por defecto: HTTP/HTTPS simple con retry y barra de progreso.
    - `-a` / `--advanced` / `--turbo`: `AdvancedDownloader` (paralelo con byte range, reanudable).
    - `--ftp`: downloader FTP con fallback anónimo.
    - `--sftp`: downloader SFTP con autenticación SSH multi-método.
    - `--torrent` o prefijo `magnet:?` detectado automáticamente: motor de torrent nativo.
    - Flags `-o <ruta>`, `-q` (silencioso), `--sha256 <hash>` soportados.
  - `config [show | set <clave> <valor>]`: lee y persiste configuraciones (`connections`, `speed-limit`, `compression`, `cache`).
  - Comandos `clear`, `version`, `help` / `?`; `get` y `dl` como alias de `download`.
  - Los errores se imprimen y el REPL continúa — un error de descarga nunca cierra la sesión.

### Cambiado
- **Manejo de errores en locks Mutex en `AdvancedDownloader`:** todas las llamadas `.unwrap()` en `Mutex::lock()` reemplazadas por `.expect("…")` con mensajes descriptivos.
- **Limpieza de la API pública de `Optimizer`:** eliminados atributos `#[allow(dead_code)]` de los métodos públicos `compress`, `get_cached_file` y `cache_file`.

## [1.6.1] - 2026-04-27

### Añadido
- La app macOS ahora valida enlaces magnet antes de crear la tarjeta de descarga.
- Las descargas completadas incluyen acciones Abrir Archivo y Abrir Carpeta.
- Menú contextual en las tarjetas de macOS: Copiar URL, Abrir Carpeta, Reiniciar y Eliminar.
- Atajos en la app macOS: `Cmd+V`, `Cmd+L`, `Esc` y `Delete`.
- Verificación SHA256 esperada por CLI `--sha256 <hash>` y por biblioteca con `DownloadOptions::expected_sha256`.
- Helper público `verify_file_sha256` para usuarios de la biblioteca.
- Notificaciones nativas de finalización y error en la GUI Rust para Linux y Windows mediante `notify-rust`.

### Cambiado
- URLs o magnets duplicados ahora enfocan la tarjeta existente en macOS en vez de crear otra.
- Las descargas avanzadas respetan el límite de conexiones del optimizador y rechazan respuestas inválidas de byte range.
- Documentación de biblioteca actualizada en inglés, portugués y español para la API actual.

### Corregido
- Los enlaces magnet inválidos se rechazan antes de activar el backend torrent.
- Un mismatch de SHA256 ahora hace fallar la descarga en vez de solo imprimir el hash calculado.

## [1.6.0] - 2026-02-28

### Añadido
- **App Nativo macOS (SwiftUI):** Aplicación macOS nativa completamente rediseñada con integración profunda al sistema.
  - Manejadores de esquema de URL (`kget://`, `magnet:`)
  - Asociaciones de archivo (`.torrent`)
  - Integración con barra de menú con acciones rápidas
  - Soporte para el menú de Servicios de macOS
  - Notificaciones nativas
  - Instalador DMG arrastrar-y-soltar con guía visual (cajas, flecha, texto de instrucción)
- **GUI Multiplataforma Mejorada:** Gran renovación visual para la GUI basada en egui (Linux/Windows).
  - Lista de descargas con seguimiento de múltiples descargas simultáneas
  - Badge TURBO para modo de descargas paralelas
  - Badge ISO para archivos ISO con verificación automática de integridad
  - Barra de progreso multi-segmento mostrando conexiones paralelas (C1, C2, C3, C4)
  - Barra de progreso de verificación con tema púrpura y animación de escudo
  - Indicador de conexiones (⚡ 4x) para modo turbo
  - Visualización de velocidad y ETA en tiempo real
  - Estado vacío con iconos de protocolo
  - Entrada de URL en línea única con controles integrados
  - Diseño compacto con nombres de archivos y URLs truncados
  - Dimensionamiento y alineación adecuados de botones
- **Mejoras Visuales:**
  - Tema oscuro mejorado con mejor contraste
  - Efectos de brillo animados en las barras de progreso
  - Badges e iconos coloreados por estado
  - Tipografía y espaciado mejorados
  - Fondo del instalador DMG con tema oscuro, cajas redondeadas, flecha chevron y texto de instrucción

### Cambiado
- **Script de Build:** Ahora cierra automáticamente las instancias de KGet en ejecución antes de compilar
- **Script de Build:** Compila el bundle de la app en `/tmp` para evitar que los atributos extendidos de iCloud interfieran con la firma de código
- **Seguimiento de Progreso:** Eliminado el límite artificial del 99%, ahora muestra progreso preciso de 0-100%
- **Verificación SHA256:** Usa CommonCrypto nativo en macOS con progreso en tiempo real
- **Progreso de Descarga Avanzada:** Ahora usa reporte de progreso vía stdout en lugar de monitoreo de tamaño de archivo

### Corregido
- Barra de progreso atascada en 90% en modo de descarga avanzada
- Barra de progreso "temblando" (saltos erráticos) durante descargas avanzadas debido a conflicto entre monitoreo de tamaño de archivo y progreso vía stdout
- Progreso de verificación sin mostrar feedback hasta la finalización
- Firma de código fallando en macOS debido a que iCloud añade atributos extendidos (`com.apple.FinderInfo`, `com.apple.provenance`)
- Iconos del instalador DMG desalineados con las cajas de fondo

## [1.5.2] - 2025-12-19

### Añadido
- **Manejo Inteligente de ISO**: Detección automática de archivos `.iso` mediante URL y tipo MIME.
- **Prevención de Corrupción**: Los archivos ISO ahora omiten las capas de descompresión/optimización para garantizar la integridad binaria 1:1.
- **Verificación de Integridad**: Se agregó una verificación opcional de suma de comprobación SHA256 al final de las descargas de ISO.

### Corregido
- **Optimización de Memoria y Disco**: Se refactorizó `AdvancedDownloader` para usar escritas en stream con `BufWriter`, reduciendo drásticamente el uso de RAM y evitando problemas de 100% de tiempo activo del disco.
- **Confirmación de Verificación**: Se corrigió un error por el cual la verificación de integridad se ejecutaba automáticamente en modo avanzado; ahora el programa solicita confirmación al usuario correctamente.
- **UI/UX**: Se limpió la salida de la terminal durante las descargas paralelas para una experiencia de barra de progreso más fluida.
- Se corrigió el error del compilador Rust `E0382` con respecto a la propiedad (ownership) del tipo `Mime` en `download.rs`.
- Se mejoró la seguridad de escritura de chunks paralelos para archivos binarios pesados.

## [1.5.1] - 2025-12-18

### Añadido
- Feature opcional `gui` en Cargo para que las dependencias de la interfaz gráfica sean opcionales; compile con `--features gui` para habilitar el soporte de GUI.
- Funciones de conveniencia de alto nivel: `kget::download(...)` y `kget::advanced_download(...)` para facilitar el uso como biblioteca.
- `create_progress_bar_factory(...)` exportado para permitir que los consumidores creen barras de progreso `indicatif`.
- Ejemplo `examples/lib_usage.rs` demostrando el uso de la biblioteca.
- Instrucciones de desarrollo Docker e integración `docker-compose` para simplificar la compilación, pruebas y contribuciones.

### Cambiado
- Actualizado README y `LIB.md` con instrucciones de uso de la biblioteca y ejemplos.
- `CONTRIBUTING.md` y traducciones actualizadas con el flujo de trabajo para colaboradores a través de Docker.
- División del código GUI: se agregó el módulo `gui_types` para que las compilaciones CLI funcionen sin la feature de GUI.

### Corregido / Varios
- Pequeñas correcciones en la documentación y actualizaciones de traducción (PT-BR/ES).

## [1.5.0] - 2025-05-26

### Añadido
- Nuevo crate público de Rust: KGet ahora puede ser usado como una biblioteca en tus propios proyectos Rust, haz clic [aquí](LIB.es.md) para saber más.
- Interfaz gráfica mejorada: fuentes más grandes, mejor diseño y controles más intuitivos.
- Integración con el portapapeles para fácil pegado de URLs.
- Botones de descarga y cancelación ahora siempre visibles y funcionales en la interfaz gráfica.
- **Modo interactivo:** Ejecuta `kget --interactive` para una experiencia tipo REPL con comandos como `download <url> [output]`, `help` y `exit`.

### Cambiado
- Proyecto renombrado de "KelpsGet" a "KGet" para simplicidad y consistencia.
- Esquema de versionado actualizado de 0.1.4 a 1.5.0 para permitir actualizaciones menores más frecuentes y seguimiento de versiones más claro.
- Lista de características movida del README al CHANGELOG para mantenimiento más fácil y mantener el README conciso.

### Eliminado
- Sección de características redundantes o excesivamente detalladas del README (ahora ver el CHANGELOG para todas las características).

## [0.1.4] - 2025-05-22

### Añadido
- Interfaz Gráfica de Usuario (GUI) para descargas más fáciles.
- Soporte para descarga FTP.
- Soporte para descarga SFTP (autenticación por contraseña y clave).
- Soporte para descarga de torrent vía enlaces magnet (integración con el daemon Transmission).
- Instrucciones detalladas para configuración del daemon Transmission en el README.

### Cambiado
- Refinada determinación de la ruta de salida para alinear comportamiento con `wget`.
- Asegurado que `final_path` sea siempre absoluto para evitar errores "Archivo o directorio no encontrado" en CWD.
- Actualizado README en inglés, portugués y español para reflejar todas las nuevas características e instrucciones de configuración.

### Corregido
- Resuelto error "Archivo o directorio no encontrado" al descargar sin `-O` asegurando rutas absolutas.
- Corregido `validate_filename` para verificar solo el nombre base del archivo, no la ruta completa.
- Resueltos problemas potenciales con `map_err` en `main.rs` para descargas de torrent y HTTP.

## [0.1.3] - 2025-03-11

### Añadido
- Modo de descarga avanzado con chunks paralelos y capacidad de reanudación
- Soporte automático de compresión (gzip, brotli, lz4)
- Sistema de caché inteligente para descargas repetidas más rápidas
- Limitación de velocidad y control de conexión
- Soporte de documentación en múltiples idiomas

### Cambiado
- Mejorado manejo de errores y retroalimentación del usuario
- Mejorada barra de progreso con información más detallada
- Optimizado uso de memoria para descargas de archivos grandes
- Actualizado sistema de configuración de proxy

### Corregido
- Corregido problemas de autenticación de proxy
- Resuelto problemas de creación de directorio de caché
- Corregido manejo de nivel de compresión
- Corregido manejo de ruta de archivo en Windows

### Seguridad
- Añadido manejo seguro de conexión proxy
- Mejorada validación de URL
- Mejorado saneamiento de nombre de archivo
- Añadida verificación de espacio antes de las descargas

## [0.1.2] - 2025-03-10

### Añadido
- Soporte para proxy (HTTP, HTTPS, SOCKS5)
- Autenticación de proxy
- Nombrado personalizado de archivo de salida
- Detección de tipo MIME

### Cambiado
- Mejorado cálculo de velocidad de descarga
- Mejorada visualización de la barra de progreso
- Mejores mensajes de error
- Documentación actualizada

### Corregido
- Corregido problemas de timeout de conexión
- Resuelto problemas de permisos de archivo
- Corregido análisis de URL
- Corregida visualización de la barra de progreso en Windows

## [0.1.1] - 2025-03-09

### Añadido
- Modo silencioso para integración con scripts
- Barra de progreso básica
- Visualización del tamaño del archivo
- Seguimiento de velocidad de descarga

### Cambiado
- Mejorado manejo de errores
- Mejorada interfaz de línea de comandos
- Mejor manipulación de archivos
- Instrucciones de instalación actualizadas

### Corregido
- Corregido problemas de manipulación de ruta
- Resuelto problemas de permisos
- Corregida visualización de progreso
- Corregido comportamiento de sobrescritura de archivo

## [0.1.0] - 2025-03-08

### Añadido
- Lanzamiento inicial
- Funcionalidad básica de descarga de archivo
- Interfaz de línea de comandos
- Manejo básico de errores
- Soporte multiplataforma
