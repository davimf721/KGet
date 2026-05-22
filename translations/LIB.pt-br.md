# Usando o KGet como Biblioteca Rust

KGet é um gerenciador de downloads e também um motor Rust reutilizável para
HTTP/HTTPS, FTP, SFTP, links magnet, callbacks de progresso, retomada de
downloads, proxy e verificação SHA256.

[English](../LIB.md) | [Português](LIB.pt-br.md) | [Español](LIB.es.md)

## Instalação

```toml
[dependencies]
Kget = "1.6.3"
```

Features opcionais:

```toml
Kget = { version = "1.6.3", features = ["torrent-native"] }
Kget = { version = "1.6.3", features = ["gui"] }
```

## API Principal

- `download`: download HTTP/HTTPS em fluxo único com retry, proxy, streaming para disco e SHA256 opcional.
- `AdvancedDownloader`: download HTTP/HTTPS paralelo e retomável usando byte ranges.
- `download_magnet`: download de magnet links com cliente nativo quando compilado com `torrent-native`.
- `DownloadOptions`: modo silencioso, caminho de saída, verificação ISO e hash SHA256 esperado.
- `Config`, `ProxyConfig`, `Optimizer`: configuração reutilizável.
- `verify_file_sha256` e `verify_iso_integrity`: utilitários de checksum.
- `metalink::download_metalink`: analisa manifesto `.meta4`/`.metalink` e baixa todos os arquivos, tentando mirrors em ordem de prioridade com verificação automática de hash.
- `queue::{DownloadHistory, HistoryEntry, EntryStatus}`: histórico persistente de downloads em `history.json` no diretório de configuração do SO.

## Download Simples

```rust,no_run
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

    Ok(())
}
```

## SHA256 Esperado

```rust,no_run
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

let options = DownloadOptions {
    output_path: Some("imagem.iso".to_string()),
    verify_iso: true,
    expected_sha256: Some("hash_sha256_esperado".to_string()),
    ..Default::default()
};

download(
    "https://example.com/imagem.iso",
    ProxyConfig::default(),
    Optimizer::new(),
    options,
    Some(&|status| println!("{status}")),
)?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

Se o SHA256 calculado não bater, a função retorna erro.

## Download Avançado

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

O downloader avançado usa byte ranges e rejeita servidores que anunciam range
mas respondem com conteúdo completo, evitando corrupção silenciosa.

## Links Magnet

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

Use `download_magnet` com a feature `torrent-native` para o cliente torrent embutido. Sem essa feature, o KGet tenta usar o app padrão do sistema para magnet links.

## Comportamento da Biblioteca

- Chamadas da biblioteca nunca perguntam nada via `stdin`.
- Progresso e status são enviados por callbacks.
- Arquivos são gravados em streaming no disco.
- Nomes de saída são validados contra separadores de caminho.
- Helpers SHA256 retornam erro quando o hash esperado não confere.

## Downloads Metalink

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

Analisa o manifesto RFC 5854, ordena mirrors por prioridade (menor número = primeiro), tenta cada um e verifica SHA-256 após o download. Mirror corrompido é deletado e o próximo é tentado automaticamente.

## Histórico de Downloads

```rust,no_run
use kget::queue::{DownloadHistory, EntryStatus, HistoryEntry};

let mut history = DownloadHistory::load();

let entry = HistoryEntry::new(
    "https://example.com/arquivo.iso",
    "/home/user/Downloads",
    Some("hash_sha256_esperado"),
);
history.record(entry, EntryStatus::Completed, None);
history.save()?;

for e in history.recent(10) {
    println!("{} {} {}", e.created_at_display(), e.status, e.filename);
}
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

Localização do arquivo de histórico:
- macOS: `~/Library/Application Support/kget/history.json`
- Linux: `~/.config/kget/history.json`
- Windows: `%APPDATA%\kget\history.json`

Veja [examples/lib_usage.rs](../examples/lib_usage.rs) para exemplos maiores.
