<img width="1000" height="500" alt="KGet Banner" src="https://github.com/user-attachments/assets/d0888e3f-90a2-42d6-a9aa-b216dc36f1f4" />

# KGet v1.8.0

**Um gerenciador de downloads rápido e moderno escrito em Rust.**  
HTTP/HTTPS · FTP/SFTP · WebDAV · BitTorrent · Metalink · yt-dlp · Hugging Face · Ollama · CLI · TUI · GUI · MCP · Biblioteca

[English](../README.md) | [Português](README.pt-BR.md) | [Español](README.es.md)


> **1.8.0** coloca o KGet na era dos agentes: daemon com fila persistente, servidor MCP,
> extensão de navegador, TUI, fontes de modelos de IA, downloads turbo HTTP/2 e histórico
> SQLite — com extração de arquivos reforçada e credenciais no chaveiro do sistema.
> Veja o [CHANGELOG](CHANGELOG.pt-BR.md).

---

## Por que KGet?

A maioria das ferramentas de download é simples demais (clones de wget com conexão única) ou pesada demais (apps Electron). O KGet é um **motor de download nativo em Rust** que oferece:

- **Downloads turbo com múltiplas conexões** — divide arquivos em faixas paralelas de bytes (até 32× mais rápido em conexões rápidas)
- **Cobertura completa de protocolos** — HTTP/HTTPS (HTTP/2), FTP/SFTP, WebDAV, magnet links, yt-dlp e modelos de IA do Hugging Face e do Ollama em um único binário
- **Vários frontends, um motor** — app macOS nativo em SwiftUI, GUI multiplataforma egui, interface de terminal, uma CLI completa, uma extensão de navegador e um servidor MCP para agentes de IA, todos com a mesma fila de jobs (opcionalmente num daemon em segundo plano)
- **Uma biblioteca Rust reutilizável** — incorpore o motor de download no seu app com uma API builder fluente, erros tipados e canais de eventos

---

## Recursos

### Protocolos de Download
| Protocolo | Flag | Notas |
|-----------|------|-------|
| HTTP / HTTPS | *(auto)* | Multi-conexão, retomável, proxy HTTP/HTTPS/SOCKS5, arquivos idênticos ao do servidor (sem descompressão) |
| FTP | `ftp://` *(auto)* ou `--ftp` | Autenticado ou anônimo, retomável (`REST`) |
| SFTP | `sftp://` *(auto)* ou `--sftp` | Senha ou chave; verificação de host-key; retomável |
| WebDAV | `--webdav` ou `webdav://` | HTTP Basic auth embutido na URL |
| Magnet / BitTorrent | *(auto-detectado)* | Cliente torrent nativo (feature `torrent-native`) |
| Metalink `.meta4` / `.metalink` | *(auto)* ou `--metalink` | Múltiplos mirrors com fallback, SHA-256/512/MD5 verificado (RFC 5854) |
| Sites de vídeo | `--ytdlp` ou *(auto-detectado)* | YouTube, Vimeo, Twitch, TikTok, Instagram… via yt-dlp |
| Hugging Face | `hf://` ou `huggingface.co/…` *(auto)* | Modelos, datasets, spaces; `--include`/`--exclude`, `--revision`, `--hf-cache`; SHA-256 do LFS verificado |
| Ollama | `ollama://` *(auto)* | Baixa para o diretório de modelos do Ollama (ou arquivos comuns com `-O`); digests verificados |

### Motor de Download
- **Modo turbo** (`-a`) — conexões paralelas por faixas de bytes num motor assíncrono com work stealing, retomável
- **HTTP/2 e HTTP/3** (`--http-version`) — HTTP/2 negociado automaticamente; HTTP/3 experimental atrás de uma flag de build
- **Download em lote** (`--batch urls.txt`) — uma URL por linha, `#` = comentário, `-j/--jobs N` downloads simultâneos (padrão 4)
- **Agendamento** (`--at "HH:MM"`) — dorme até o horário local especificado
- **Limite de velocidade** (`-l <bytes/s>`) — throttle global por token-bucket
- **Headers HTTP customizados** (`-H "Nome: Valor"`) — injeta headers arbitrários
- **Auto-extração de arquivos** (`--extract`) — descompacta após o download (`.zip`, `.tar.gz`, `.7z`, …)
- **Verificação SHA-256** (`--sha256 <hash>`) — erro se não corresponder; nunca aceita arquivos corrompidos
- **Qualquer checksum** (`--checksum sha512:<hex>`) e **arquivos sidecar** (`--checksum-url …/SHA256SUMS`) — formatos GNU, BSD e só-hash
- **Content-Disposition** — usa nomes de arquivo sugeridos pelo servidor
- **Segurança de nomes de arquivo** — rejeita bytes nulos, path traversal, nomes reservados do Windows e nomes >255 bytes

### Integridade e Segurança
- **Checksums multi-algoritmo** — SHA-256, SHA-512, SHA-1, MD5, BLAKE3
- **Verificação de host-key SFTP** — checa `~/.ssh/known_hosts`; erro em caso de divergência
- **Política de retry** — tenta novamente em 408/429/5xx e erros de rede (respeita `Retry-After`); falha imediata nos demais 4xx
- **Senhas salvas** — senhas de FTP/SFTP/WebDAV/proxy e tokens do HF no chaveiro do sistema (`--save-credentials`, `--credentials`); nunca no `config.json`
- **Eventos JSONL** (`--jsonl`) — eventos versionados (`"v":1`) legíveis por máquina para todos os protocolos

### Histórico e Persistência
- **Histórico de downloads** — todo download gravado em SQLite (`history.db`, o `history.json` é migrado); busca com filtros, tags e `--json`
- **Regras inteligentes** — organiza em pastas, adiciona tags, pula ou cria hard links para duplicados
- **Daemon em segundo plano** — `kget daemon` + `kget queue …`: fila persistente que sobrevive a reinícios
- **REPL interativo** (`--interactive`) — todas as opções de download da CLI, lote, retry, checksums, Tab para completar, histórico persistente, Ctrl+C para pausar

### GUIs
- **App macOS nativo** — SwiftUI, sidebar `NavigationSplitView`, monitor de clipboard, drag-and-drop de URLs, sparkline de velocidade, aba de histórico, Share Extension, menu bar
- **GUI multiplataforma egui** (`--gui`) — design inspirado no Apple, modo escuro/claro adaptativo, navegação em sidebar, barra de progresso com shimmer
- **Interface de terminal** (`kget tui`) — fila, detalhes e histórico em tela cheia em qualquer terminal
- **Extensão de navegador** — Chrome, Edge, Brave, Firefox: envie links (ou todos os downloads) para o KGet

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
- **macOS** — `KGet-1.8.0-macOS-Native.dmg` (app SwiftUI nativo, sem necessidade de Rust)
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

### Modelos de IA (Hugging Face, Ollama)

```bash
kget hf://Qwen/Qwen2.5-0.5B-Instruct                          # repositório inteiro em ./Qwen2.5-0.5B-Instruct/
kget hf://TheBloke/Llama-2-7B-GGUF --include "*Q4_K_M.gguf"    # só os arquivos que casam
kget hf://org/repo --revision v1.0 --exclude "*fp32*" -j 8     # tag/branch/commit, 8 arquivos por vez
kget hf://datasets/org/nome/data/train.parquet                 # um arquivo de um dataset
kget hf://org/repo --hf-cache                                  # em ~/.cache/huggingface/hub para o transformers
kget ollama://llama3.2:3b                                      # em ~/.ollama/models, pronto para `ollama run`
kget ollama://llama3.2 -O ./modelos/                           # arquivos comuns
```

Repositórios restritos ou privados usam `HF_TOKEN`, `HUGGING_FACE_HUB_TOKEN`, o token do `hf auth login` ou um
token salvo com `kget --credentials add hf://token@huggingface.co`. Downloads de modelos interrompidos pulam os
arquivos já completos na próxima execução.

### Seleção de arquivos e seed de torrents

```bash
kget --list-files "magnet:?xt=urn:btih:HASH"                   # lista numerada de arquivos e sai
kget --select 1,3-5 "magnet:?xt=urn:btih:HASH"                 # só estes arquivos
kget --select-glob "*.mkv" ~/Downloads/serie.torrent           # só os caminhos que casam
kget --seed-ratio 1.5 --seed-time 2h "magnet:?xt=urn:btih:HASH"  # continua semeando depois
```

Seleção, `--list-files` e `--seed-time` exigem o cliente nativo (`--features torrent-native`); o backend
Transmission suporta `--select` e `--seed-ratio`. Com `--jsonl`, eventos `torrent_stats` informam peers, seeders,
velocidades, ratio e trackers.

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

### Busca no histórico

```bash
kget --history                    # lista os últimos 50 downloads
kget --history --status failed --domain example.com --since 7d
kget --history --type video --sort largest --history-limit 10
kget --history --search ubuntu --tag linux --until 2026-10-01
kget --history --json             # array JSON (com --jsonl: um objeto por linha)
kget --history-clear              # remove todas as entradas
kget --history-clear completed    # mantém só as falhas
kget --history-clear failed       # remove só as falhas
```

Filtros: `--status` (completed, failed, cancelled, handed_off), `--domain` (inclui subdomínios), `--type`
(video, audio, image, archive, document, disk, package ou extensões), `--protocol`, `--since`/`--until`
(`today`, `7d`, `12h`, `AAAA-MM-DD`; UTC), `--search`, `--tag`; `--sort newest|oldest|largest|smallest|name|fastest`,
`--history-limit`, `--history-offset`. O comando `history` do REPL aceita os mesmos filtros.

### Regras inteligentes, tags e duplicados

```bash
kget --rules add name=ISOs ext=iso domain=ubuntu.com,debian.org folder="ISOs/{domain}" tags=linux
kget --rules test https://releases.ubuntu.com/24.04/ubuntu.iso   # qual regra casa e para onde iria
kget --rules enable                     # organiza todo download (ou --organize para uma execução)
kget --rules enable builtins            # usa também as pastas embutidas Videos/Music/Images/…
kget --rules list                       # também: remove <nome>, disable [nome]
kget --rules suggest                    # regras sugeridas pelo histórico (--ai consulta o Claude, --apply adiciona)
kget --tag trabalho --tag q4 https://example.com/relatorio.pdf
kget --duplicates skip https://example.com/arquivo.zip   # já baixado? skip | ask | download | link
kget --dedupe https://example.com/copia.iso              # conteúdo idêntico já salvo? cria hard link
```

As regras ficam no `config.json` e são testadas em ordem (a primeira que casar vence). Elas casam por domínio,
regex/glob da URL, extensão, categoria, tipo MIME, tamanho ou protocolo, e podem definir pasta (`{category}`,
`{domain}`, `{ext}`, `{protocol}`, `{date:%Y-%m}`), tags, conexões, limite de velocidade, extração ou checksum
obrigatório. `-O` sempre vence a pasta da regra. `--rules suggest --ai` exige `ANTHROPIC_API_KEY` e envia só
contagens agregadas, depois da sua confirmação. Sem `--duplicates`, o KGet pergunta no terminal e baixa de novo
nos outros casos.

### Senhas salvas (chaveiro)

```bash
kget --save-credentials sftp://alice@servidor/backups/db.tar.gz   # pergunta uma vez e salva no chaveiro
kget sftp://alice@servidor/backups/db.tar.gz                      # na próxima vez: sem pergunta
kget --credentials add ftp://bob@files.example.com                # salvar antecipadamente
kget --credentials list                                           # só os alvos, nunca as senhas
kget --credentials delete ftp://bob@files.example.com
kget --credentials status                                         # qual chaveiro está em uso
```

As senhas vão para as Chaves do macOS, o Gerenciador de Credenciais do Windows ou o Secret Service (Linux/BSD).
Os alvos são `esquema://usuario@host[:porta]` (sftp, ftp, webdav, webdavs ou uma URL de proxy),
`ssh-key:///caminho/da/chave` para a frase da chave, ou `hf://token@huggingface.co`. `--save-config` também guarda
ali a senha do proxy. Defina `KGET_KEYCHAIN=off` para desativar.

### HTTP/2 e HTTP/3

```bash
kget -a https://example.com/grande.iso                       # auto: HTTP/2 via ALPN quando oferecido, senão HTTP/1.1
kget -a --http-version 1.1 https://example.com/grande.iso    # nunca HTTP/2
kget -a --http-version 2 http://h2c.example.com/grande.iso   # HTTP/2 com conhecimento prévio (também sem TLS)
```

Defina um padrão com `optimization.http_version` no `config.json`. `--http-version 3` (QUIC) é experimental e exige
`RUSTFLAGS="--cfg reqwest_unstable" cargo build --release --features http3`; os outros builds o rejeitam com uma
mensagem clara. Downloads Metalink buscam chunks de todos os mirrors ao mesmo tempo e descartam os que falham.

### Daemon e fila

```bash
kget daemon                          # inicia o daemon em segundo plano (--foreground para ficar em primeiro plano)
kget queue add -a https://example.com/grande.iso --wait   # enfileira um download e acompanha
kget queue list                      # também: status <id>, pause, resume, cancel, rm, watch [ids]
kget https://example.com/arquivo.zip --via-daemon         # qualquer download pelo daemon
kget daemon status                   # código de saída 3 se não estiver rodando
kget daemon stop                     # jobs não terminados continuam no próximo início
```

O daemon mantém uma fila persistente (`jobs.db`), roda `daemon.max_concurrent` jobs por vez (`-j`) e salva em
`daemon.download_dir` (padrão `~/Downloads`); os clientes não podem gravar fora dele, a menos que
`daemon.allow_outside_download_dir` esteja ativo. Ele escuta num socket Unix `0600` (TCP de loopback com token no
Windows). Veja [docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md#daemon-and-mcp) para o protocolo e o modelo de segurança.

### Uso por agentes de IA (MCP)

```bash
claude mcp add kget -- kget mcp                           # Claude Code
claude mcp add kget -- kget mcp --root ~/Downloads/agente # restringe o agente a uma pasta
```

`kget mcp` fala o Model Context Protocol em stdin/stdout. Ferramentas: `download`, `download_status`,
`list_downloads`, `cancel_download`, `pause_download`, `resume_download`, `search_history`, `verify_checksum`,
`get_config`. Toda gravação (e todo arquivo lido por `verify_checksum`) fica dentro de `--root` (padrão: o diretório
de downloads do daemon). Usa o daemon em execução, então os downloads sobrevivem à sessão do agente
(`--start-daemon` inicia um; `--no-daemon` mantém tudo no próprio processo).

### Interface de terminal

```bash
kget tui            # usa o daemon se estiver rodando, senão baixa neste processo
kget tui --daemon   # inicia/usa o daemon: os downloads continuam depois de sair
kget tui --local    # sempre no próprio processo
```

Teclas: `a` adiciona URL, `p`/Espaço pausa ou retoma, `c` cancela, `r` tenta de novo, `d` remove, `o` abre,
`1`/`2` fila ou histórico, `/` busca no histórico, `f` filtra por status, `?` ajuda, `q` sai. Incluída nos builds
padrão (feature `tui`).

### Extensão de navegador

```bash
kget native-host install                    # registra em todos os navegadores instalados
kget native-host install --browser firefox  # ou chrome, chromium, edge, brave
kget native-host uninstall
```

Depois carregue `browser-extension/` como extensão sem empacotamento (Chrome, Edge, Brave, Chromium) ou como
complemento temporário (Firefox 128+). Ela adiciona "Download with KGet" ao menu de contexto, um popup com progresso
ao vivo e interceptação opcional dos downloads do navegador. Os downloads passam pelo daemon (iniciado
automaticamente). Veja [browser-extension/README.md](../browser-extension/README.md).

### REPL Interativo

```bash
kget --interactive
```

```
kget> https://example.com/arquivo.zip       # colar uma URL já baixa
kget> cd ~/Downloads                        # pasta onde esta sessão salva os arquivos
kget> set turbo on                          # padrões da sessão: turbo, quality, retries, timeout
kget> download -a -o "ubuntu.iso" --checksum-url https://host/SHA256SUMS https://releases.ubuntu.com/...
kget> download -l 1048576 -p socks5://127.0.0.1:1080 https://example.com/grande.iso   # só neste download
kget> download --ytdlp --quality 720p https://youtube.com/watch?v=...
kget> batch urls.txt -j 8                   # downloads em paralelo a partir de um arquivo
kget> info https://example.com/arquivo.zip  # tamanho, tipo, retomável? (sem baixar)
kget> history failed                        # também: history 20 | completed | search <texto> | clear
kget> retry 2                               # refaz uma entrada; continua o .part
kget> open last                             # também: reveal, extract, checksum, verify
kget> config set connections 8              # salvo no config.json (config show | path | reset)
kget> doctor
kget> help download
```

Tab completa comandos, flags, chaves de config e caminhos; ↑/↓ navegam pelos comandos anteriores
(salvos entre sessões). Ctrl+C para o download em andamento e mantém os dados parciais — `retry last` continua.

### Eventos JSONL (para scripts e agentes)

```bash
kget --jsonl -a https://example.com/arquivo.iso | jq -c 'select(.event=="progress") | .percent'
```

Cada linha é um objeto JSON com `"v": 1` e um campo `event` (`rule`, `started`, `progress`, `status`,
`files`, `file_progress`, `torrent_stats`, `phase`, `verified`, `duplicate`, `saved`, `completed`, `error`). O evento `phase` indica
verificação (`{"event":"phase","phase":"verifying","algorithm":"sha256"}`) ou extração
(`{"event":"phase","phase":"extracting"}`). Use os campos estruturados, nunca o texto de `status`.
Novos eventos e campos não mudam `"v"`; ignore os que não conhecer. Veja o formato completo no
[README em inglês](../README.md#jsonl-events-for-scripts-and-agents).

### Todas as flags CLI

Rode `kget --help` para a lista completa.

| Flag | Descrição |
|------|-----------|
| `-a, --advanced` | Modo turbo — conexões paralelas, retomável |
| `--http-version <v>` | Versão HTTP do turbo: `auto` (HTTP/2 via ALPN), `1.1`, `2`, `3` (exige build `http3`); config: `optimization.http_version` |
| `-O, --output <caminho>` | Arquivo ou pasta de saída |
| `-q, --quiet` | Saída mínima |
| `-c, --continue` | Continua um download parcial (`<arquivo>.part`) |
| `--no-clobber` | Pula o download se o arquivo já existir |
| `--retries <n>` | Tentativas por requisição (padrão 3) |
| `--timeout <s>` | Timeout de conexão / por leitura (padrão 60) |
| `-p, --proxy <url>` | Proxy HTTP, HTTPS ou SOCKS5 |
| `--proxy-user`, `--proxy-pass` | Credenciais do proxy (a senha nunca vai para o `config.json`) |
| `--proxy-type <t>` | `http`, `https`, `socks5` (deduzido da URL do proxy se omitido) |
| `-l, --limit <bytes/s>` | Limite de velocidade em bytes por segundo (`0` = ilimitado) |
| `--save-config` | Grava as flags de proxy/limite/torrent no `config.json` (senha do proxy → chaveiro) |
| `--save-credentials` | Salva no chaveiro as senhas usadas por este download |
| `--credentials <ação> [alvo]` | `list`, `status`, `add <alvo>`, `delete <alvo>` |
| `-H, --header "Nome: Valor"` | Header HTTP extra (repetível) |
| `--sha256 <hash>` | Verificar SHA-256 após download |
| `--checksum <alg:hex>` | Verificar com sha256, sha512, sha1, md5 ou blake3 |
| `--checksum-url <url>` | Verificar contra um arquivo de checksums (GNU, BSD ou só hash) |
| `--extract` | Auto-extrair arquivos após download |
| `--at "HH:MM"` | Agendar download para horário local específico |
| `--batch <arquivo\|->` | Baixar todas as URLs de um arquivo (ou stdin) |
| `-j, --jobs <n>` | Downloads simultâneos no modo lote; arquivos por vez em `hf://`/`ollama://` (padrão 4) |
| `--include`, `--exclude <glob>` | `hf://`: só / pula arquivos que casam (repetível) |
| `--revision <rev>` | `hf://`: branch, tag ou commit |
| `--hf-cache` | `hf://`: grava no cache do hub do Hugging Face |
| `-t, --torrent` | Força torrent |
| `--list-files` | Torrent: lista numerada de arquivos e sai (`torrent-native`) |
| `--select <lista>`, `--select-glob <padrão>` | Torrent: baixa só alguns arquivos (repetível) |
| `--seed-ratio <r>`, `--seed-time <d>` | Torrent: continua semeando até um ratio / por um tempo (`30m`, `2h`) |
| `--max-peers`, `--max-seeds`, `--torrent-port`, `--no-dht` | Ajustes do cliente torrent |
| `--organize`, `--no-organize` | Aplica (ou ignora) as regras inteligentes neste download |
| `--rules <ação> …` | `list`, `test <url>`, `add chave=valor…`, `remove <nome>`, `enable\|disable [nome\|builtins]`, `suggest` (`--ai`, `--apply`) |
| `--tag <tag>` | Tag gravada no histórico (repetível); com `--history`: filtra por tag |
| `--duplicates <política>` | Já baixado: `skip`, `ask`, `download`, `link` |
| `--dedupe` | Cria hard link para conteúdo idêntico já presente no histórico |
| `--via-daemon` | Roda o download pelo daemon em segundo plano |
| `--doctor` | Imprime diagnósticos e sai |
| `--ftp`, `--sftp`, `--webdav` | Força FTP / SFTP / WebDAV |
| `--ytdlp` | Rotear pelo yt-dlp (auto-detectado para sites de vídeo) |
| `--quality <q>` | Qualidade yt-dlp: `best`, `1080p`, `720p`, `480p`, `360p`, `audio` |
| `--metalink` | Baixar de manifesto Metalink |
| `--history` | Exibir histórico (filtros: `--status`, `--domain`, `--type`, `--protocol`, `--since`, `--until`, `--search`, `--tag`, `--sort`, `--history-limit`, `--history-offset`, `--json`) |
| `--history-clear [completed\|failed]` | Limpar histórico |
| `--jsonl` | Emitir eventos JSON Lines para stdout |
| `--gui` | Abrir interface gráfica egui |
| `-i, --interactive` | Modo REPL interativo |

| Subcomando | Descrição |
|------------|-----------|
| `kget daemon [--foreground] [status\|stop]` | Daemon de downloads em segundo plano |
| `kget queue add\|list\|status\|cancel\|pause\|resume\|rm\|watch` | Gerencia a fila do daemon |
| `kget mcp [--root DIR]` | Servidor MCP para agentes de IA (stdio) |
| `kget tui [--daemon\|--local]` | Interface de terminal |
| `kget native-host install\|uninstall` | Registra o host nativo da extensão de navegador |

---

## Uso como Biblioteca

O KGet também é uma biblioteca Rust reutilizável. Adicione ao seu projeto:

```toml
[dependencies]
Kget = "1.8.0"
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
    .spawn();

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

Além do builder, a biblioteca expõe a mesma fila de jobs usada pelo daemon e pelas GUIs (`kget::JobManager`),
o despachante compartilhado (`kget::runner`), a busca no histórico (`kget::queue::HistoryQuery`), o chaveiro
(`kget::credentials`), downloads do Hugging Face/Ollama (`kget::hf`, `kget::ollama`) e um cliente do daemon.
Veja [LIB.pt-br.md](LIB.pt-br.md) e o [LIB.md em inglês](../LIB.md) (mais atualizado) para a referência completa.

---

## Build

```bash
# Somente CLI (sem GUI)
cargo build --release

# Com GUI egui (Linux/Windows/macOS)
cargo build --release --features gui

# Com cliente torrent nativo
cargo build --release --features torrent-native

# CLI mínima (sem chaveiro, sem interface de terminal)
cargo build --release --no-default-features

# HTTP/3 experimental (QUIC)
RUSTFLAGS="--cfg reqwest_unstable" cargo build --release --features http3

# App macOS nativo + DMG (requer Xcode)
./build-native-macos.sh

# Cross-compilar Linux/Windows a partir do macOS (requer zig)
brew install zig && cargo install cargo-zigbuild
./build-cross.sh
```

### Features

| Flag | Padrão | Adiciona |
|------|--------|----------|
| `keychain` | ✅ | Senhas salvas no chaveiro do sistema (Rust puro) |
| `tui` | ✅ | Interface de terminal `kget tui` (ratatui, Rust puro) |
| `gui` | | GUI egui, `torrent-native`, notificações |
| `torrent-native` | | Cliente BitTorrent embutido (librqbit): seleção de arquivos, seed, estatísticas do swarm |
| `torrent-transmission` | | Cliente RPC do Transmission |
| `http3` | | HTTP/3 experimental; exige `RUSTFLAGS="--cfg reqwest_unstable"` |
| `async` | | Helpers `download_async()` / `download_all_async()` do builder |

## Testes

```bash
cargo test
cargo test --lib --test unit_tests
cargo test --test mock_server_tests
cargo test --test robustness_tests  # disco cheio, cancelar no meio do chunk, arquivo que muda, Range ignorado
cargo test --test daemon_tests --test mcp_tests --test native_host_tests
cargo test -p kget-ffi              # testes do engine UniFFI
node --test browser-extension/tests/
cargo fuzz run url                  # fuzzing (cargo-fuzz, nightly; veja fuzz/)
./run-tests.sh
```

---

## Suporte a Plataformas

| Plataforma | CLI / TUI / daemon | GUI egui | App Nativo | Extensão de navegador |
|------------|--------------------|----------|------------|-----------------------|
| macOS | ✅ | ✅ | ✅ SwiftUI DMG | ✅ Navegadores Chromium, Firefox |
| Linux | ✅ | ✅ | — | ✅ Navegadores Chromium, Firefox |
| Windows | ✅ | ✅ | — | ✅ Navegadores Chromium, Firefox |

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
