# KelpsGet agora é KGet! v1.5.2 (Novo Lançamento)

Um downloader moderno, leve e versátil escrito em Rust para downloads rápidos e confiáveis via linha de comando (CLI) e interface gráfica (GUI).

[English](../README.md) | [Português](README.pt-BR.md) | [Español](README.es.md)

## Screenshots
- GUI:
 <img src="https://github.com/user-attachments/assets/30f77e72-aaac-454f-ace4-947b92411bf7"  width="600"/>
 
- Torrent em `localhost:9091/transmission/web/`:
 <img src="https://github.com/user-attachments/assets/d80b60d7-f53e-4198-8e11-1cacf0e78958"  width="600"/>

- CLI:
 <img src="https://github.com/user-attachments/assets/a835c4df-5424-4aaa-b687-2445a99ba067"  width="600"/>

- Interativo:
<img src="https://github.com/user-attachments/assets/c8d03a5c-6459-4f3d-a581-5180797f8b1c"  width="600"/>

## Como Funciona (Resumo)
1. **Barra de Progresso (CLI):** Mostra velocidade, tempo estimado e bytes transferidos.
2. **Nomeação Inteligente de Arquivos:**
    * Usa o nome do arquivo da URL.
    * Usa `index.html` por padrão se a URL terminar com `/`.
3. **Tratamento de Erros:** Encerra com código 1 em erros HTTP (ex: 404).
4. **Verificação de Espaço:** Verifica espaço disponível em disco.
5. **Retry Automático:** Tenta novamente o download em falhas de rede.
6. **Detecção Inteligente de ISO:** Detecta arquivos `.iso` para garantir transferência binária pura e evitar corrupção.
7. **Verificação de Integridade:** Verificação opcional de SHA256 para imagens de disco após o download.
8. **Eficiência de Memória:** Downloads paralelos usam buffers de stream para manter baixo uso de RAM, independente do tamanho do arquivo.
9. **Otimização de Disco:** Utiliza I/O com buffer para evitar alta ocupação do disco e travamentos do sistema durante transferências rápidas.
10. **Modo de Download Avançado (HTTP/HTTPS):** Downloads em chunks paralelos, suporta retomada.
11. **Suporte a Proxy:** HTTP, HTTPS, SOCKS5 com autenticação.
12. **Recursos de Otimização:** Compressão (para cache), cache de arquivos, limite de velocidade.
13. **Downloads de Torrent:** Adiciona links magnet ao `transmission-daemon` para download.
14. **Downloads FTP/SFTP:** Conecta a servidores FTP/SFTP para transferir arquivos.

## Funcionalidades

Veja a lista completa de funcionalidades e mudanças recentes no [CHANGELOG](CHANGELOG.md).

## KGet agora é uma Biblioteca também!
Se você quer usar o KGet como uma lib, clique [aqui](LIB.md).

### Recurso GUI opcional

A GUI é opcional e está disponível por meio de uma feature do Cargo chamada `gui`. Para compilar ou executar o binário com a GUI ativada:

```bash
cargo build --features gui
cargo run --features gui -- --gui
```

Se não ativar a feature `gui`, o binário e a biblioteca serão compilados sem as dependências da interface gráfica.

## Instalação

### Opção 1: Compilar da fonte (Recomendado para obter todas as funcionalidades)

Você precisará ter o Rust instalado. Se não tiver, instale-o em [rustup.rs](https://rustup.rs/).

Instale algumas dependências:
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
git clone https://github.com/davimf721/KGet.git
cd KGet
cargo build --release
```
O executável estará em `target/release/kget`. Você pode copiá-lo para um diretório no seu `PATH`:
```bash
sudo cp target/release/kget /usr/local/bin/
```

### Opção 2: Instalar via Cargo
Você pode instalar o binário publicado no crates.io (a GUI é opt-in via features):

```bash
# Instala o binário sem GUI (padrão)
cargo install kget

# Instala o binário com GUI (compila dependências GUI)
cargo install kget --features gui
```

Se encontrar problemas com a GUI ao instalar via `cargo install`, compilar da fonte é mais confiável.

### Opção 3: Baixar Binários Pré-compilados
Verifique a seção [Releases](https://github.com/davimf721/KGet/releases) para os binários mais recentes para seu SO.

#### macOS (bundle .app):
Baixe `KGet.app` ou o instalador `.dmg`, então:
1. Abra o DMG e arraste `KGet.app` para a pasta Aplicações (siga o guia visual)
2. Clique duas vezes para iniciar a GUI, ou use o CLI via `KGet.app/Contents/MacOS/kget`

O app nativo macOS inclui:
- Interface SwiftUI nativa com suporte a modo escuro
- Esquemas de URL (`kget://`, `magnet:`) para integração com navegadores
- Associações de arquivo `.torrent`
- Ações rápidas na barra de menu
- Suporte ao menu de Serviços do macOS
- Notificações nativas

#### Linux/macOS:
```bash
chmod +x kelpsget  # Tornar executável
./kelpsget [URL]   # Executar diretamente
```
#### Windows:
Execute o arquivo `.exe` diretamente.

### Requisito Adicional para Downloads de Torrent: Transmission Daemon

O KGet usa o `transmission-daemon` para gerenciar downloads de torrent.

**1. Instalar o Transmission Daemon:**
* **Debian/Ubuntu:**
     ```bash
     sudo apt update
     sudo apt install transmission-daemon
     ```
* **Fedora:**
     ```bash
     sudo dnf install transmission-daemon
     ```
* **Arch Linux:**
     ```bash
     sudo pacman -S transmission-cli
     ```

**2. Parar o Daemon para Configuração:**
```bash
sudo systemctl stop transmission-daemon
```

**3. Configurar o Transmission:**
Edite o arquivo `settings.json`. Localizações comuns:
* `/var/lib/transmission-daemon/info/settings.json` (Debian/Ubuntu, se instalado como serviço)
* `/var/lib/transmission/.config/transmission-daemon/settings.json` (Outro caminho comum, verifique seu sistema)
* `~/.config/transmission-daemon/settings.json` (se executado como usuário)

Use `sudo nano /var/lib/transmission-daemon/info/settings.json` (ou o caminho correto para seu sistema).

Encontre e modifique estas linhas:
```json
{
     // ...
     "rpc-authentication-required": true,
     "rpc-enabled": true,
     "rpc-password": "transmission", // Este é o valor que o KGet usa por padrão para conectar ao Transmission (recomendado)
     "rpc-port": 9091,
     "rpc-username": "transmission", // Nome de usuário que o KGet usa para conectar ao Transmission
     "rpc-whitelist-enabled": false, // Para acesso local. Para acesso remoto, configure IPs.
     "download-dir": "/var/lib/transmission-daemon/downloads", // Diretório padrão de download do Transmission
     // ...
}
```
**Importante:** Após salvar e iniciar o `transmission-daemon`, ele substituirá a senha em texto plano `rpc-password` por uma versão com hash.

**4. (Opcional) Ajustar Permissões do Usuário do Daemon:**
Se o `transmission-daemon` rodar como um usuário específico (ex: `debian-transmission` ou `transmission`), certifique-se que este usuário tem permissões de escrita nos diretórios de download que você pretende usar com o KelpsGet ou o próprio Transmission. Você pode adicionar seu usuário ao grupo do Transmission daemon:
```bash
sudo usermod -a -G debian-transmission seu_usuario_linux # Para Debian/Ubuntu
# Verifique o nome do grupo/usuário do Transmission no seu sistema
```

**5. Iniciar o Transmission Daemon:**
```bash
sudo systemctl start transmission-daemon
# Verificar status:
sudo systemctl status transmission-daemon
```
Acesse `http://localhost:9091` no seu navegador. Você deverá ver a interface web do Transmission e ser solicitado a fazer login com o `rpc-username` e `rpc-password` que você configurou.

## Uso

### Linha de Comando (CLI)
```bash
kelpsget [OPÇÕES] <URL>
```
**Exemplos:**
* **Download HTTP/HTTPS:**
     ```bash
     kelpsget https://example.com/arquivo.txt
     ```
* **Renomear Arquivo de Saída:**
     ```bash
     kelpsget -O novo_nome.txt https://example.com/arquivo.txt
     kelpsget -O ~/Downloads/ https://example.com/video.mp4 # Salva como ~/Downloads/video.mp4
     ```
* **Download FTP:**
     ```bash
     kelpsget ftp://usuario:senha@ftp.example.com/arquivo.zip
     kelpsget --ftp ftp://ftp.example.com/pub/arquivo.txt
     ```
* **Download SFTP:**
     (Requer configuração de chave SSH ou senha se o servidor permitir)
     ```bash
     kelpsget sftp://usuario@sftp.example.com/caminho/arquivo.dat
     kelpsget --sftp sftp://usuario@sftp.example.com/caminho/arquivo.dat -O local.dat
     ```
* **Download de Torrent (Link Magnet):**
     (Requer `transmission-daemon` configurado e rodando)
     ```bash
     kelpsget "magnet:?xt=urn:btih:SEU_HASH_AQUI&dn=NomeTorrent"
     kelpsget --torrent "magnet:?xt=urn:btih:SEU_HASH_AQUI" -O ~/MeusTorrents/
     ```
     O KelpsGet adicionará o torrent ao Transmission e tentará abrir a interface web (`http://localhost:9091`) para gerenciamento.

* **Modo Silencioso:**
     ```bash
     kelpsget -q https://example.com/arquivo.txt
     ```
* **Modo de Download Avançado (HTTP/HTTPS):**
     ```bash
     kelpsget -a https://example.com/arquivo_grande.zip
     ```
* **Usar Proxy:**
     ```bash
     kelpsget -p http://proxy:8080 https://example.com/arquivo.txt
     ```
* **Proxy com Autenticação:**
     ```bash
     kelpsget -p http://proxy:8080 --proxy-user usuario --proxy-pass senha https://example.com/arquivo.txt
     ```
* **Limite de Velocidade:**
     ```bash
     kelpsget -l 1048576 https://example.com/arquivo.txt  # Limita a 1MB/s
     ```
* **Desabilitar Compressão (específico do KelpsGet, não HTTP):**
     ```bash
     kelpsget --no-compress https://example.com/arquivo.txt
     ```
* **Desabilitar Cache (específico do KelpsGet):**
     ```bash
     kelpsget --no-cache https://example.com/arquivo.txt
     ```

### Interface Gráfica (GUI)
Para iniciar a GUI:
```bash
kelpsget --gui
```
A GUI permite que você insira a URL, caminho de saída e inicie downloads. Status e progresso são mostrados na interface.

## Configuração do KelpsGet
O KelpsGet usa um arquivo de configuração em:
- Windows: `%APPDATA%\kelpsget\config.json`
- Linux/macOS: `~/.config/kelpsget/config.json`

**Exemplo de `config.json` para o KelpsGet:**
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
     "key_path": null // Caminho para chave SSH privada, ex: "~/.ssh/id_rsa"
  }
}
```
**Nota sobre `cache_dir` e `key_path`:** Se usar `~`, certifique-se que seu programa expande corretamente o til para o diretório home do usuário, ou use caminhos absolutos.

## 🔗 Links Importantes
- 📚 [Documentação](https://davimf721.github.io/KelpsGet/)
- 📦 [crates.io](https://crates.io/crates/kelpsget)
- 💻 [GitHub](https://github.com/davimf721/KelpsGet)
- 📝 [Changelog](CHANGELOG.md)

## Você pode ver posts sobre o projeto em outras comunidades:
- [Dev.to](https://dev.to/davimf7221/kelpsget-v014-modern-download-manager-in-rust-4b9f)
- [r/rust](https://www.reddit.com/r/rust/comments/1kt69vh/after_5_months_of_development_i_finally_released/)
- [PitchHut](https://www.pitchhut.com/project/kelpsget)
- [Hacker News](https://hn.algolia.com/?query=Show%20HN%3A%20KelpsGet%20%E2%80%93%20Modern%20download%20manager%20built%20in%20Rust&type=story&dateRange=all&sort=byDate&storyText=false&prefix&page=0)

## Contribuindo
Quer contribuir? Confira nosso [guia de contribuição](CONTRIBUTING.md)!

Encontrou um bug ou quer adicionar uma funcionalidade? Abra uma issue ou envie um PR!

🚀 Faça downloads facilmente com a velocidade e confiabilidade do Rust. 🚀

## Licença
Este projeto está licenciado sob a Licença MIT - veja o arquivo [LICENSE](LICENSE) para detalhes.

