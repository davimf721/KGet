<img width="1000" height="500" alt="KGet Banner" src="https://github.com/user-attachments/assets/d0888e3f-90a2-42d6-a9aa-b216dc36f1f4" />

# KGet v1.7.0

**Um gerenciador de downloads rápido e moderno escrito em Rust.**  
HTTP/HTTPS · FTP/SFTP · WebDAV · BitTorrent · Metalink · yt-dlp · CLI · GUI · Biblioteca

[English](../README.md) | [Português](README.pt-BR.md) | [Español](README.es.md)

---

## Por que KGet?

A maioria das ferramentas de download é simples demais (clones de wget com conexão única) ou pesada demais (apps Electron). O KGet é um **motor de download nativo em Rust** que oferece:

- **Downloads turbo com múltiplas conexões** — divide arquivos em faixas paralelas de bytes (até 32× mais rápido em conexões rápidas)
- **Cobertura completa de protocolos** — HTTP/HTTPS, FTP/SFTP, WebDAV, magnet links e yt-dlp em um único binário
- **Três frontends, um motor** — app macOS nativo em SwiftUI, GUI multiplataforma egui (Linux/Windows) e uma CLI completa
- **Uma biblioteca Rust reutilizável** — incorpore o motor de download no seu app com uma API builder fluente, erros tipados e canais de eventos

---

## Recursos

### Protocolos de Download
| Protocolo | Flag | Notas |
|-----------|------|-------|
| HTTP / HTTPS | *(auto)* | Multi-conexão, retomável, gzip/brotli/lz4, proxy |
| FTP | `--ftp` | Autenticado ou anônimo |
| SFTP | `--sftp` | Senha ou chave; verificação de host-key |
| WebDAV | `--webdav` ou `webdav://` | HTTP Basic auth embutido na URL |
| Magnet / BitTorrent | *(auto-detectado)* | Cliente torrent nativo (feature `torrent-native`) |
| Metalink `.meta4` | `--metalink` | Múltiplos mirrors com fallback, SHA-256 verificado (RFC 5854) |
| Sites de vídeo | `--ytdlp` ou *(auto-detectado)* | YouTube, Vimeo, Twitch, TikTok, Instagram… via yt-dlp |

### Motor de Download
- **Modo turbo** (`-a`) — conexões paralelas por faixas de bytes, retomável
- **Download em lote** (`--batch urls.txt`) — uma URL por linha, `#` = comentário, todas em paralelo
- **Agendamento** (`--at "HH:MM"`) — dorme até o horário local especificado
- **Limite de velocidade** (`-l <bytes/s>`) — throttle global por token-bucket
- **Headers HTTP customizados** (`-H "Nome: Valor"`) — injeta headers arbitrários
- **Auto-extração de arquivos** (`--extract`) — descompacta após o download (`.zip`, `.tar.gz`, `.7z`, …)
- **Verificação SHA-256** (`--sha256 <hash>`) — erro se não corresponder; nunca aceita arquivos corrompidos
- **Arquivos de checksum sidecar** — verifica contra arquivos GNU/BSD `.sha256sum`
- **Content-Disposition** — usa nomes de arquivo sugeridos pelo servidor
- **Segurança de nomes de arquivo** — rejeita bytes nulos, path traversal, nomes reservados do Windows e nomes >255 bytes

### Integridade e Segurança
- **Checksums multi-algoritmo** — SHA-256, SHA-512, SHA-1, MD5, BLAKE3
- **Verificação de host-key SFTP** — checa `~/.ssh/known_hosts`; erro em caso de divergência
- **Política de retry** — tenta novamente apenas em 5xx e erros de rede; falha imediata em 4xx
- **Eventos JSONL** (`--jsonl`) — progresso legível por máquina para scripts e agentes

### Histórico e Persistência
- **Histórico de downloads** — todo download gravado em `history.json`; `--history` / `--history-clear`
- **REPL interativo** (`--interactive`) — histórico completo, todos os protocolos, edição de config ao vivo

### GUIs
- **App macOS nativo** — SwiftUI, sidebar `NavigationSplitView`, monitor de clipboard, drag-and-drop de URLs, sparkline de velocidade, aba de histórico, Share Extension, menu bar
- **GUI multiplataforma egui** (`--gui`) — design inspirado no Apple, modo escuro/claro adaptativo, navegação em sidebar, barra de progresso com shimmer

### Biblioteca
- **API builder fluente** — `kget::builder(url).connections(8).sha256("…").download()?`
- **Erros tipados** — enum `KgetError` com impls `From` para `reqwest::Error`, `io::Error`
- **Canal de eventos** — `.spawn()` retorna `(JoinHandle, Receiver<DownloadEvent>)`
- **API Async** — `.download_async()` / `.download_all_async()` com `--features async`
- **Download em memória** — `.download_to_bytes()` e `.download_to_reader()`
- **Batch builder** — `kget::batch([…]).concurrency(4).download_all()`

---

## Instalação

### Homebrew (macOS / Linux)

```bash
brew tap davimf721/kget
brew install kget                           # somente CLI
brew install kget --with-gui                # com interface gráfica egui
brew install kget --with-torrent            # com cliente BitTorrent nativo
brew install kget --with-gui --with-torrent # todas as features opcionais
```

### Binários prontos

Baixe a versão mais recente em [Releases](https://github.com/davimf721/KGet/releases):
- **macOS** — `KGet-1.7.0-macOS-Native.dmg` (app SwiftUI nativo, sem necessidade de Rust)
- **Linux/Windows** — binário CLI ou GUI (veja os assets do release)

### Via crates.io

```bash
cargo install Kget --features gui   # com GUI egui
cargo install Kget                  # somente CLI
```

### A partir do código-fonte

```bash
# Toolchain Rust: https://rustup.rs

# Dependências Linux (Debian/Ubuntu)
sudo apt install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
                    libxkbcommon-dev libssl-dev pkg-config

git clone https://github.com/davimf721/KGet.git
cd KGet
cargo build --release --features gui
./target/release/kget --gui
```

---

## Uso

### Downloads básicos

```bash
# HTTP/HTTPS
kget https://example.com/arquivo.zip

# Salvar em local específico
kget -O ~/Downloads/meuarquivo.zip https://example.com/arquivo.zip

# Modo turbo — conexões paralelas, retomável
kget -a https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso

# Modo silencioso
kget -q https://example.com/arquivo.zip
```

### Protocolos

```bash
# FTP (anônimo ou autenticado)
kget --ftp ftp://ftp.gnu.org/gnu/emacs/emacs-28.2.tar.gz
kget --ftp ftp://usuario:senha@servidor/arquivo.zip

# SFTP (senha ou chave)
kget --sftp sftp://usuario:senha@servidor/caminho/arquivo.dat
kget --sftp sftp://usuario@servidor/caminho/arquivo.dat

# WebDAV (auto-detectado pelo scheme)
kget webdav://arquivos.meuservidor.com/compartilhamento/relatorio.pdf
kget webdavs://usuario:senha@nas.local/backups/db.tar.gz

# Magnet link (auto-detectado)
kget "magnet:?xt=urn:btih:HASH&dn=nomedoarquivo"

# Metalink — tenta mirrors em ordem de prioridade, verifica SHA-256
kget --metalink ubuntu-24.04.meta4
kget https://releases.ubuntu.com/ubuntu.meta4
```

### Downloads de vídeo

```bash
# Auto-detectado pelo host da URL
kget https://www.youtube.com/watch?v=dQw4w9WgXcQ

# Flag explícita com qualidade
kget --ytdlp --quality 1080p https://vimeo.com/123456
kget --ytdlp --quality audio https://www.youtube.com/watch?v=dQw4w9WgXcQ

# Qualidades: best, 1080p, 720p, 480p, 360p, audio
```

### Lote e agendamento

```bash
# Lote — uma URL por linha, # = comentário
kget --batch urls.txt -O ~/Downloads/

# Agendar para as 23h de hoje
kget --at "23:00" -a https://example.com/arquivo-grande.iso
```

### Checksums e verificação

```bash
# Verificar SHA-256 esperado
kget --sha256 abc123def456... https://example.com/arquivo.iso

# Auto-extrair após download
kget --extract https://example.com/arquivo.tar.gz

# Headers customizados
kget -H "Authorization: Bearer token123" -H "Accept: application/json" https://api.exemplo.com/exportar
```

### Histórico

```bash
kget --history                    # lista os últimos 50 downloads
kget --history-clear              # remove todas as entradas
kget --history-clear completed    # remove apenas concluídos/cancelados
```

### REPL Interativo

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

### Eventos JSONL (para scripts e agentes)

```bash
kget --jsonl -a https://example.com/arquivo.iso | jq '.percent'
```

### Todas as flags CLI

| Flag | Descrição |
|------|-----------|
| `-a, --advanced` | Modo turbo — conexões paralelas, retomável |
| `-O <caminho>` | Arquivo ou pasta de saída |
| `-q, --quiet` | Saída mínima |
| `-p <proxy>` | Proxy HTTP/SOCKS5 |
| `-l <bytes/s>` | Limite de velocidade em bytes por segundo |
| `-H "Nome: Valor"` | Header HTTP extra (repetível) |
| `--sha256 <hash>` | Verificar SHA-256 após download |
| `--extract` | Auto-extrair arquivos após download |
| `--at "HH:MM"` | Agendar download para horário local específico |
| `--batch <arquivo>` | Baixar todas as URLs de um arquivo |
| `--ftp` | Usar protocolo FTP |
| `--sftp` | Usar protocolo SFTP |
| `--webdav` | Usar protocolo WebDAV |
| `--ytdlp` | Rotear pelo yt-dlp (auto-detectado para sites de vídeo) |
| `--quality <q>` | Qualidade yt-dlp: `best`, `1080p`, `720p`, `480p`, `360p`, `audio` |
| `--metalink` | Baixar de manifesto Metalink |
| `--history` | Exibir histórico de downloads |
| `--history-clear [completed]` | Limpar histórico |
| `--jsonl` | Emitir eventos JSON Lines para stdout |
| `--gui` | Abrir interface gráfica egui |
| `-i, --interactive` | Modo REPL interativo |

---

## Uso como Biblioteca

O KGet também é uma biblioteca Rust reutilizável. Adicione ao seu projeto:

```toml
[dependencies]
Kget = "1.7.0"
```

### API Builder (recomendada)

```rust
use kget::KgetError;

// Download simples
kget::builder("https://example.com/arquivo.zip")
    .output("./downloads/")
    .connections(8)
    .sha256("abc123...")
    .download()?;

// Lote paralelo com canal de eventos
let results = kget::batch([
    "https://mirror1.example.com/arquivo.iso",
    "https://mirror2.example.com/outro.tar.gz",
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
        kget::DownloadEvent::Completed { path, .. } => println!("\nSalvo em {}", path),
        kget::DownloadEvent::Error(e) => eprintln!("Erro: {}", e),
        _ => {}
    }
}
handle.join().ok();
```

Veja [LIB.pt-br.md](LIB.pt-br.md) para a referência completa da biblioteca.

---

## Build

```bash
# Somente CLI (sem GUI)
cargo build --release

# Com GUI egui (Linux/Windows/macOS)
cargo build --release --features gui

# Com cliente torrent nativo
cargo build --release --features torrent-native

# App macOS nativo + DMG (requer Xcode)
./build-native-macos.sh

# Cross-compilar Linux/Windows a partir do macOS (requer zig)
brew install zig && cargo install cargo-zigbuild
./build-cross.sh
```

## Testes

```bash
cargo test
cargo test --lib --test unit_tests
cargo test --test mock_server_tests
./run-tests.sh
```

---

## Suporte a Plataformas

| Plataforma | CLI | GUI egui | App Nativo |
|------------|-----|----------|------------|
| macOS | ✅ | ✅ | ✅ SwiftUI DMG |
| Linux | ✅ | ✅ | — |
| Windows | ✅ | ✅ | — |

---

## Links

- [Changelog](../CHANGELOG.md)
- [Referência da Biblioteca (LIB.pt-br.md)](LIB.pt-br.md)
- [Arquitetura](../docs/ARCHITECTURE.md)
- [crates.io](https://crates.io/crates/Kget)
- [Contribuindo](../CONTRIBUTING.md)

## Comunidade

- [Dev.to](https://dev.to/davimf7221/kelpsget-v014-modern-download-manager-in-rust-4b9f)
- [r/rust](https://www.reddit.com/r/rust/comments/1kt69vh/after_5_months_of_development_i_finally_released/)
- [PitchHut](https://www.pitchhut.com/project/kelpsget)

## Licença

MIT — veja [LICENSE](../LICENSE)
