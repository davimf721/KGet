# KGet v1.6.3

Um gerenciador de downloads moderno e rápido escrito em Rust. O KGet suporta HTTP/HTTPS, FTP/SFTP e magnet links com cliente torrent nativo.

[English](../README.md) | [Português](README.pt-BR.md) | [Español](README.es.md)

## Recursos

- **Multi-protocolo:** HTTP, HTTPS, FTP, SFTP e magnet links.
- **Cliente torrent nativo:** downloads por magnet sem depender de apps externos quando compilado com `torrent-native`.
- **Modo turbo:** downloads HTTP/HTTPS paralelos com byte ranges, retomável.
- **Modo interativo REPL:** `kget --interactive` com histórico, todos os protocolos e edição de config ao vivo.
- **GUI e CLI:** interface gráfica e uso por terminal.
- **Multiplataforma:** macOS, Linux e Windows.
- **Verificação SHA256:** valida ISOs e qualquer arquivo com hash esperado.
- **Eventos JSONL:** progresso experimental em formato legível por scripts e agentes.
- **App macOS nativo:** menu de contexto, atalhos, ações Abrir Arquivo/Abrir Pasta e detecção de duplicatas.
- **Notificações nativas:** conclusão e falhas na GUI Rust em Linux/Windows.

## Instalação

### A partir do código-fonte

```bash
# Instale Rust em https://rustup.rs se necessário

# Dependências Linux (Debian/Ubuntu)
sudo apt install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev pkg-config

git clone https://github.com/davimf721/KGet.git
cd KGet
cargo build --release --features gui

./target/release/kget --gui
```

### Via crates.io

```bash
cargo install Kget --features gui
```

### Binários prontos

Baixe versões para macOS, Linux e Windows em [Releases](https://github.com/davimf721/KGet/releases).

## Uso

```bash
# Download simples
kget https://example.com/arquivo.zip

# Modo turbo (paralelo, retomável)
kget -a https://example.com/grande.iso

# Salvar em local específico
kget -O ~/Downloads/arquivo.zip https://example.com/arquivo.zip

# Verificar SHA256 esperado
kget --sha256 <hash> https://example.com/imagem.iso

# Magnet link (detectado automaticamente)
kget "magnet:?xt=urn:btih:HASH..."

# FTP anônimo
kget --ftp ftp://ftp.gnu.org/gnu/emacs/emacs-28.2.tar.gz

# FTP autenticado
kget --ftp ftp://usuario:senha@servidor/arquivo.zip

# SFTP com senha na URL
kget --sftp sftp://usuario:senha@servidor/caminho/arquivo.dat

# SFTP com chave SSH (usa SSH agent ou ~/.ssh/id_*)
kget --sftp sftp://usuario@servidor/caminho/arquivo.dat
```

## Modo Interativo

```bash
kget --interactive
```

Abre um REPL com banner ASCII, histórico de comandos e suporte a todos os protocolos:

```
kget> download -a -o ~/Downloads/ubuntu.iso https://releases.ubuntu.com/...
kget> download --sftp sftp://user@servidor/backups/db.sql.gz
kget> download magnet:?xt=urn:btih:...
kget> config set connections 8
kget> config set speed-limit 1048576
kget> help
```

## Opções principais

| Flag | Descrição |
|------|-----------|
| `-a, --advanced` | Modo turbo com conexões paralelas (retomável) |
| `-O <path>` | Arquivo ou pasta de saída |
| `-q, --quiet` | Saída mínima |
| `-p <proxy>` | Proxy HTTP/SOCKS5 |
| `-l <bytes>` | Limite de velocidade em bytes/s |
| `--sha256 <hash>` | Verifica o arquivo final contra um hash SHA256 esperado |
| `--jsonl` | Emite eventos JSON Lines experimentais para scripts e agentes |
| `--ftp` | Usar protocolo FTP |
| `--sftp` | Usar protocolo SFTP (senha ou autenticação por chave) |
| `--gui` | Abre a interface gráfica |
| `-i, --interactive` | Abre o modo REPL interativo |

## Biblioteca Rust

O KGet também é uma biblioteca Rust reutilizável. Veja [LIB.pt-br.md](LIB.pt-br.md) para exemplos completos da API atual.

```rust
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

let options = DownloadOptions::default();
download(
    "https://example.com/arquivo.zip",
    ProxyConfig::default(),
    Optimizer::new(),
    options,
    None,
)?;
```

## Build e testes

```bash
cargo build --release
cargo build --release --features gui
cargo test
./run-tests.sh
```

## Links

- [Documentação](https://davimf721.github.io/KGet/)
- [Changelog](CHANGELOG.pt-BR.md)
- [crates.io](https://crates.io/crates/Kget)
- [Contribuição](CONTRIBUTING.pt-BR.md)

## Licença

MIT - veja [LICENSE](../LICENSE).
