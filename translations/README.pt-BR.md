# KelpsGet v0.1.4 (Nova Versão)

Um downloader moderno, leve e versátil escrito em Rust para downloads rápidos e confiáveis de arquivos via linha de comando (CLI) e interface gráfica (GUI).

[English](../README.md) | [Português](translations/README.pt-BR.md) | [Español](translations/README.es.md)

## Features
✅ Ferramenta CLI e GUI simples para baixar arquivos via HTTP/HTTPS.<br>
✅ Suporte para downloads via FTP e SFTP.<br>
✅ Suporte para downloads de Torrents (links magnéticos) via integração com Transmission.<br>
✅ Barra de progresso com velocidade em tempo real e rastreamento de ETA (CLI).<br>
✅ Nomes de saída personalizados (flag -O para renomear arquivos baixados).<br>
✅ Detecção de tipo MIME e manipulação adequada de arquivos.<br>
✅ Multiplataforma (Linux, macOS, Windows).<br>
✅ Modo silencioso para scripts.<br>
✅ Verificação automática de espaço antes do download.<br>
✅ Tentativa automática em caso de falha de conexão.<br>
✅ Validação de nome de arquivo.<br>
✅ Exibição detalhada de informações de download.<br>
✅ Modo de download avançado com partes paralelas e capacidade de resumo (HTTP/HTTPS).<br>
✅ Suporte a Proxy (HTTP, HTTPS, SOCKS5).<br>
✅ Compressão e cache automáticos (para otimizações específicas do KelpsGet).<br>
✅ Limite de velocidade e controle de conexão.<br>

## Instalação

### Opção 1: Compilar a partir do código-fonte (Recomendado para ter todas as funcionalidades)

Você precisará do Rust instalado. Se não o tiver, instale-o a partir de [rustup.rs](https://rustup.rs/).

Para compilar com todas as funcionalidades, incluindo a GUI, você pode precisar de algumas dependências de desenvolvimento.
Para sistemas baseados em Debian/Ubuntu:
```bash
sudo apt update
sudo apt install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev pkg-config
```
Para Fedora:
```bash
sudo dnf install -y libxcb-devel libxkbcommon-devel openssl-devel pkg-config
```

Clone o repositório e compile o projeto:
```bash
git clone https://github.com/davimf721/KelpsGet.git
cd KelpsGet
cargo build --release
```
O executável estará em `target/release/kelpsget`. Você pode copiá-lo para um diretório em seu `PATH`:
```bash
sudo cp target/release/kelpsget /usr/local/bin/
```

### Opção 2: Instalar via Cargo (Pode não incluir todas as dependências da GUI por padrão)
```bash
cargo install kelpsget
```
Se encontrar problemas com a GUI ao instalar via `cargo install`, compilar a partir do código-fonte é mais garantido.

### Opção 3: Baixar Binários Pré-compilados
Verifique a seção [Release](https://github.com/davimf721/KelpsGet/releases) para os binários mais recentes para o seu SO.

#### Linux/macOS:
```bash
chmod +x kelpsget  # Torna executável
./kelpsget [URL]    # Executa diretamente
```
#### Windows:
Execute o arquivo `.exe` diretamente.

### Requisito Adicional para Downloads de Torrent: Transmission Daemon

KelpsGet usa o `transmission-daemon` para gerenciar downloads de torrent.

**1. Instalar o Transmission Daemon:**
*   **Debian/Ubuntu:**
    ```bash
    sudo apt update
    sudo apt install transmission-daemon
    ```
*   **Fedora:**
    ```bash
    sudo dnf install transmission-daemon
    ```
*   **Arch Linux:**
    ```bash
    sudo pacman -S transmission-cli
    ```

**2. Parar o Daemon para Configuração:**
```bash
sudo systemctl stop transmission-daemon
```

**3. Configurar o Transmission:**
Edite o arquivo `settings.json`. Localizações comuns:
*   `/var/lib/transmission-daemon/info/settings.json` (Debian/Ubuntu, se instalado como serviço)
*   `/var/lib/transmission/.config/transmission-daemon/settings.json`
*   `~/.config/transmission-daemon/settings.json` (se executado como usuário)

Use `sudo nano /var/lib/transmission/.config/transmission-daemon/settings.json` (ou o caminho correto para seu sistema).

Procure e modifique estas linhas:
```json
{
    // ...
    "rpc-authentication-required": true,
    "rpc-enabled": true,
    "rpc-password": "transmission", // esse é o valor utilizado pelo kelpsget por padrão para se conectar ao transmission(recomendado)
    "rpc-port": 9091,
    "rpc-username": "transmission", // Nome de usuario que o kelpsget usa para se conectar ao transmission
    "rpc-whitelist-enabled": false, // Para acesso local. Para acesso remoto, configure IPs.
    "download-dir": "/var/lib/transmission-daemon/downloads", // Diretório padrão de downloads do Transmission
    // ...
}
```
**Importante:** Após salvar e iniciar o `transmission-daemon`, ele substituirá a `rpc-password` em texto plano por uma versão hasheada.

**4. (Opcional) Ajustar Permissões do Usuário do Daemon:**
Se o `transmission-daemon` roda como um usuário específico (ex: `debian-transmission` ou `transmission`), certifique-se de que este usuário tenha permissões de escrita nos diretórios de download que você pretende usar com KelpsGet ou com o próprio Transmission. Você pode adicionar seu usuário ao grupo do daemon Transmission:
```bash
sudo usermod -a -G debian-transmission seu_usuario_linux # Para Debian/Ubuntu
# Verifique o nome do grupo/usuário do Transmission no seu sistema
```

**5. Iniciar o Daemon Transmission:**
```bash
sudo systemctl start transmission-daemon
# Verifique o status:
sudo systemctl status transmission-daemon
```
Acesse `http://localhost:9091` no seu navegador. Você deverá ver a interface web do Transmission.

## Uso

### Linha de Comando (CLI)
```bash
kelpsget [OPÇÕES] <URL>
```
**Exemplos:**
*   **Download HTTP/HTTPS:**
    ```bash
    kelpsget https://example.com/file.txt
    ```
*   **Renomear Arquivo de Saída:**
    ```bash
    kelpsget -O novo_nome.txt https://example.com/file.txt
    kelpsget -O ~/MeusDownloads/ https://example.com/video.mp4 # Salva como ~/MeusDownloads/video.mp4
    ```
*   **Download FTP:**
    ```bash
    kelpsget ftp://usuario:senha@ftp.example.com/arquivo.zip
    kelpsget --ftp ftp://ftp.example.com/pub/arquivo.txt
    ```
*   **Download SFTP:**
    (Requer configuração de chave SSH ou senha se o servidor permitir)
    ```bash
    kelpsget sftp://usuario@sftp.example.com/caminho/arquivo.dat
    kelpsget --sftp sftp://usuario@sftp.example.com/caminho/arquivo.dat -O local.dat
    ```
*   **Download de Torrent (Link Magnético):**
    (Requer `transmission-daemon` configurado e em execução)
    ```bash
    kelpsget "magnet:?xt=urn:btih:SEU_HASH_AQUI&dn=NomeDoTorrent"
    kelpsget --torrent "magnet:?xt=urn:btih:SEU_HASH_AQUI" -O ~/MeusTorrents/
    ```
    KelpsGet adicionará o torrent ao Transmission e tentará abrir a interface web (`http://localhost:9091`) para gerenciamento.

*   **Modo Silencioso:**
    ```bash
    kelpsget -q https://example.com/file.txt
    ```
*   **Modo de Download Avançado (HTTP/HTTPS):**
    ```bash
    kelpsget -a https://example.com/large_file.zip
    ```
*   **Usar Proxy:**
    ```bash
    kelpsget -p http://proxy:8080 https://example.com/file.txt
    ```
*   **Proxy com Autenticação:**
    ```bash
    kelpsget -p http://proxy:8080 --proxy-user user --proxy-pass pass https://example.com/file.txt
    ```
*   **Limite de Velocidade:**
    ```bash
    kelpsget -l 1048576 https://example.com/file.txt  # Limite de 1MB/s
    ```
*   **Desabilitar Cache (específico do KelpsGet):**
    ```bash
    kelpsget --no-cache https://example.com/file.txt
    ```

### Interface Gráfica do Usuário (GUI)
Para iniciar a GUI:
```bash
kelpsget --gui
```
A GUI permite inserir a URL, o caminho de saída e iniciar downloads. O status e o progresso são exibidos na interface.

## Configuração do KelpsGet
KelpsGet usa um arquivo de configuração em:
- Windows: `%APPDATA%\kelpsget\config.json`
- Linux/macOS: `~/.config/kelpsget/config.json`

**Exemplo de configuração `config.json` do KelpsGet:**
```json
{
  "proxy": {
    "enabled": false,
    "url": null,
    "username": null,
    "password": null,
    "proxy_type": "Http"
  },
  "optimization": {
    "compression": true, // Compressão para cache do KelpsGet
    "compression_level": 6,
    "cache_enabled": true,
    "cache_dir": "~/.cache/kelpsget", // Expanda ~ manualmente ou use caminho absoluto
    "speed_limit": null,
    "max_connections": 4
  },
  "torrent": {
    "enabled": true,
    "transmission_url": "http://localhost:9091/transmission/rpc",
    "username": "transmission", // Usuário configurado no settings.json do Transmission
    "password": "transmission", // Senha configurada no settings.json do Transmission
    "max_peers": 50,
    "max_seeds": 50,
    "port": null,
    "dht_enabled": true,
    "default_download_dir": null // Diretório padrão para downloads de torrent via KelpsGet
  },
  "ftp": {
    "default_port": 21,
    "passive_mode": true
  },
  "sftp": {
    "default_port": 22,
    "key_path": null // Caminho para chave privada SSH, ex: "~/.ssh/id_rsa"
  }
}
```
## Como Funciona (Resumo)
1.  **Barra de Progresso (CLI):** Mostra velocidade, ETA e bytes transferidos.
2.  **Nomenclatura Inteligente de Arquivos:**
    *   Usa o nome do arquivo da URL.
    *   Padrão para `index.html` se a URL terminar com `/`.
3.  **Tratamento de Erros:** Sai com código 1 em erros HTTP (ex: 404).
4.  **Verificação de Espaço:** Verifica o espaço em disco disponível.
5.  **Tentativa Automática:** Tenta novamente o download em caso de falha de rede.
6.  **Modo de Download Avançado (HTTP/HTTPS):** Baixa em partes paralelas, suporta resumo.
7.  **Suporte a Proxy:** HTTP, HTTPS, SOCKS5 com autenticação.
8.  **Recursos de Otimização:** Compressão (para cache), cache de arquivos, limite de velocidade.
9.  **Downloads de Torrent:** Adiciona links magnéticos ao `transmission-daemon` para download.
10. **Downloads FTP/SFTP:** Conecta-se a servidores FTP/SFTP para transferir arquivos.

## Funcionalidades de Segurança
- Verificação de Espaço: Garante espaço em disco suficiente.
- Validação de Nome de Arquivo: Previne injeção de caminho.
- Manipulação Segura de URLs.
- Suporte a Proxy Seguro.

## Contribuindo
Encontrou um bug ou quer adicionar uma funcionalidade? Abra uma issue ou envie um PR!

🚀 Baixe arquivos sem esforço com a velocidade e confiabilidade do Rust. 🚀

## 🔗 Links Importantes
- 📚 [Documentação](https://davimf721.github.io/KelpsGet/)
- 📦 [crates.io](https://crates.io/crates/kelpsget)
- 💻 [GitHub](https://github.com/davimf721/KelpsGet)
- 📝 [Changelog](CHANGELOG.md)

## 🎯 Próximos Passos (Exemplo - ajuste conforme seu projeto)
- [X] Suporte a download FTP/SFTP
- [X] Suporte a download de Torrent
- [X] Interface GUI Desktop
- [ ] Interface web para monitoramento de download
- [ ] Integração com serviços de armazenamento em nuvem
- [ ] Sistema de plugins personalizados
- [ ] Melhorias na compressão adaptativa
- [ ] Otimização do sistema de cache
- [ ] Suporte a protocolos de proxy adicionais
- [ ] Documentação multilíngue (em andamento)

Quer contribuir? Confira nosso [guia de contribuição](CONTRIBUTING.md)!

## Licença
Este projeto está licenciado sob a Licença MIT - veja o arquivo [LICENSE](LICENSE) para detalhes.
