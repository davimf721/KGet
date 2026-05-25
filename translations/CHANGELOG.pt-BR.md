# Changelog

[English](../CHANGELOG.md) | [Português](CHANGELOG.pt-BR.md) | [Español](CHANGELOG.es.md)

Todas as mudanças notáveis neste projeto serão documentadas neste arquivo.

O formato é baseado em [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
e este projeto adere ao [Versionamento Semântico](https://semver.org/spec/v2.0.0.html).

## [1.7.0] - 2026-05-24

### Adicionado
- **Download em lote (`--batch urls.txt`):** baixa múltiplos arquivos em paralelo a partir de um arquivo de texto simples — uma URL por linha, linhas iniciadas com `#` são ignoradas. Todas as URLs executam concorrentemente via threads do SO. Status `[OK]`/`[FAIL]` por URL; resumo ao final.
- **Aba de histórico no app macOS:** novo item "History" na sidebar lê o `history.json` persistente gerado pela CLI Rust. Exibe todos os downloads com data, tamanho e badge de status. Passe o cursor sobre uma linha para revelar o botão de re-download.
- **Drag-and-drop de URLs na janela macOS:** arraste qualquer link HTTP/HTTPS/FTP/magnet do Safari, Chrome ou qualquer app e solte na janela do KGet. Um overlay azul translúcido aparece durante o hover; ao soltar, a URL cai na barra de entrada pronta para iniciar.
- **Monitor de clipboard no app macOS:** o app monitora o clipboard a cada 1,5 s. Quando uma nova URL HTTP, HTTPS, FTP, SFTP ou magnet é detectada, um banner dispensável aparece com um botão "Download" de um clique. O banner é suprimido se a URL já estiver na lista atual.
- **Headers HTTP customizados (`-H "Nome: Valor"`):** passe uma ou mais flags `-H` para injetar headers arbitrários em downloads simples e turbo. Múltiplos headers podem ser empilhados. Funciona nos modos URL única, lote e interativo.
- **Sparkline de velocidade no app macOS:** cada linha de download ativo exibe um gráfico de velocidade em tempo real de 44×16pt que acumula as últimas 30 leituras de velocidade. Construído com SwiftUI `Path` + gradiente pelo novo componente `SparklineView`.
- **Auto-extração de arquivos:** `kget --extract` executa automaticamente `unzip`, `tar` ou `7z` no arquivo baixado quando a extensão é `.zip`, `.tar.gz`, `.tgz`, `.tar.bz2`, `.tar.xz` ou `.7z`. Controlável pela nova opção "Auto-extrair arquivos" nas Configurações macOS → Downloads.
- **Agendamento de download (`--at "HH:MM"`):** adia um download CLI para um horário local específico. O processo dorme até o minuto alvo ser atingido, então inicia o download.
- **Integração yt-dlp (`--ytdlp`, auto-detectado):** se `yt-dlp` (ou `youtube-dl`) estiver instalado, URLs do YouTube, Vimeo, Twitch, TikTok, Instagram, Bilibili e 10+ outras plataformas são automaticamente roteadas por ele. Qualidade selecionável via `--quality best|1080p|720p|480p|360p|audio`.
- **Suporte WebDAV (`webdav://`, `webdavs://`):** novo adaptador `WebDavDownloader` em `src/webdav/mod.rs` converte `webdav://` → `http://` e `webdavs://` → `https://`, extrai credenciais embutidas e injeta header `Authorization: Basic`. Detectado automaticamente pelo scheme da URL; flag explícita `--webdav` também disponível.
- **Share Extension (`Compartilhar > KGet`):** a Share Extension macOS agora está completa. `ShareViewController` codifica a URL compartilhada como `kget://download?url=<encoded>` e abre o app principal. Compilada e embutida em `KGet.app/Contents/PlugIns/KGetShareExtension.appex` pelo `build-native-macos.sh`.
- **Overhaul da API pública da biblioteca (`src/builder.rs`, `src/error.rs`, `src/events.rs`, `src/checksum.rs`):**
  - **Padrão Builder** — `kget::builder(url)` e `kget::batch([...])` substituem argumentos posicionais. Métodos fluentes: `.output()`, `.connections()`, `.speed_limit()`, `.proxy()`, `.sha256/sha512/sha1/md5/blake3()`, `.verify_from()`, `.header()`, `.retry()`, `.range()`, `.quiet()`.
  - **Erros tipados** — enum `KgetError` (`Network`, `Io`, `ChecksumMismatch`, `Protocol`, `Cancelled`, `NotFound`, `SidecarError`, `Other`) com impls `From` para `reqwest::Error`, `std::io::Error` e `Box<dyn Error>`.
  - **Canal de eventos** — `.spawn()` retorna `(JoinHandle, Receiver<DownloadEvent>)` com variantes `Progress { percent, speed_bps, eta_secs }`, `Status`, `Completed`, `Error`.
  - **Métricas de download** — struct `DownloadResult` com `path`, `bytes_downloaded`, `avg_speed_bps`, `duration`, `connections_used`, `checksums`.
  - **Download em memória** — `.download_to_bytes() -> Vec<u8>` e `.download_to_reader() -> impl Read`.
  - **Checksums multi-algoritmo** — SHA-256, SHA-512, SHA-1 (crate `sha1`), MD5 (crate `md-5`), BLAKE3 (crate `blake3`). Enum `ChecksumAlgorithm` + `compute_checksum()` em `src/checksum.rs`.
  - **Retry configurável** — `RetryConfig { max_attempts, backoff: Backoff::Exponential { base_ms, max_ms }, retry_on_status }`.
  - **Lote com controle de concorrência** — `BatchBuilder::concurrency(n)` usa pool de threads Rayon; retorna `Vec<BatchResult>`.
  - **API Async** — `DownloadBuilder::download_async()` e `BatchBuilder::download_all_async()` com `--features async`. Ambos usam `tokio::task::spawn_blocking`.

### Corrigido
- **`AdvancedDownloader::new()` entrava em panic em falha de build do HTTP client** — tipo de retorno alterado de `Self` para `Result<Self, …>` para o erro ser propagado em vez de causar crash.
- **Throttle paralelo era por thread** — com N conexões e limite de 1 MB/s, o throughput real era N×1 MB/s. Substituído por `Arc<Mutex<TokenBucket>>` global compartilhado entre todos os workers rayon; a taxa agregada agora é limitada corretamente.
- **`file.set_len(total_size)` acontecia antes de confirmar suporte a ranges** — se o servidor retornasse 200 em vez de 206, o arquivo era pré-alocado e então sobrescrito por um download completo, produzindo resultado corrompido. A pré-alocação agora ocorre apenas quando `supports_range` é confirmado.
- **Prompt de integridade ISO lia do stdin em contexto de biblioteca/automação** — `AdvancedDownloader` agora tem campo `ResumePolicy` (`Ask` / `AlwaysResume` / `AlwaysRestart`). Chamadores da biblioteca definem `AlwaysResume` para evitar bloqueio.
- **Tipo MIME ISO errado** — `"application/x-iso9001"` (norma ISO 9001) corrigido para `"application/x-iso9660-image"`.
- **`verify_file_sha256` imprimia no stdout incondicionalmente** — todas as chamadas `println!` removidas; mensagens agora enviadas apenas via callback opcional.
- **Downloader simples tentava novamente em 404/403/410** — erros 4xx permanentes agora falham imediatamente; apenas respostas 5xx transitórias e erros de conexão são tentados novamente.
- **`validate_filename` era insuficiente** — agora também rejeita: bytes nulos (`\0`), sequências de path traversal (`..`), nomes de arquivo maiores que 255 bytes e nomes de dispositivos reservados do Windows (CON, PRN, AUX, NUL, COM1–COM9, LPT1–LPT9) — case-insensitive, com ou sem extensão.
- **`sftp/mod.rs`: `CheckResult::Failure` continuava silenciosamente** — o caso de erro interno do libssh2 agora retorna erro grave e aborta a conexão em vez de contornar a verificação de host-key.

### Alterado
- **App macOS SwiftUI — redesign completo:** layout `NavigationSplitView` com sidebar recolhível para navegação por filtros (Todos / Ativos / Concluídos / Com Falha com badges de contagem ao vivo); barra de entrada URL limpa com toggle Turbo inline; barra de progresso fina de 3px com animação shimmer substituindo a barra multi-segmento anterior; linhas de download com status dot, type badges (Turbo / ISO / Torrent) e ícones de ação compactos.
- **GUI egui (Linux/Windows) — redesign completo:** paleta de cores inspirada no Apple iOS adaptativa ao sistema; sidebar esquerda (180px) com navegação Library e badges de contagem por categoria; botão de toggle escuro/claro; barra de progresso fina de 3px com shimmer; cards de download com status dot, type badges, botões de ação limpos; sombras nos cards; barra de status com estatísticas ao vivo.
- **Tema egui agora é adaptativo ao sistema:** lê a preferência de escuro/claro do SO na inicialização; botão de override manual na sidebar.

## [1.6.3] - 2026-05-21

### Adicionado
- **Suporte a Metalink (`.meta4` / `.metalink`):** `kget --metalink arquivo.meta4` analisa o manifesto RFC 5854, tenta mirrors em ordem de prioridade e verifica SHA-256 após o download. Funciona na CLI (`--metalink`) e no modo interativo (`download --metalink`). Auto-detectado pela extensão do arquivo.
- **Histórico persistente de downloads:** todo download via CLI e modo interativo é gravado em `history.json` no diretório de configuração do SO. Consulte com `kget --history`; limpe com `kget --history-clear` (ou `--history-clear completed`). Modo interativo ganha os comandos `history`, `history clear` e `history clear completed`.

## [1.6.3] - 2026-05-10

### Adicionado
- **Eventos JSONL experimentais na CLI:** `--jsonl` emite eventos `started`, `progress`, `status`, `completed` e `error` para agentes, scripts e frontends futuros.
- **Filtros na GUI:** o app macOS e a GUI Rust agora filtram downloads por todos, ativos, concluídos e falhos/cancelados.
- **Mais ações de download:** macOS e GUI Rust expõem Copiar URL, Abrir Arquivo, Abrir Pasta e Copiar SHA256 quando houver checksum.
- **SHA256 esperado na GUI Rust:** a GUI Linux/Windows pode enviar um hash SHA256 esperado ao worker de download.

### Alterado
- **Configurações do macOS agora afetam comportamento real:** modo avançado por padrão, notificações e limite de velocidade são persistidos e aplicados a novos downloads.
- **Versão do macOS corrigida:** metadados do app/extensão usam 1.6.3 e labels visíveis leem a versão do bundle em vez de strings hardcoded.
- **Limites de velocidade agora regulam downloads HTTP:** downloads HTTP simples e avançados respeitam o limite em bytes/s configurado.

### Corrigido
- **Fallback de metadados no download avançado:** quando `HEAD` falha ou não retorna `Content-Length`, o KGet tenta `Range: bytes=0-0` antes de desistir.
- **Progresso correto ao retomar:** downloads avançados inicializam o progresso com o tamanho parcial existente em vez de recomeçar visualmente do zero.
- **Modo JSONL não mistura mais linhas humanas de progresso avançado na saída de máquina.**

## [1.6.2] - 2026-04-28

### Corrigido
- **Downloads SFTP completamente não funcionais.** A implementação anterior passava a string `sftp://…` inteira para `TcpStream::connect` e a usava como caminho remoto, causando falha imediata em toda chamada SFTP. O módulo foi totalmente reescrito:
  - A URL é parseada corretamente para extrair `host`, `porta`, `usuário` e `caminho remoto`.
  - Autenticação em ordem de prioridade: senha na URL → SSH agent ativo → arquivos de chave padrão (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`, `~/.ssh/id_ecdsa`).
  - Arquivo transmitido em chunks de 32 KB com barra de progresso em tempo real.
  - Mensagens de erro claras e acionáveis em cada ponto de falha.
- **Login anônimo FTP falhava quando a URL não continha usuário.** `url.username()` da crate `url` retorna string vazia `""` (não `None`) quando a URL não tem segmento de usuário. Passar `""` para `ftp.login()` fazia servidores FTP anônimos rejeitarem a conexão. O downloader agora usa `"anonymous"` como fallback.

### Adicionado
- **Modo interativo completamente implementado.** Anteriormente `kget --interactive` abria um REPL que apenas imprimia `"Would download: …"` sem realizar nenhum download. O modo agora está completo:
  - Banner de entrada com arte ASCII em fonte de blocos Unicode.
  - Editor de linha `rustyline` com histórico de comandos persistente.
  - `download [flags] <url>` — aciona o downloader correto conforme os flags:
    - Padrão: HTTP/HTTPS simples com retry e barra de progresso.
    - `-a` / `--advanced` / `--turbo`: `AdvancedDownloader` (paralelo com byte range, retomável).
    - `--ftp`: downloader FTP com fallback anônimo.
    - `--sftp`: downloader SFTP com autenticação SSH multi-método.
    - `--torrent` ou prefixo `magnet:?` detectado automaticamente: motor de torrent nativo.
    - Flags `-o <caminho>`, `-q` (silencioso), `--sha256 <hash>` suportados.
  - `config [show | set <chave> <valor>]`: lê e persiste configurações (`connections`, `speed-limit`, `compression`, `cache`).
  - Comandos `clear`, `version`, `help` / `?`; `get` e `dl` como apelidos de `download`.
  - Erros são impressos e o REPL continua — um download com falha nunca encerra a sessão.

### Alterado
- **Tratamento de erros em locks Mutex no `AdvancedDownloader`:** todas as chamadas `.unwrap()` em `Mutex::lock()` foram substituídas por `.expect("…")` com mensagens descritivas.
- **Limpeza da API pública do `Optimizer`:** removidos atributos `#[allow(dead_code)]` dos métodos públicos `compress`, `get_cached_file` e `cache_file`.

## [1.6.1] - 2026-04-27

### Adicionado
- O app macOS agora valida magnet links antes de criar o card de download.
- Downloads concluídos ganharam ações Abrir Arquivo e Abrir Pasta.
- Menu de contexto nos cards do macOS: Copiar URL, Abrir Pasta, Reiniciar e Remover.
- Atalhos no app macOS: `Cmd+V`, `Cmd+L`, `Esc` e `Delete`.
- Verificação de SHA256 esperado via CLI `--sha256 <hash>` e pela biblioteca com `DownloadOptions::expected_sha256`.
- Helper público `verify_file_sha256` para usuários da biblioteca.
- Notificações nativas de conclusão e falha na GUI Rust para Linux e Windows via `notify-rust`.

### Alterado
- URLs ou magnets duplicados agora focam o card existente no app macOS em vez de criar outro.
- Downloads avançados respeitam o limite de conexões do otimizador e rejeitam respostas inválidas de byte range.
- Documentação da biblioteca atualizada em inglês, português e espanhol para a API atual.

### Corrigido
- Magnet links inválidos são recusados antes de acionar o backend torrent.
- Mismatch de SHA256 agora falha o download em vez de apenas imprimir o hash calculado.

## [1.6.0] - 2026-02-28

### Adicionado
- **App Nativo macOS (SwiftUI):** Aplicativo macOS nativo completamente redesenhado com integração profunda ao sistema.
  - Manipuladores de esquema de URL (`kget://`, `magnet:`)
  - Associações de arquivo (`.torrent`)
  - Integração com barra de menu com ações rápidas
  - Suporte ao menu de Serviços do macOS
  - Notificações nativas
  - Instalador DMG arrasta-e-solta com guia visual (caixas, seta, texto de instrução)
- **GUI Multiplataforma Melhorada:** Grande reformulação visual para a GUI baseada em egui (Linux/Windows).
  - Lista de downloads com rastreamento de múltiplos downloads simultâneos
  - Badge TURBO para modo de downloads paralelos
  - Badge ISO para arquivos ISO com verificação automática de integridade
  - Barra de progresso multi-segmento mostrando conexões paralelas (C1, C2, C3, C4)
  - Barra de progresso de verificação com tema roxo e animação de escudo
  - Indicador de conexões (⚡ 4x) para modo turbo
  - Exibição de velocidade e ETA em tempo real
  - Estado vazio com ícones de protocolo
  - Entrada de URL em linha única com controles integrados
  - Layout compacto com nomes de arquivos e URLs truncados
  - Dimensionamento e alinhamento adequados de botões
- **Melhorias Visuais:**
  - Tema escuro aprimorado com melhor contraste
  - Efeitos de brilho animados nas barras de progresso
  - Badges e ícones coloridos por status
  - Tipografia e espaçamento melhorados
  - Fundo do instalador DMG com tema escuro, caixas arredondadas, seta chevron e texto de instrução

### Alterado
- **Script de Build:** Agora fecha automaticamente instâncias do KGet em execução antes de compilar
- **Script de Build:** Compila o bundle do app em `/tmp` para evitar que atributos estendidos do iCloud interfiram na assinatura de código
- **Rastreamento de Progresso:** Removido limite artificial de 99%, agora mostra progresso preciso de 0-100%
- **Verificação SHA256:** Usa CommonCrypto nativo no macOS com progresso em tempo real
- **Progresso de Download Avançado:** Agora usa relatório de progresso via stdout em vez de monitoramento de tamanho de arquivo

### Corrigido
- Barra de progresso travando em 90% no modo de download avançado
- Barra de progresso "tremendo" (saltos erráticos) durante downloads avançados devido a conflito entre monitoramento de tamanho de arquivo e progresso via stdout
- Progresso de verificação não mostrando feedback até a conclusão
- Assinatura de código falhando no macOS devido ao iCloud adicionar atributos estendidos (`com.apple.FinderInfo`, `com.apple.provenance`)
- Ícones do instalador DMG desalinhados com as caixas de fundo

## [1.5.2] - 2025-12-19

### Adicionado
- **Manuseio Inteligente de ISO**: Detecção automática de arquivos `.iso` via URL e tipo MIME.
- **Prevenção de Corrupção**: Arquivos ISO agora ignoram camadas de descompressão/otimização para garantir integridade binária 1:1.
- **Verificação de Integridade**: Adicionada verificação opcional de checksum SHA256 ao final de downloads de ISO.

### Corrigido
- **Otimização de Memória e Disco**: Refatoração do `AdvancedDownloader` para usar escritas em stream com `BufWriter`, reduzindo drasticamente o uso de RAM e evitando problemas de 100% de tempo ativo do disco.
- **Confirmação de Verificação**: Corrigido bug onde a verificação de integridade rodava automaticamente no modo avançado; agora o programa solicita confirmação do usuário corretamente.
- **UI/UX**: Limpeza na saída do terminal durante downloads paralelos para uma experiência de barra de progresso mais fluida.
- Corrigido erro do compilador Rust `E0382` em relação à posse (ownership) do tipo `Mime` em `download.rs`.
- Melhorada a segurança de escrita de chunks paralelos para arquivos binários pesados.

## [1.5.1] - 2025-12-18

### Adicionado
- Feature opcional `gui` no Cargo para tornar as dependências da interface gráfica opcionais; compile com `--features gui` para habilitar o suporte a GUI.
- Funções de conveniência de alto nível: `kget::download(...)` e `kget::advanced_download(...)` para facilitar o uso como biblioteca.
- `create_progress_bar_factory(...)` exportado para permitir que consumidores criem barras de progresso `indicatif`.
- Exemplo `examples/lib_usage.rs` demonstrando o uso da biblioteca.
- Instruções de desenvolvimento Docker e integração `docker-compose` para simplificar a compilação, testes e contribuições.

### Alterado
- Atualizado README e `LIB.md` com instruções de uso da biblioteca e exemplos.
- `CONTRIBUTING.md` e traduções atualizadas com o fluxo de trabalho para contribuidores via Docker.
- Divisão do código GUI: adicionado o módulo `gui_types` para que builds CLI funcionem sem a feature de GUI.

### Corrigido / Diversos
- Pequenas correções na documentação e atualizações de tradução (PT-BR/ES).

## [1.5.0] - 2025-05-26

### Adicionado
- Novo crate público Rust: KGet agora pode ser usado como uma biblioteca em seus próprios projetos Rust clique [aqui](LIB.pt-br.md) para saber mais.
- Interface gráfica melhorada: fontes maiores, layout melhor e controles mais intuitivos.
- Integração com a área de transferência para fácil colagem de URLs.
- Botões de download e cancelamento agora sempre visíveis e funcionais na interface gráfica.
- **Modo interativo:** Execute `kget --interactive` para uma experiência tipo REPL com comandos como `download <url> [output]`, `help` e `exit`.

### Alterado
- Projeto renomeado de "KelpsGet" para "KGet" para simplicidade e consistência.
- Esquema de versionamento atualizado de 0.1.4 para 1.5.0 para permitir atualizações menores mais frequentes e rastreamento de versões mais claro.
- Lista de recursos movida do README para o CHANGELOG para manutenção mais fácil e manter o README conciso.

### Removido
- Seção de recursos redundantes ou excessivamente detalhados do README (agora veja o CHANGELOG para todos os recursos).

## [0.1.4] - 2025-05-22

### Adicionado
- Interface Gráfica do Usuário (GUI) para downloads mais fáceis.
- Suporte a download FTP.
- Suporte a download SFTP (autenticação por senha e chave).
- Suporte a download de torrent via links magnet (integra com o daemon Transmission).
- Instruções detalhadas para configuração do daemon Transmission no README.

### Alterado
- Refinado determinação do caminho de saída para alinhar comportamento com `wget`.
- Garantido que `final_path` seja sempre absoluto para evitar erros "Arquivo ou diretório não encontrado" no CWD.
- Atualizado README em inglês, português e espanhol para refletir todos os novos recursos e instruções de configuração.

### Corrigido
- Resolvido erro "Arquivo ou diretório não encontrado" ao baixar sem `-O` garantindo caminhos absolutos.
- Corrigido `validate_filename` para verificar apenas o nome base do arquivo, não o caminho completo.
- Resolvido problemas potenciais com `map_err` em `main.rs` para downloads de torrent e HTTP.

## [0.1.3] - 2025-03-11

### Adicionado
- Modo de download avançado com chunks paralelos e capacidade de retomada
- Suporte automático a compressão (gzip, brotli, lz4)
- Sistema de cache inteligente para downloads repetidos mais rápidos
- Limitação de velocidade e controle de conexão
- Suporte a documentação em múltiplos idiomas

### Alterado
- Melhorado tratamento de erros e feedback do usuário
- Aprimorada barra de progresso com informações mais detalhadas
- Otimizado uso de memória para downloads de arquivos grandes
- Atualizado sistema de configuração de proxy

### Corrigido
- Corrigido problemas de autenticação de proxy
- Resolvido problemas de criação de diretório de cache
- Corrigido tratamento de nível de compressão
- Corrigido tratamento de caminho de arquivo no Windows

### Segurança
- Adicionado tratamento seguro de conexão proxy
- Melhorada validação de URL
- Aprimorado sanitização de nome de arquivo
- Adicionado verificação de espaço antes dos downloads

## [0.1.2] - 2025-03-10

### Adicionado
- Suporte a proxy (HTTP, HTTPS, SOCKS5)
- Autenticação de proxy
- Nomeação personalizada de arquivo de saída
- Detecção de tipo MIME

### Alterado
- Melhorado cálculo de velocidade de download
- Aprimorado exibição da barra de progresso
- Melhores mensagens de erro
- Documentação atualizada

### Corrigido
- Corrigido problemas de timeout de conexão
- Resolvido problemas de permissão de arquivo
- Corrigido análise de URL
- Corrigido exibição da barra de progresso no Windows

## [0.1.1] - 2025-03-09

### Adicionado
- Modo silencioso para integração com scripts
- Barra de progresso básica
- Exibição do tamanho do arquivo
- Rastreamento de velocidade de download

### Alterado
- Melhorado tratamento de erros
- Aprimorada interface de linha de comando
- Melhor manipulação de arquivos
- Instruções de instalação atualizadas

### Corrigido
- Corrigido problemas de manipulação de caminho
- Resolvido problemas de permissão
- Corrigido exibição de progresso
- Corrigido comportamento de sobrescrita de arquivo

## [0.1.0] - 2025-03-08

### Adicionado
- Lançamento inicial
- Funcionalidade básica de download de arquivo
- Interface de linha de comando
- Tratamento básico de erros
- Suporte multiplataforma
