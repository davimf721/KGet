# Usando o KGet como Biblioteca Rust

KGet é um gerenciador de downloads e também um motor Rust reutilizável para
HTTP/HTTPS, FTP, SFTP, WebDAV, magnet links, callbacks de progresso, retomada
de downloads, proxy e verificação de checksum multi-algoritmo.

[English](../LIB.md) | [Português](LIB.pt-br.md) | [Español](LIB.es.md)

## Instalação

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
    kget::builder("https://example.com/arquivo.zip")
        .output("./downloads/")
        .connections(8)
        .sha256("abc123def456...")
        .download()?;
    Ok(())
}
```

## Métodos do Builder

`kget::builder(url)` retorna um `DownloadBuilder`. Todos os métodos são encadeáveis:

| Método | Descrição |
|--------|-----------|
| `.output(caminho)` | Salvar em arquivo ou diretório |
| `.connections(n)` | Conexões paralelas (modo turbo) |
| `.speed_limit(bps)` | Máximo de bytes/s (token bucket global) |
| `.proxy(url)` | URL do proxy HTTP ou SOCKS5 |
| `.quiet(bool)` | Suprimir saída de progresso |
| `.sha256(hash)` | Verificar SHA-256 após download |
| `.sha512(hash)` | Verificar SHA-512 após download |
| `.sha1(hash)` | Verificar SHA-1 após download |
| `.md5(hash)` | Verificar MD5 após download |
| `.blake3(hash)` | Verificar BLAKE3 após download |
| `.verify_from(url)` | Baixar e analisar arquivo de checksum sidecar GNU/BSD |
| `.header(nome, valor)` | Adicionar header HTTP |
| `.retry(config)` | Política de retry customizada (`RetryConfig`) |
| `.range(inicio, fim)` | Solicitar faixa de bytes específica |

Métodos terminais:

| Método | Retorna | Descrição |
|--------|---------|-----------|
| `.download()` | `Result<DownloadResult, KgetError>` | Download para disco |
| `.download_to_bytes()` | `Result<Vec<u8>, KgetError>` | Download em memória |
| `.download_to_reader()` | `Result<impl Read, KgetError>` | Reader de streaming |
| `.spawn()` | `Result<(JoinHandle, Receiver<DownloadEvent>), KgetError>` | Thread em background com canal de eventos |
| `.download_async()` | `impl Future<…>` | API async (feature `async`) |

## Downloads em Lote

```rust,no_run
let results = kget::batch([
    "https://mirror1.example.com/arquivo.iso",
    "https://mirror2.example.com/outro.tar.gz",
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
            println!("\nSalvo {} bytes em {path}", bytes);
        }
        DownloadEvent::Error(e) => eprintln!("Erro: {e}"),
        _ => {}
    }
}
handle.join().ok();
# Ok::<(), KgetError>(())
```

## Erros Tipados

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

Erros permanentes (`Cancelled`, `NotFound`, `ChecksumMismatch`) nunca são tentados novamente.

## Checksums

```rust,no_run
use kget::checksum::{compute_checksum, ChecksumAlgorithm};

let hash = compute_checksum(ChecksumAlgorithm::Blake3, std::path::Path::new("arquivo.bin"))?;
println!("BLAKE3: {hash}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Algoritmos suportados: `Sha256`, `Sha512`, `Sha1`, `Md5`, `Blake3`.

Verificação por arquivo sidecar:

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
    "https://example.com/imagem.iso".to_string(),
    "imagem.iso".to_string(),
    true,
    ProxyConfig::default(),
    Optimizer::new(),
)?;
dl.set_resume_policy(ResumePolicy::AlwaysResume); // nunca bloqueia no stdin
dl.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

| Variante | Comportamento |
|----------|--------------|
| `Ask` (padrão) | Solicita via stdin em sessões interativas |
| `AlwaysResume` | Ignora prompt, prossegue sem perguntar |
| `AlwaysRestart` | Ignora prompt, reinicia do zero |

Chamadores da biblioteca devem sempre definir `AlwaysResume` ou `AlwaysRestart` para evitar bloqueio.

## Download HTTP Avançado Paralelo

```rust,no_run
use kget::{AdvancedDownloader, Optimizer, ProxyConfig};

let mut downloader = AdvancedDownloader::new(
    "https://example.com/grande.iso".to_string(),
    "grande.iso".to_string(),
    false,
    ProxyConfig::default(),
    Optimizer::new(),
)?;  // new() retorna Result — propague com ?

downloader.set_resume_policy(kget::ResumePolicy::AlwaysResume);
downloader.set_progress_callback(|progress| {
    print!("\r{:.1}%", progress * 100.0);
});
downloader.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## Download HTTP Simples

```rust,no_run
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

let options = DownloadOptions {
    output_path: Some("arquivo.zip".to_string()),
    ..Default::default()
};

download(
    "https://example.com/arquivo.zip",
    ProxyConfig::default(),
    Optimizer::new(),
    options,
    None,
)?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## Download em Memória

```rust,no_run
let bytes: Vec<u8> = kget::builder("https://example.com/dados.json")
    .download_to_bytes()?;
println!("Recebidos {} bytes", bytes.len());
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
    "sftp://usuario:senha@servidor.example.com/caminho/arquivo.tar.gz".to_string(),
    "arquivo.tar.gz".to_string(),
    false,
    ProxyConfig::default(),
    Optimizer::new(),
);
dl.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

Prioridade de autenticação:
1. Senha na URL (`sftp://usuario:senha@host/caminho`)
2. SSH agent ativo
3. Arquivos de chave padrão (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`, `~/.ssh/id_ecdsa`)

Host keys verificadas contra `~/.ssh/known_hosts`; divergências geram erro.

## WebDAV

```rust,no_run
use kget::webdav::WebDavDownloader;
use kget::{Optimizer, ProxyConfig};

let dl = WebDavDownloader::new(
    "webdavs://usuario:senha@nas.local/backups/db.tar.gz".to_string(),
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

## Histórico de Downloads

```rust,no_run
use kget::queue::{DownloadHistory, EntryStatus, HistoryEntry};

let mut history = DownloadHistory::load();
let entry = HistoryEntry::new("https://example.com/arquivo.iso", "/home/user/Downloads", None);
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

Com `--features async`:

```rust,no_run
#[tokio::main]
async fn main() -> Result<(), kget::KgetError> {
    kget::builder("https://example.com/arquivo.zip")
        .output("./downloads/")
        .connections(4)
        .download_async()
        .await?;
    Ok(())
}
```

## Garantias da Biblioteca

- Chamadores da biblioteca nunca bloqueiam no `stdin` quando `ResumePolicy` está definida.
- Progresso e status são expostos via callbacks e canais de eventos.
- Arquivos são transmitidos para disco (exceto `.download_to_bytes()`).
- Nomes de arquivo são validados: rejeita bytes nulos, path traversal, >255 bytes e nomes reservados do Windows.
- Apenas erros 5xx e de conexão são tentados novamente; 4xx falha imediatamente.

Veja [examples/lib_usage.rs](../examples/lib_usage.rs) para mais exemplos.
