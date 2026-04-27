# KGet v1.6.1

Um gerenciador de downloads moderno e rápido escrito em Rust. O KGet suporta HTTP/HTTPS, FTP/SFTP e magnet links com cliente torrent nativo.

[English](../README.md) | [Português](README.pt-BR.md) | [Español](README.es.md)

## Recursos

- **Multi-protocolo:** HTTP, HTTPS, FTP, SFTP e magnet links.
- **Cliente torrent nativo:** downloads por magnet sem depender de apps externos quando compilado com `torrent-native`.
- **Modo turbo:** downloads HTTP/HTTPS paralelos com byte ranges.
- **GUI e CLI:** interface gráfica e uso por terminal.
- **Multiplataforma:** macOS, Linux e Windows.
- **Verificação SHA256:** valida ISOs e qualquer arquivo com hash esperado.
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

# Modo turbo
kget -a https://example.com/grande.iso

# Salvar em local específico
kget -O ~/Downloads/arquivo.zip https://example.com/arquivo.zip

# Verificar SHA256 esperado
kget --sha256 <hash> https://example.com/imagem.iso

# Magnet link
kget "magnet:?xt=urn:btih:HASH..."

# FTP/SFTP
kget ftp://usuario:senha@servidor/arquivo.zip
kget sftp://usuario@servidor/arquivo.dat
```

## Opções principais

| Flag | Descrição |
|------|-----------|
| `-a, --advanced` | Modo turbo com conexões paralelas |
| `-O <path>` | Arquivo ou pasta de saída |
| `-q, --quiet` | Saída mínima |
| `-p <proxy>` | Proxy HTTP/SOCKS5 |
| `-l <bytes>` | Limite de velocidade em bytes/s |
| `--sha256 <hash>` | Verifica o arquivo final contra um hash SHA256 esperado |
| `--gui` | Abre a interface gráfica |
| `--interactive` | Abre o modo REPL interativo |

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
