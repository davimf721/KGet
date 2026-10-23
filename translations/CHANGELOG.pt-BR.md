# Changelog

[English](../CHANGELOG.md) | [Português](CHANGELOG.pt-BR.md) | [Español](CHANGELOG.es.md)

Todas as mudanças notáveis neste projeto serão documentadas neste arquivo.

O formato é baseado em [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
e este projeto adere ao [Versionamento Semântico](https://semver.org/spec/v2.0.0.html).

## [1.8.0] - 2026-10-23

### Adicionado
- **Download de modelos de IA (`hf://`, `ollama://`):** `kget hf://org/repo[@rev][/caminho]` baixa um modelo,
  dataset (`hf://datasets/…`) ou space (`hf://spaces/…`) do Hugging Face, e URLs `huggingface.co/…/resolve|blob|tree/…`
  também funcionam. Filtre arquivos com globs em `--include`/`--exclude`, escolha branch, tag ou commit com
  `--revision` e use `--hf-cache` para gravar no cache padrão do hub, para que o `transformers` reaproveite os
  arquivos. Arquivos LFS são verificados pelo SHA-256. O token vem de `HF_TOKEN`, `HUGGING_FACE_HUB_TOKEN`, do
  arquivo de token do `hf auth login` ou do chaveiro (`hf://token@huggingface.co`).
  `kget ollama://llama3.2:3b` baixa um modelo para o diretório de modelos do Ollama (`OLLAMA_MODELS`), para que o
  `ollama run` o encontre, e verifica o digest de cada blob. Com `-O <dir>`, salva arquivos comuns. Os dois baixam
  `-j` arquivos por vez com o motor turbo, e um download interrompido pula os arquivos que já estão completos.
- **Daemon e fila em segundo plano:** `kget daemon` (`--foreground`, `status`, `stop`) roda um serviço local de
  downloads com fila persistente (`jobs.db`): jobs não terminados continuam após reiniciar. Gerencie com
  `kget queue add|list|status|cancel|pause|resume|rm|watch`, ou envie um único download com `kget <url> --via-daemon`.
  Os clientes falam JSON-RPC 2.0 delimitado por linhas num socket Unix (TCP de loopback com token no Windows).
  Configure na nova seção `daemon` do `config.json`.
- **Servidor MCP para agentes de IA (`kget mcp`):** o Claude Code e outros clientes MCP podem baixar, acompanhar o
  progresso, pausar, retomar, cancelar, pesquisar o histórico, verificar checksums e ler a configuração (9 ferramentas).
  Registre com `claude mcp add kget -- kget mcp`. As gravações ficam restritas a `--root` (padrão: o diretório de
  downloads do daemon). Usa o daemon quando há um rodando (`--no-daemon`, `--start-daemon`).
- **Interface de terminal (`kget tui`):** telas de fila, detalhes e histórico em tela cheia. Adicione URLs,
  pause/retome, cancele, tente de novo, remova, abra arquivos e pesquise ou filtre o histórico pelo teclado. Usa o
  daemon quando há um rodando (`--daemon` inicia um, `--local` roda os downloads no próprio processo).
- **Extensão de navegador (`browser-extension/`):** extensão Manifest V3 para Chrome, Chromium, Edge, Brave e
  Firefox. Adiciona o menu de contexto "Download with KGet", um popup com progresso ao vivo, interceptação opcional
  dos downloads do navegador e envio opcional de cookies. `kget native-host install|uninstall [--browser …]
  [--extension-id …]` registra a ponte de native messaging com o daemon.
- **Regras inteligentes, tags e duplicados:** regras no `config.json` organizam downloads em pastas (`{category}`,
  `{domain}`, `{ext}`, `{protocol}`, `{date:%Y-%m}`), adicionam tags e definem conexões, limite de velocidade,
  extração ou checksum obrigatório. As regras podem casar por domínio, regex, glob, extensão, categoria, tipo MIME,
  tamanho ou protocolo, e a primeira que casar vence. Há pastas por categoria embutidas e opcionais. Gerencie as
  regras com `--rules list|test|add|remove|enable|disable` e aplique-as com `--organize`/`--no-organize`.
  `--rules suggest` sugere regras a partir do histórico; com `--ai`, consulta o Claude e envia só contagens
  agregadas, depois da sua confirmação.
  `--tag` grava tags no histórico. `--duplicates skip|ask|download|link` decide o que acontece quando uma URL já foi
  baixada. `--dedupe` troca conteúdo idêntico por um hard link.
- **Senhas salvas no chaveiro do sistema:** Chaves do macOS, Gerenciador de Credenciais do Windows ou Secret
  Service no Linux/BSD (feature `keychain`, ativa por padrão, Rust puro). Senhas de FTP, SFTP (senha e frase da
  chave), WebDAV e proxy são buscadas quando a URL tem usuário mas não tem senha. O KGet pergunta no terminal e
  oferece salvar a senha. `--save-credentials` salva sem perguntar. Gerencie as senhas com
  `--credentials list|status|add|delete` (também o comando `credentials` no REPL). `KGET_KEYCHAIN=off` desativa.
- **Busca no histórico:** `--history` filtra por `--status`, `--domain`, `--type`, `--protocol`, `--since`/`--until`
  (`7d`, `12h`, `today`, datas), `--search` e `--tag`. Ordena com `--sort newest|oldest|largest|smallest|name|fastest`,
  pagina com `--history-limit`/`--history-offset` e imprime JSON com `--json` (JSON Lines com `--jsonl`). O comando
  `history` do REPL aceita os mesmos filtros. As entradas agora registram protocolo, duração, velocidade média,
  resultado do checksum e tags.
- **Seleção de arquivos e seed de torrents:** `--list-files` lista o conteúdo numerado de um magnet link ou arquivo
  `.torrent`. `--select 1,3-5` e `--select-glob "*.mkv"` baixam só uma parte. `--seed-ratio 1.5` e `--seed-time 2h`
  continuam semeando depois (padrões: `torrent.seed_ratio` / `torrent.seed_time_secs`). As estatísticas do swarm
  (peers, seeders, velocidades, ratio, trackers) chegam como eventos `torrent_stats` e aparecem na TUI, na GUI egui
  e no app macOS. Seleção, listagem de arquivos e tempo de seed exigem o cliente nativo (`torrent-native`); o
  Transmission suporta a seleção e o ratio de seed.
- **HTTP/2, HTTP/3 e mirrors em paralelo:** `--http-version auto|1.1|2|3` (ou `optimization.http_version`) para
  downloads turbo. `auto` negocia HTTP/2 via ALPN. O HTTP/3 é experimental e exige um build com a feature `http3` e
  `RUSTFLAGS="--cfg reqwest_unstable"`. Downloads Metalink buscam chunks de todos os mirrors ao mesmo tempo e
  descartam os mirrors que falham.
- **Eventos JSONL** (ainda `"v":1`): `phase` (`verifying` com `algorithm`, ou `extracting`), `rule`, `duplicate` e
  `torrent_stats`. Entradas de `files` trazem `selected` quando há uma seleção.
- **Biblioteca:** `jobs::JobManager` (fila com concorrência limitada, pausar/retomar, `JobEvent`s por assinante),
  `runner::{DownloadJob, RunContext, Reporter, run_job}`, `queue::{HistoryQuery, HistoryStore, HistorySort}`,
  `credentials`, `hf`, `ollama`, `fetch`, `rules`, `dedupe`, `daemon::client::DaemonClient`, `http::HttpVersion`,
  `AdvancedDownloader::{set_mirrors, set_http_version}`, os métodos do builder `.include()`, `.exclude()`,
  `.revision()`, `.hf_cache()` e `.parallel_files()`, e `config::set_config_dir_override`. `JsonEvent`/`JobEvent`
  também implementam `Deserialize`.
- **App macOS:** lê o histórico via `kget --history --json`, mostra o estado "extraindo" e o algoritmo do checksum
  a partir dos eventos `phase`, e mostra um resumo do swarm do torrent.
- **Modo interativo (`kget -i`) agora cobre a CLI:** todas as flags de download (`--checksum`, `--checksum-url`,
  `--extract`, `-c`, `--no-clobber`, `-H`, `--retries`, `--timeout`, `--at`, `-j`, protocolos forçados) e
  `-l`/`-p` por download, sem salvar. Novos comandos: `batch`, `info`, `retry`, `open`, `reveal`,
  `checksum`, `verify`, `extract`, `doctor`, `cd`/`pwd`, `set` (padrões da sessão), `history N|failed|completed|search|clear`,
  `config path|reset`, `version`, `help <tópico>`. Tab para completar, histórico salvo entre sessões,
  várias URLs por linha, e Ctrl+C para o download em andamento (mantendo o `.part`) em vez de ser ignorado.

### Alterado
- **Novo motor turbo:** as requisições de faixa agora rodam como tarefas tokio com reqwest assíncrono, em vez de um
  pool rayon de clientes bloqueantes. Uma conexão ociosa divide a faixa que terminaria por último (work stealing,
  até 512 KiB), e o HTTP/2 multiplexa as faixas em poucas conexões. O formato de retomada `.kget` não mudou.
- **O histórico fica em SQLite** (`history.db`, WAL, seguro com vários processos) em vez de `history.json`, e guarda
  as últimas 10.000 entradas. `--history` ganhou a coluna Size.
- Um caminho de download compartilhado (`kget::runner`) para a CLI, o modo batch, o REPL, a GUI egui, o daemon e o
  MCP. Credenciais salvas, eventos de fase e detalhes do histórico (duração, velocidade) funcionam igual em todos.
- `--save-config` guarda a senha do proxy no chaveiro do sistema. Ela continua nunca sendo gravada no `config.json`.
- No terminal, o KGet pergunta antes de baixar de novo uma URL cujo arquivo anterior ainda existe (escolha com
  `--duplicates`). Execuções não interativas baixam de novo, como antes.
- SIGTERM e SIGHUP param os downloads da CLI de forma limpa, como o Ctrl+C: os dados parciais ficam para retomar.
- `tokio` agora é uma dependência central. A feature `async` só controla os helpers assíncronos do builder.
- As features padrão agora são `keychain` e `tui`.

### Corrigido
- Retomar um download simples depois que o arquivo remoto mudou podia emendar bytes antigos e novos. O KGet agora
  salva os validadores em `<arquivo>.kget`, envia `If-Range` e reinicia quando o arquivo mudou.
- Um download simples que retoma um `.part` do modo turbo agora confia só no prefixo completo.
- Erros de escrita (disco cheio, cota, arquivo grande demais) eram repetidos com um offset dessincronizado. Agora o
  download falha na hora e mantém o arquivo parcial para `-c`.
- Um estado `.kget` corrompido ou inconsistente (chunks sobrepostos ou fora de ordem) é rejeitado em vez de usado.
- Quando um servidor responde à faixa de um chunk com `200` ou `416`, o KGet agora tenta outro mirror ou reinicia o
  download como fluxo único em vez de falhar.
- Retomar (`-c`) num servidor que ignora `Range` e reenvia o arquivo inteiro travava builds de debug
  e quebrava o limite de velocidade nos builds de release.
- `sanitize_filename` podia deixar pontos ou espaços no final depois de truncar (`. . a`).
- O parser de arquivos de checksum entrava em pânico com linhas como `) (` e aceitava hashes não hexadecimais.
- Um download com falha ou cancelado salvo com `-O arquivo` fica registrado com esse nome, para poder ser retomado.

### Segurança
- A extração de arquivos agora acontece num diretório temporário irmão e rejeita todos os links simbólicos antes de
  publicar arquivos, impedindo que um arquivo extraído redirecione um download posterior para fora do diretório permitido.
- URLs FTP rejeitam CR/LF em credenciais e caminhos decodificados antes que cheguem à camada de comandos FTP.
- `ProxyConfig` nunca serializa a senha do proxy e a oculta na saída de `Debug`. Senhas nunca aparecem no
  `config.json`, no histórico, nos logs ou nos eventos JSONL.
- Daemon: o socket Unix tem modo `0600` num diretório `0700`. O modo TCP só aceita loopback, com um token de 256 bits
  num arquivo acessível só ao dono. As requisições têm limite de 1 MiB, e cabeçalhos e URLs são validados. As saídas
  ficam dentro do diretório de downloads (sem `..`, fugas por symlink ou nomes aninhados), a menos que
  `daemon.allow_outside_download_dir` esteja ativo. Segredos são ocultados nas respostas e no `daemon.log`.
- MCP: toda gravação, e todo arquivo lido por `verify_checksum`, fica restrito a `--root`.
- Host nativo: confere a extensão chamadora em `allowed_origins.json`, repassa só nomes de arquivo e remove
  `Cookie`/`Authorization`/`Proxy-Authorization`, a menos que os cookies estejam ativados. Aceita só links
  http(s), ftp(s) e magnet, e `open` recusa executáveis.
- Tokens do Hugging Face nunca vão para os logs e são descartados em redirecionamentos para outro host.

### Desenvolvimento
- Workspace Cargo com `crates/kget-ffi`: bindings UniFFI 0.32 (Swift, Kotlin) sobre o `JobManager` (`KgetEngine`,
  listener de eventos). `scripts/gen-bindings.sh`, `scripts/build-xcframework.sh` e `scripts/swift-smoke-test.sh`
  geram e testam os bindings. `default-members = ["."]` mantém os comandos `cargo` simples no pacote `Kget`.
- Novas suítes de teste: `job_manager_tests`, `history_tests`, `robustness_tests`, `fuzz_regressions`, `model_tests`,
  `model_env_tests`, `smart_rules_tests`, `daemon_tests`, `mcp_tests`, `native_host_tests`, `bench_turbo` (benchmark
  ignorado por padrão), testes do engine do `kget-ffi` e a suíte `node --test` da extensão.
- `fuzz/`: crate cargo-fuzz com 12 alvos sobre `kget::fuzz_api`. A CI ganhou um job rápido de fuzz e um job FFI no
  macOS (bindings e um smoke test em Swift).
- Variáveis para testes: `KGET_HF_ENDPOINT`, `KGET_OLLAMA_REGISTRY`, `KGET_KEYCHAIN=mock`, `KGET_RUNTIME_DIR`,
  `KGET_DAEMON_TRANSPORT=tcp`, `KGET_NATIVE_HOST_DIR`, `KGET_ANTHROPIC_BASE_URL`.

### Notas de atualização
- **Histórico:** a primeira execução importa o `history.json` para o `history.db` e mantém o arquivo antigo como
  `history.json.bak`. Um banco ilegível é movido para `history.db.corrupt`. Binários `kget` antigos continuam lendo
  o `history.json` e não veem as entradas novas. O app macOS volta a ler o `history.json` com um binário antigo.
- **Configuração:** novos campos opcionais, todos com padrão, então arquivos existentes carregam sem mudanças:
  `rules_enabled` (`false`), `builtin_rules` (`true`), `organize_dir`, `rules`,
  `daemon.{download_dir, allow_outside_download_dir, max_concurrent}`, `optimization.http_version` e
  `torrent.{seed_ratio, seed_time_secs}`.
- **Biblioteca (pode quebrar):** `http::ClientOptions` tem o novo campo público `http_version`, então adicione
  `..Default::default()` aos literais. Matches exaustivos precisam de novos braços: `Protocol::{HuggingFace, Ollama}`,
  `JsonEvent::{Phase, Rule, Duplicate, TorrentStats}` e `WorkerToGuiMessage::TorrentStats`. `FileEntry` tem o novo
  campo `selected`. `app::JobId` agora é um re-export de `jobs::JobId` (continua `u64`).
- **Features:** `keychain` e `tui` vêm ativas por padrão (`--no-default-features` compila sem elas). `http3` é
  experimental e exige `RUSTFLAGS="--cfg reqwest_unstable"`. `async` agora só ativa `download_async()` /
  `download_all_async()`.
- **Consumidores de JSONL:** o protocolo continua `"v":1`. Ignore tipos de evento e campos que você não conhece.

## [1.7.1] - 2026-10-18

### Corrigido
- **O modo turbo podia gerar arquivos corrompidos em silêncio:** retomar um download paralelo interrompido
  (incluindo Cancelar → Tentar novamente no app macOS e as novas tentativas automáticas do `kget::builder`)
  tratava o arquivo pré-alocado como completo, e chunks cortados pelo servidor eram aceitos como completos.
  Agora os downloads usam `<arquivo>.part` + um estado de retomada `<arquivo>.kget` validado contra o servidor
  (tamanho, `ETag`, `Last-Modified`); todo chunk precisa estar completo e as novas tentativas continuam do byte exato.
  O número de conexões não é mais limitado pelo número de núcleos da CPU.
- **Flags da CLI eram gravadas para sempre no `config.json`** (`--proxy`, `--proxy-pass`, `-l`, `--no-cache`, flags de torrent).
  Agora valem só para a execução atual; use `--save-config` para gravá-las (a senha do proxy nunca é gravada).
  Se você usou `-l`/`--proxy` antes, rode `kget --save-config` sem elas ou apague o `config.json`.
- **Proxy HTTP não cobria downloads HTTPS e SOCKS5 não funcionava;** proxies inválidos eram ignorados em silêncio.
- **O app macOS não encontrava o yt-dlp instalado pelo Homebrew** quando aberto pelo Dock/Finder.
- **O app macOS marcava vídeos baixados como "Falhou"** e não mostrava progresso do yt-dlp.
- O yt-dlp podia travar quando escrevia muitos avisos (stderr nunca era lido); a mensagem de erro dele agora aparece.
- `--sha256` era ignorado no modo quiet/biblioteca e quando o servidor não suportava Range.
- O download simples não deixa mais um arquivo truncado com o nome final; corpos menores que o `Content-Length`
  são retomados com `Range`; HTTP 429/5xx respeitam `Retry-After`.
- Respostas brotli eram descomprimidas, alterando o arquivo salvo.
- `cargo test`, `examples/lib_usage.rs` e 10 doctests não compilavam.
- Rodar a suíte de testes alterava a configuração real do usuário (`KGET_CONFIG_DIR` agora é respeitado).
- O histórico podia ser apagado ou perder entradas com downloads simultâneos; credenciais nas URLs não são mais gravadas.
- Um `config.json` antigo ou parcial não impede mais o KGet de iniciar; o arquivo é gravado de forma atômica com permissão `0600`.
- URLs FTP/SFTP/WebDAV/Metalink/torrent são detectadas pelo esquema; caminhos e credenciais com `%xx` são decodificados;
  FTP funciona sem `SIZE`, tem timeouts e retomada; SFTP tem timeouts, usa `sftp.key_path`, aceita `/~/` e grava hosts novos no `known_hosts`.
- Manifestos Metalink 3.x agora são lidos; SHA-512/MD5 e `<size>` são verificados; `~` é expandido.
- Modo batch: concorrência limitada (`-j`), nomes únicos e todas as flags/protocolos respeitados.
- O `kget::builder` não repete mais 404/403, e a velocidade média é calculada corretamente.
- A saída JSONL não é mais misturada com texto e tem limite de frequência.
- O torrent nativo respeita `--torrent-port`, `--no-dht`, pode ser cancelado e o ETA está correto.
- Transmission: sem o limite de 30 minutos e sem abrir o navegador sem pedir.
- GUI egui: downloads simultâneos não compartilham mais progresso nem cancelamento.
- Modo interativo: `~` é expandido, caminhos com espaço entre aspas funcionam, colar uma URL inicia o download.
- macOS: a Share Extension não corta mais URLs com `&`; `webdav://` é aceito; o `kget` do Homebrew no Apple Silicon
  é encontrado; corrida de dados corrigida; saída lida por linha; uma URL concluída pode ser baixada de novo.

### Adicionado
- `-c/--continue`, `--no-clobber`, `--retries`, `--timeout`, `-j/--jobs`, `--batch -` (stdin), `--save-config`, `--doctor`.
- `--checksum <alg>:<hex>` e `--checksum-url <url>`; arquivos de checksum só com o hash.
- `KGET_YTDLP_PATH`, `ytdlp_path` no `config.json` e campo "yt-dlp path" + painel de diagnóstico nas configurações do app macOS.
- Pausar/Retomar no app macOS.
- Protocolo JSONL versionado (`"v":1`) para todos os protocolos; o app macOS passou a usá-lo.
- Arquivos `.torrent` (locais e URLs) com o cliente nativo.
- CI no GitHub Actions (Linux, macOS, Windows) e workflow de release que gera todos os binários, publica no crates.io e atualiza o tap do Homebrew.

### Removido / Obsoleto
- Código de cache/compressão não utilizado do `Optimizer` (obsoleto, sem efeito); `--no-cache` oculto.
- Dependências não usadas removidas; `tokio` só é compilado com as features `async`/torrent.

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
