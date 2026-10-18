# KGet 1.7.1: relatório de auditoria completa

**Data:** 18/10/2026
**Base auditada:** `main` @ `59c9f84` (versão 1.7.0)
**Escopo:** biblioteca Rust (`src/`), binário CLI, GUI egui, app nativo macOS (`macos-app/`), testes, exemplos, documentação e empacotamento.
**Objetivo:** listar tudo o que precisa ser corrigido ou melhorado para a **v1.7.1**, com o código de cada correção e de cada feature nova.

Este é o único documento de referência da release. Ele **inclui por inteiro** o relatório separado *"yt-dlp não encontrado no app macOS"* (seção [P0-5](#p0-5-yt-dlp-não-encontrado-no-app-macos)), ampliado com o que a auditoria encontrou em volta.

---

## Índice

0. [Resumo executivo](#0-resumo-executivo)
1. [Como a auditoria foi feita e estado atual do build](#1-como-a-auditoria-foi-feita-e-estado-atual-do-build)
2. [P0: críticos (corrupção de dados, segurança, feature quebrada)](#2-p0-críticos)
3. [P1: alta prioridade](#3-p1-alta-prioridade)
4. [P2: média e baixa prioridade](#4-p2-média-e-baixa-prioridade)
5. [Features novas para a 1.7.1](#5-features-novas-para-a-171)
6. [Testes a adicionar](#6-testes-a-adicionar)
7. [Plano de release 1.7.1](#7-plano-de-release-171)
8. [Checklist geral](#8-checklist-geral)

---

## 0. Resumo executivo

| Severidade | Qtde | Destaques |
|---|---|---|
| **P0, crítico** | 10 | O modo turbo (`-a`) **grava arquivo corrompido e informa sucesso** em dois cenários reproduzidos. Flags da CLI **são gravadas permanentemente** no `config.json`, inclusive a senha do proxy. O proxy HTTP **não cobre downloads HTTPS** e o SOCKS5 não funciona. O yt-dlp não é encontrado no app macOS, e o vídeo baixado é marcado como "falhou". `cargo test` **não compila**. |
| **P1, alto** | 22 | Saída JSONL contaminada, GUI egui misturando downloads simultâneos, FTP/SFTP sem detecção automática por esquema, histórico que pode ser apagado inteiro, config antigo que impede o binário de iniciar, Metalink v3 anunciado e não suportado, várias falhas de parsing no app macOS. |
| **P2, médio/baixo** | 18 | Código morto (cache/compressão), dependências não usadas, `tokio` sempre compilado, ausência de CI, pequenos erros de formatação. |
| **Features 1.7.1** | 10 | Download via `.part` + estado de retomada, `-c/--continue`, `--no-clobber`, `--retries/--timeout`, caminho configurável do yt-dlp, eventos JSONL completos (com migração do app macOS), `--jobs` no batch, sidecar só com o hash, arquivos `.torrent`, CI no GitHub Actions. |

**Os 3 problemas mais graves, em uma frase cada:**

1. **Retomar um download turbo interrompido gera um arquivo de zeros com status "sucesso".** O arquivo é pré-alocado com o tamanho total, e na retomada esse tamanho é lido como "já baixado" (reproduzido; ver [P0-1](#p0-1-retomada-do-modo-turbo-gera-arquivo-de-zeros)).
2. **Uma conexão que fecha no meio de um chunk é aceita como chunk completo.** O arquivo fica com buracos e o download termina em `Ok(())` (reproduzido; ver [P0-2](#p0-2-chunk-truncado-é-aceito-como-completo)).
3. **Qualquer `kget --proxy … / -l … / --no-cache …` grava a opção para sempre em `config.json`.** No app macOS, isso deixa todos os downloads futuros limitados pela última velocidade configurada, e a suíte de testes deixa um proxy inexistente ligado na máquina do desenvolvedor ([P0-3](#p0-3-flags-da-cli-são-gravadas-para-sempre-no-configjson)).

---

## 1. Como a auditoria foi feita e estado atual do build

- Leitura completa de todos os módulos Rust (`main.rs`, `download.rs`, `advanced_download.rs`, `builder.rs`, `app.rs`, `config.rs`, `queue.rs`, `utils.rs`, `optimization.rs`, `checksum.rs`, `error.rs`, `ftp/`, `sftp/`, `torrent/`, `webdav/`, `metalink/`, `ytdlp/`, `interactive.rs` e as partes de estado e fluxo do `gui.rs`) e do app macOS (`DownloadManager.swift`, `KGetApp.swift`, `SettingsView.swift`, `ShareViewController.swift`).
- Verificação, no código-fonte do `reqwest 0.13.2`, da semântica de `timeout`, `Proxy::http` e SOCKS.
- Execução de `cargo build`, `cargo clippy --all-targets`, `cargo test` (todas as suítes e os doctests).
- **Reprodução dos bugs P0-1 e P0-2** com um teste `wiremock` temporário (o arquivo não foi commitado; a versão definitiva entra na [seção 6](#6-testes-a-adicionar)).

### 1.1 Resultado do build e dos testes (Linux, toolchain 1.97)

| Comando | Resultado |
|---|---|
| `cargo build` | ✅ compila |
| `cargo clippy --all-targets` | ❌ **falha**: `examples/lib_usage.rs` não compila (5 erros) e a lib gera 34 warnings. Com `-D warnings`, como em `run-tests.sh`, nada passa. |
| `cargo test` | ❌ **não compila**: o mesmo exemplo quebrado derruba o comando inteiro |
| `cargo test --lib --tests` | ✅ 136 testes passam (7 lib + 26 cli + 16 mock + 38 torrent + 49 unit) |
| `cargo test --doc` | ❌ **10 de 41 doctests falham** (`AdvancedDownloader::new` agora retorna `Result`, e falta `extra_headers` em `DownloadOptions`) |

**Efeito colateral encontrado ao rodar os testes:** depois de `cargo test`, o arquivo `~/.config/kget/config.json` da máquina ficou assim:

```json
"proxy": { "enabled": true, "url": "http://nonexistent-proxy:9999", ... },
"optimization": { ..., "max_connections": 8 }
```

O proxy veio de `tests/cli_tests.rs::test_cli_nonexistent_proxy` (gravado pelo bug P0-3), e o `max_connections: 8` veio do doctest de `src/config.rs`, que chama `config.save()` de verdade. **Rodar a suíte no Mac do desenvolvedor estraga a configuração real do KGet.**

---

## 2. P0: críticos

### P0-1. Retomada do modo turbo gera arquivo de zeros

**Arquivo:** `src/advanced_download.rs` (`download`, linhas 404-522, e `calculate_chunks`)

**Problema.** O tamanho já baixado é estimado pelo tamanho do arquivo em disco:

```rust
let existing_size = if Path::new(&self.output_path).exists() {
    Some(std::fs::metadata(&self.output_path)?.len())
```

No modo paralelo, porém, o arquivo é **pré-alocado** com o tamanho total logo no início (`file.set_len(total_size)`). Se o download for interrompido (Ctrl+C, queda de rede, **botão Cancelar do app macOS**, erro em um chunk), o arquivo já tem `total_size` bytes, quase todos zero. Na execução seguinte:

1. `existing_size == total_size`;
2. `calculate_chunks(total, Some(total))` devolve uma lista vazia;
3. nenhum byte é baixado, a checagem de tamanho passa e a função retorna `Ok(())`.

Mesmo sem pré-alocação, o tamanho do arquivo não indicaria o progresso, porque os chunks terminam fora de ordem.

**Reprodução (wiremock):** arquivo de 5 MiB pré-alocado com zeros e `AdvancedDownloader` executado: `result=Ok(()) identical=false`.

**Cenários reais:**
- App macOS: *Turbo* → *Cancelar* → *Tentar novamente* conclui na hora com um arquivo corrompido.
- `kget::builder(url).connections(4).download()`: o `run_with_retry` repete a tentativa depois de uma falha, e a segunda tentativa "retoma" o arquivo pré-alocado e devolve sucesso.
- Metalink: quando um espelho falha, o próximo espelho encontra o arquivo pré-alocado.

**Correção: arquivo `.part` + arquivo de estado `.kget`.**

O destino final só passa a existir quando o download termina. O progresso de cada chunk é registrado num JSON ao lado do arquivo:

```rust
// src/advanced_download.rs (novo bloco)
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChunkState {
    start: u64,
    end: u64,  // exclusivo
    done: u64, // bytes já gravados a partir de `start`
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResumeState {
    version: u32,
    url: String,
    total_size: u64,
    etag: Option<String>,
    last_modified: Option<String>,
    chunks: Vec<ChunkState>,
}

impl ResumeState {
    fn state_path(output: &str) -> PathBuf {
        PathBuf::from(format!("{output}.kget"))
    }

    fn part_path(output: &str) -> PathBuf {
        PathBuf::from(format!("{output}.part"))
    }

    fn load(output: &str) -> Option<Self> {
        let bytes = std::fs::read(Self::state_path(output)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Escrita atômica: tmp + rename, para nunca deixar JSON pela metade.
    fn save(&self, output: &str) -> std::io::Result<()> {
        let path = Self::state_path(output);
        let tmp = path.with_extension("kget.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(tmp, path)
    }

    fn remove(output: &str) {
        let _ = std::fs::remove_file(Self::state_path(output));
    }

    /// O estado só é reaproveitado se o recurso remoto for o mesmo.
    fn matches(&self, url: &str, remote: &RemoteInfo) -> bool {
        self.version == 1
            && self.url == url
            && self.total_size == remote.size
            && (self.etag.is_none() || self.etag == remote.etag)
            && (self.last_modified.is_none() || self.last_modified == remote.last_modified)
    }

    fn is_complete(&self) -> bool {
        self.chunks.iter().all(|c| c.start + c.done >= c.end)
    }
}

struct RemoteInfo {
    size: u64,
    accepts_ranges: bool,
    etag: Option<String>,
    last_modified: Option<String>,
}
```

Novo fluxo de `download()` (trecho principal):

```rust
pub fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
    let remote = self.probe_remote()?; // HEAD com fallback para GET bytes=0-0 (já existe; agora também lê ETag/Last-Modified)
    let part = ResumeState::part_path(&self.output_path);

    if let Some(parent) = part.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }

    if !remote.accepts_ranges {
        // Fluxo de stream único, gravando em .part (ver P0-8) e renomeando no final
        self.download_whole_to_part(&part)?;
        return self.finish(&part, remote.size);
    }

    // Reaproveita o estado anterior apenas se for compatível
    let mut state = match ResumeState::load(&self.output_path) {
        Some(s)
            if self.resume_policy != ResumePolicy::AlwaysRestart
                && part.exists()
                && s.matches(&self.url, &remote) =>
        {
            self.send_status("Resuming previous download");
            s
        }
        _ => {
            let f = File::create(&part)?;
            f.set_len(remote.size)?; // pré-alocação agora é segura: o estado diz o que falta
            ResumeState {
                version: 1,
                url: self.url.clone(),
                total_size: remote.size,
                etag: remote.etag.clone(),
                last_modified: remote.last_modified.clone(),
                chunks: self
                    .calculate_chunks(remote.size)
                    .into_iter()
                    .map(|(start, end)| ChunkState { start, end, done: 0 })
                    .collect(),
            }
        }
    };
    state.save(&self.output_path)?;

    let file = File::options().read(true).write(true).open(&part)?;
    let state = Arc::new(Mutex::new(state));
    self.download_chunks_parallel(&file, &state, &remote)?; // ver P0-2

    file.sync_all()?;
    let state = state.lock().unwrap();
    if !state.is_complete() {
        return Err("Download incomplete; run again to resume".into());
    }
    drop(file);
    self.finish(&part, remote.size)
}

/// Confere o tamanho, verifica o hash, renomeia .part para o destino e apaga o estado.
fn finish(&self, part: &Path, expected: u64) -> Result<(), Box<dyn Error + Send + Sync>> {
    let len = std::fs::metadata(part)?.len();
    if len != expected {
        return Err(format!("File size mismatch: expected {expected}, got {len}").into());
    }
    if let Some(expected_hash) = &self.expected_sha256 {
        crate::download::verify_file_sha256(part, Some(expected_hash), None)?;
    }
    std::fs::rename(part, &self.output_path)?;
    ResumeState::remove(&self.output_path);
    self.send_status("Advanced download completed successfully!");
    Ok(())
}
```

`calculate_chunks` passa a receber só `total_size`, porque a retomada agora é por chunk, e não por offset global.

> Compatibilidade: um arquivo final que já existe **sem** `.part`/`.kget` é tratado como "já baixado": pula o download se `--no-clobber` estiver ativo, caso contrário sobrescreve (ver [F-3](#f-3-flags---continue---no-clobber---retries---timeout)). Ele nunca mais é usado como ponto de retomada.

---

### P0-2. Chunk truncado é aceito como completo

**Arquivo:** `src/advanced_download.rs`, `download_chunks_parallel` (linhas 731-864)

**Problemas:**

1. Quando o servidor fecha a conexão antes do fim do chunk, `response.read()` devolve `0`, o laço faz `break` e o chunk retorna `Ok(())` **sem conferir `current_pos == end`**.
2. Um erro de leitura no meio do stream (`response.read(...)?`) sai com erro **sem nova tentativa**; o laço de retry só cobre o `send()`.
3. Respostas 403/429/500/503 não entram em nenhum ramo do `if`: o laço tenta de novo **imediatamente**, sem backoff e sem registrar o status. Já o ramo 416, que é um erro permanente, faz backoff à toa.
4. Com a pré-alocação, a checagem final de tamanho sempre passa, então o defeito não aparece.

**Reprodução (wiremock):** segundo chunk respondido com só 1000 de 1 048 576 bytes: `result=Ok(()) len=5242880 identical=false`.

**Correção.** Retomar a partir de `pos` a cada tentativa, separar erro transitório de erro fatal e exigir o chunk completo:

```rust
enum ChunkError {
    Transient(String),
    Fatal(String),
}

const READ_BUF: usize = 64 * 1024;

fn fetch_chunk(
    &self,
    idx: usize,
    file: &File,
    state: &Mutex<ResumeState>,
    remote: &RemoteInfo,
    on_bytes: &(dyn Fn(u64) + Sync),
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let (chunk_start, end, mut pos) = {
        let s = state.lock().unwrap();
        let c = &s.chunks[idx];
        (c.start, c.end, c.start + c.done)
    };
    let mut failures = 0u32;
    let mut last_persist = Instant::now();

    while pos < end {
        if self.is_cancelled() {
            return Err("Download cancelled".into());
        }

        let attempt: Result<(), ChunkError> = (|| {
            let mut req = self
                .apply_headers(self.client.get(&self.url))
                .header(reqwest::header::RANGE, format!("bytes={}-{}", pos, end - 1));
            // If-Range: se o arquivo mudou no servidor, ele responde 200 em vez de 206
            if let Some(v) = remote.etag.as_ref().or(remote.last_modified.as_ref()) {
                req = req.header(reqwest::header::IF_RANGE, v);
            }
            let mut resp = req.send().map_err(|e| ChunkError::Transient(e.to_string()))?;
            let status = resp.status();
            if status == reqwest::StatusCode::OK {
                return Err(ChunkError::Fatal(
                    "Server ignored the range or the remote file changed; restart the download".into(),
                ));
            }
            if status.as_u16() == 429 || status.is_server_error() {
                return Err(ChunkError::Transient(format!("HTTP {status}")));
            }
            if status != reqwest::StatusCode::PARTIAL_CONTENT {
                return Err(ChunkError::Fatal(format!("HTTP {status} for chunk {chunk_start}-{end}")));
            }

            let mut buf = vec![0u8; READ_BUF];
            while pos < end {
                if self.is_cancelled() {
                    return Err(ChunkError::Fatal("Download cancelled".into()));
                }
                let want = ((end - pos) as usize).min(buf.len());
                let n = resp
                    .read(&mut buf[..want])
                    .map_err(|e| ChunkError::Transient(e.to_string()))?;
                if n == 0 {
                    return Err(ChunkError::Transient(format!(
                        "connection closed early at byte {pos} (chunk ends at {end})"
                    )));
                }
                write_all_at(file, &buf[..n], pos).map_err(|e| ChunkError::Fatal(e.to_string()))?;
                pos += n as u64;
                failures = 0; // houve progresso, então zera o contador de falhas
                on_bytes(n as u64);

                // Persiste o progresso no máximo a cada 1 s
                if last_persist.elapsed() >= Duration::from_secs(1) {
                    let mut s = state.lock().unwrap();
                    s.chunks[idx].done = pos - chunk_start;
                    let _ = s.save(&self.output_path);
                    last_persist = Instant::now();
                }
            }
            Ok(())
        })();

        match attempt {
            Ok(()) => {}
            Err(ChunkError::Fatal(msg)) => return Err(msg.into()),
            Err(ChunkError::Transient(msg)) => {
                failures += 1;
                if failures > MAX_RETRIES as u32 {
                    return Err(format!("Chunk {chunk_start}-{end} failed after retries: {msg}").into());
                }
                std::thread::sleep(Duration::from_millis(500 * 2u64.pow(failures.min(6))));
            }
        }
    }

    let mut s = state.lock().unwrap();
    s.chunks[idx].done = end - chunk_start;
    let _ = s.save(&self.output_path);
    Ok(())
}

/// `write_at`/`seek_write` podem gravar menos bytes que o pedido; garante a gravação completa.
fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        #[cfg(unix)]
        let n = file.write_at(buf, offset)?;
        #[cfg(windows)]
        let n = file.seek_write(buf, offset)?;
        if n == 0 {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}
```

> Observação: o código atual usa `file.write_at(...)?` e ignora o número de bytes gravados. `write_at` **não garante** a gravação completa; o `write_all_at` acima corrige isso também.

**Bônus de desempenho no mesmo trecho:**
- **Pool dedicado.** Hoje `chunks.par_iter()` usa o pool global do rayon, que tem o tamanho do número de CPUs. Numa máquina de 4 núcleos, `connections = 16` vira 4 conexões (`parallelism = configured.min(rayon::current_num_threads())`). Downloads são I/O, não CPU. Use um pool próprio:

  ```rust
  let pool = rayon::ThreadPoolBuilder::new()
      .num_threads(self.optimizer.max_connections())
      .thread_name(|i| format!("kget-chunk-{i}"))
      .build()?;
  pool.install(|| pending.par_iter().try_for_each(|&idx| self.fetch_chunk(idx, &file, &state, &remote, &on_bytes)))?;
  ```

  Em `calculate_chunks`, remova o `.min(runtime_parallelism)`.
- **Contenção de mutex.** A barra de progresso e o callback são atualizados a cada 16 KiB, sempre sob `Mutex`. Troque por um `AtomicU64` de bytes e por uma única thread "ticker" que, a cada 100-200 ms, atualiza a barra, chama o callback e imprime `PROGRESS:`. Ver também [P1-1](#p1-1-saída-jsonl-contaminada-e-inundada).

---

### P0-3. Flags da CLI são gravadas para sempre no `config.json`

**Arquivo:** `src/main.rs`, linhas 256-294

**Problema.** No modo CLI, `main()` aplica `--proxy`, `--proxy-user`, `--proxy-pass`, `--proxy-type`, `-l/--limit`, `--no-cache`, `--torrent`, `--max-peers`, `--max-seeds`, `--torrent-port` e `--no-dht` em `config` e depois chama **`config.save()?`**. Consequências:

- `kget -l 100000 URL` deixa **todos os downloads seguintes** limitados a 100 KB/s, sem aviso.
- **App macOS:** `buildArguments` passa `-l <bytes>` quando o usuário define um limite. Se depois ele apagar o campo, o app deixa de passar `-l`, mas o valor antigo continua no `config.json` e **o limite nunca sai**.
- `--proxy-pass` grava a **senha do proxy em texto puro** em `config.json`.
- `--proxy-type` tem `default_value = "http"`, então **qualquer execução** sobrescreve o tipo de proxy salvo (por exemplo, `Socks5` volta a ser `Http`).
- A suíte de testes polui a configuração real do desenvolvedor (seção 1.1).
- Um erro de escrita (diretório sem permissão, disco cheio) **aborta o download**, por causa do `?`.

**Correção.** As flags sobrepõem a configuração **só em memória**. Para gravar, use um comando explícito.

```rust
// src/main.rs
#[arg(long = "proxy-type")]
proxy_type: Option<String>,          // antes: String com default "http"

/// Persist the given options to config.json
#[arg(long = "save-config")]
save_config: bool,
```

```rust
// substitui o bloco `if !should_start_gui { ... config.save()?; }`
if !should_start_gui {
    apply_cli_overrides(&mut config, &args);
    if args.save_config {
        // Nunca grava a senha do proxy
        let mut to_save = config.clone();
        to_save.proxy.password = None;
        to_save.save()?;
        println!("Configuration saved to {}", Config::path()?.display());
    }
}

fn apply_cli_overrides(config: &mut Config, args: &Args) {
    if let Some(url) = &args.proxy {
        config.proxy.enabled = true;
        config.proxy.url = Some(url.clone());
        // Tipo inferido do esquema quando --proxy-type não for informado
        config.proxy.proxy_type = match args.proxy_type.as_deref() {
            Some(t) => ProxyType::parse(t),
            None => ProxyType::from_url(url),
        };
    } else if let Some(t) = args.proxy_type.as_deref() {
        config.proxy.proxy_type = ProxyType::parse(t);
    }
    if let Some(u) = &args.proxy_user { config.proxy.username = Some(u.clone()); }
    if let Some(p) = &args.proxy_pass { config.proxy.password = Some(p.clone()); }
    if let Some(l) = args.speed_limit { config.optimization.speed_limit = (l > 0).then_some(l); }
    if args.no_cache { config.optimization.cache_enabled = false; }
    if args.torrent { config.torrent.enabled = true; }
    if let Some(v) = args.max_peers { config.torrent.max_peers = v; }
    if let Some(v) = args.max_seeds { config.torrent.max_seeds = v; }
    if let Some(v) = args.torrent_port { config.torrent.port = Some(v); }
    if args.no_dht { config.torrent.dht_enabled = false; }
}
```

```rust
// src/config.rs
impl ProxyType {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "https" => ProxyType::Https,
            "socks5" | "socks5h" | "socks" => ProxyType::Socks5,
            _ => ProxyType::Http,
        }
    }
    pub fn from_url(url: &str) -> Self {
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("socks") { ProxyType::Socks5 }
        else if lower.starts_with("https://") { ProxyType::Https }
        else { ProxyType::Http }
    }
}
```

**Migração para quem já tem o config "contaminado".** Na 1.7.1, o app macOS deve **sempre** passar o limite, inclusive `-l 0` (= sem limite). Assim, o valor antigo deixa de ter efeito mesmo sem limpar o arquivo:

```swift
// DownloadManager.swift, buildArguments
args.append(contentsOf: ["-l", "\(speedLimitBytesPerSecond() ?? 0)"])
```

Mencione no CHANGELOG que quem usou `-l`/`--proxy` antes pode rodar `kget --save-config` sem as flags para limpar o arquivo, ou apagar `config.json`.

---

### P0-4. Proxy: HTTPS não passa pelo proxy HTTP, SOCKS5 não funciona e erros são ignorados

**Arquivos:** `src/download.rs:171-185`, `src/advanced_download.rs:231-248`, `src/metalink/mod.rs:384-408`, `src/builder.rs:686-727`

**Problemas:**

1. `ProxyType::Http` usa `reqwest::Proxy::http(url)`, que **só intercepta URLs `http://`**. Como `--proxy-type` vale `http` por padrão, `kget --proxy http://corp:3128 https://site/arquivo.iso` **baixa direto, sem proxy**. Em rede corporativa isso falha; em cenário de privacidade, vaza o IP.
2. `ProxyType::Https` usa `Proxy::https`, que só intercepta URLs `https://`. É o inverso do que o nome sugere.
3. O `reqwest` está compilado **sem a feature `socks`**, então `socks5://…` dá "unknown proxy scheme".
4. Todos os pontos fazem `if let Ok(proxy) = proxy_client { ... }`, ou seja, **um proxy inválido é ignorado em silêncio** e o download sai sem proxy.
5. A mesma lógica está copiada em 4 lugares, e o `builder.rs` usa `Proxy::all`, diferente dos demais.

**Correção: um único construtor de client, em `src/http.rs`.**

```toml
# Cargo.toml
reqwest = { version = "0.13.2", default-features = false, features = [
    "blocking", "json", "stream", "hickory-dns", "default-tls", "socks",
] }
# "gzip", "brotli" e "deflate" saem: downloads de arquivo nunca devem ser descomprimidos (ver P1-12)
```

```rust
// src/http.rs (novo)
use crate::config::{ProxyConfig, ProxyType};
use crate::error::KgetError;
use reqwest::blocking::Client;
use std::time::Duration;

pub const USER_AGENT: &str = concat!("KGet/", env!("CARGO_PKG_VERSION"));

pub struct ClientOptions {
    pub connect_timeout: Duration,
    /// Timeout por operação (envio e cada leitura), não total
    pub io_timeout: Option<Duration>,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self { connect_timeout: Duration::from_secs(20), io_timeout: Some(Duration::from_secs(60)) }
    }
}

pub fn build_client(proxy: &ProxyConfig, opts: &ClientOptions) -> Result<Client, KgetError> {
    let mut b = Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(opts.connect_timeout)
        .timeout(opts.io_timeout);

    if proxy.enabled {
        let raw = proxy
            .url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
            .ok_or_else(|| KgetError::Protocol("Proxy is enabled but no URL is set".into()))?;
        let url = normalize_proxy_url(raw, &proxy.proxy_type);
        // Proxy::all cobre http:// e https://; o *tipo* do proxy está no esquema da URL dele
        let mut p = reqwest::Proxy::all(&url)
            .map_err(|e| KgetError::Protocol(format!("Invalid proxy '{url}': {e}")))?;
        if let (Some(u), Some(pw)) = (&proxy.username, &proxy.password) {
            p = p.basic_auth(u, pw);
        }
        b = b.proxy(p);
    }
    b.build().map_err(|e| KgetError::Protocol(format!("Failed to create HTTP client: {e}")))
}

fn normalize_proxy_url(url: &str, kind: &ProxyType) -> String {
    if url.contains("://") {
        return url.to_string();
    }
    match kind {
        ProxyType::Http => format!("http://{url}"),
        ProxyType::Https => format!("https://{url}"),
        ProxyType::Socks5 => format!("socks5h://{url}"), // DNS resolvido pelo proxy
    }
}
```

Depois, troque os quatro blocos duplicados por `crate::http::build_client(&proxy, &ClientOptions::default())?`. Atualize a documentação de `ProxyType` em `config.rs`: `Http` = proxy que fala HTTP, `Https` = proxy que fala TLS, `Socks5` = SOCKS5. Os três cobrem downloads HTTP e HTTPS.

---

### P0-5. yt-dlp não encontrado no app macOS

> Esta seção reúne **todo o conteúdo do relatório "KGet — yt-dlp não encontrado no app macOS" (18/10/2026)**, com os complementos da auditoria marcados como *[auditoria]*.

**Componentes afetados:** `macos-app/KGet/DownloadManager.swift`, `src/ytdlp/mod.rs`, `src/main.rs`
**Severidade:** alta. Downloads de vídeo (YouTube, Vimeo etc.) falham sempre que o app é aberto pelo Dock, pelo Finder ou pelo Spotlight.

#### 5.1 Sintoma

Ao baixar uma URL de vídeo pelo app macOS, o download falha com:

```
yt-dlp is not installed. Install with:
  brew install yt-dlp
```

O erro aparece mesmo com o `yt-dlp` instalado (`which yt-dlp` no terminal retorna o caminho normalmente). Pela CLI (`kget <url>` no terminal), o mesmo download funciona.

#### 5.2 Causa raiz

Apps GUI do macOS são iniciados pelo `launchd` e **não herdam o ambiente do shell** (`.zshrc`, `.zprofile`). O `PATH` recebido costuma ser apenas:

```
/usr/bin:/bin:/usr/sbin:/sbin
```

O Homebrew instala binários em `/opt/homebrew/bin` (Apple Silicon) ou em `/usr/local/bin` (Intel). Nenhum dos dois está nesse PATH.

A cadeia que leva à falha:

1. **`DownloadManager.swift` → `buildEnvironment()`** repassa `ProcessInfo.processInfo.environment` ao processo `kget-bin` sem acrescentar nada ao `PATH`.
2. **`src/ytdlp/mod.rs` → `ytdlp_binary()`** procura o binário com `which yt-dlp` e depois tenta executar `yt-dlp --version`. As duas buscas usam o `PATH` reduzido e falham.
3. **`src/main.rs` (≈ linha 478)** recebe `None` e emite o erro "yt-dlp is not installed".

Pelo terminal, o processo herda o `PATH` completo do shell, e por isso a CLI funciona.

*[auditoria]* Outros pontos na mesma cadeia:
- `which` não existe no Windows, e em algumas imagens Linux mínimas também não. A busca deve percorrer o `PATH` em Rust, sem depender de um processo externo.
- `ytdlp_binary()` é chamado **3 vezes** por download (`ytdlp_available()`, a mensagem do `println!` e `download_video`), e cada chamada dispara até 4 processos. Vale guardar o resultado em cache.
- A mensagem de erro em `main.rs:478` faz `ytdlp_binary().unwrap_or_else(|| "yt-dlp")` justamente quando o binário **não** foi encontrado, então sempre imprime `(looking for: yt-dlp)`, o que não ajuda no diagnóstico.

#### 5.3 Correção

São duas mudanças independentes, e as duas são recomendadas: a primeira resolve o app, e a segunda deixa o binário Rust robusto em qualquer contexto de execução (launchd, cron, outros front-ends).

##### 5.3.1 Swift: completar o PATH do processo filho

`macos-app/KGet/DownloadManager.swift`

```swift
private func buildEnvironment() -> [String: String] {
    var env = ProcessInfo.processInfo.environment

    // Apps GUI recebem um PATH mínimo do launchd; incluir locais comuns
    // de instalação (Homebrew Apple Silicon/Intel, MacPorts, pipx/pip --user).
    let extraPaths = [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/opt/local/bin",                      // [auditoria] MacPorts
        NSHomeDirectory() + "/.local/bin",
    ]
    let currentPath = env["PATH"] ?? "/usr/bin:/bin:/usr/sbin:/sbin"
    let existing = Set(currentPath.split(separator: ":").map(String.init))
    let missing = extraPaths.filter { !existing.contains($0) }
    env["PATH"] = (missing + [currentPath]).joined(separator: ":")

    // [auditoria] Caminho escolhido pelo usuário (seção 5.3.3) tem prioridade
    let customYtdlp = ytdlpPath.trimmingCharacters(in: .whitespacesAndNewlines)
    if !customYtdlp.isEmpty {
        env["KGET_YTDLP_PATH"] = (customYtdlp as NSString).expandingTildeInPath
    }

    env["LANG"] = "en_US.UTF-8"
    env["TERM"] = "xterm-256color"
    return env
}
```

##### 5.3.2 Rust: fallback com caminhos absolutos

Versão do relatório original (funciona e pode entrar como está):

```rust
/// Diretórios onde yt-dlp costuma estar instalado, mesmo fora do PATH.
fn fallback_dirs() -> Vec<String> {
    let mut dirs = vec![
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
    ];
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(format!("{home}/.local/bin"));
    }
    dirs
}

/// Returns the yt-dlp binary (name in PATH or absolute path), or `None`.
pub fn ytdlp_binary() -> Option<String> {
    // 1. Busca via PATH (comportamento atual)
    for candidate in &["yt-dlp", "youtube-dl"] {
        let ok = Command::new("which")
            .arg(candidate)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Some(candidate.to_string());
        }
    }
    for candidate in &["yt-dlp", "youtube-dl"] {
        let ok = Command::new(candidate)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Some(candidate.to_string());
        }
    }

    // 2. Fallback: caminhos absolutos conhecidos
    for dir in fallback_dirs() {
        for name in &["yt-dlp", "youtube-dl"] {
            let path = std::path::Path::new(&dir).join(name);
            if path.is_file() {
                return Some(path.to_string_lossy().into_owned());
            }
        }
    }
    None
}
```

`ytdlp_binary()` já é usado tanto na detecção quanto na execução (`main.rs`, ≈ linha 491). Quando ele devolve o caminho absoluto, o download funciona sem outras mudanças.

*[auditoria]* **Versão recomendada para a 1.7.1.** Ela é multiplataforma, não executa nenhum processo, guarda o resultado em cache, aceita `KGET_YTDLP_PATH` e expõe os diretórios pesquisados para a mensagem de erro:

```rust
// src/ytdlp/mod.rs
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Variável de ambiente com o caminho explícito do yt-dlp (prioridade máxima).
pub const YTDLP_PATH_ENV: &str = "KGET_YTDLP_PATH";

const CANDIDATES: &[&str] = if cfg!(windows) {
    &["yt-dlp.exe", "youtube-dl.exe"]
} else {
    &["yt-dlp", "youtube-dl"]
};

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Diretórios pesquisados, em ordem: PATH, e depois locais comuns fora do PATH.
pub fn ytdlp_search_dirs() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();

    for d in ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"] {
        out.push(PathBuf::from(d));
    }
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".local/bin")); // pipx / pip --user (Linux e macOS)
        // pip --user no macOS: ~/Library/Python/3.x/bin
        if let Ok(rd) = std::fs::read_dir(home.join("Library/Python")) {
            for e in rd.flatten() {
                out.push(e.path().join("bin"));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|d| seen.insert(d.clone()));
    out
}

fn locate() -> Option<String> {
    if let Some(p) = std::env::var_os(YTDLP_PATH_ENV) {
        let p = PathBuf::from(p);
        if is_executable(&p) {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    ytdlp_search_dirs().into_iter().find_map(|dir| {
        CANDIDATES
            .iter()
            .map(|name| dir.join(name))
            .find(|c| is_executable(c))
            .map(|c| c.to_string_lossy().into_owned())
    })
}

/// Returns the absolute path of yt-dlp (or youtube-dl), or `None`.
pub fn ytdlp_binary() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE.get_or_init(locate).clone()
}

/// Mensagem de erro com os locais pesquisados (seção 5.3.4).
pub fn ytdlp_not_found_message() -> String {
    let extra = ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin", "~/.local/bin"];
    format!(
        "yt-dlp not found. Searched PATH and: {}\n\
         Install with: brew install yt-dlp  (or: pipx install yt-dlp)\n\
         Or set {YTDLP_PATH_ENV}=/path/to/yt-dlp",
        extra.join(", ")
    )
}
```

E em `src/main.rs`:

```rust
if use_ytdlp && ytdlp_binary().is_none() {
    let msg = kget::ytdlp::ytdlp_not_found_message();
    if args.jsonl {
        emit_jsonl(json!({ "event": "error", "code": "ytdlp_not_found", "message": msg }));
    } else {
        eprintln!("{msg}");
    }
    return Err("yt-dlp not found".into());
}
```

##### 5.3.3 (Opcional → recomendado) Configuração pelo usuário

Adicionar em `SettingsView.swift`, na seção "Video Downloads (yt-dlp)", um campo **"Caminho do yt-dlp"**, repassado ao binário pela variável de ambiente `KGET_YTDLP_PATH`, com prioridade sobre a busca automática. Isso cobre instalações em venv, conda e outros locais fora do padrão.

*[auditoria]* Código:

```swift
// DownloadManager.swift
@Published var ytdlpPath = ""

// saveSettings()
UserDefaults.standard.set(ytdlpPath, forKey: "ytdlpPath")
// loadSettings()
ytdlpPath = UserDefaults.standard.string(forKey: "ytdlpPath") ?? ""
```

```swift
// SettingsView.swift, dentro de GlassSection(header: "Video Downloads (yt-dlp)")
@State private var ytdlpPath: String = ""
// .onAppear: ytdlpPath = downloadManager.ytdlpPath
// .onChange(of: ytdlpPath) { v in downloadManager.ytdlpPath = v; downloadManager.saveSettings() }

GlassRow(icon: "terminal.fill", iconColor: .gray, title: "yt-dlp path",
         subtitle: "Leave empty to auto-detect (Homebrew, MacPorts, pipx)") {
    HStack {
        TextField("/opt/homebrew/bin/yt-dlp", text: $ytdlpPath)
            .textFieldStyle(.roundedBorder)
            .frame(width: 220)
        Button("Choose…") {
            let panel = NSOpenPanel()
            panel.canChooseFiles = true
            panel.canChooseDirectories = false
            panel.allowsMultipleSelection = false
            if panel.runModal() == .OK, let url = panel.url { ytdlpPath = url.path }
        }
    }
}
```

##### 5.3.4 (Opcional → recomendado) Mensagem de erro mais útil

Incluir na mensagem os diretórios onde a busca foi feita ajuda no diagnóstico:

```
yt-dlp não encontrado. Procurado no PATH e em:
  /opt/homebrew/bin, /usr/local/bin, ~/.local/bin
Instale com: brew install yt-dlp
```

(Implementado por `ytdlp_not_found_message()` acima.)

#### 5.4 Como testar

1. Instalar via Homebrew: `brew install yt-dlp`. Confirmar com `which yt-dlp` (esperado: `/opt/homebrew/bin/yt-dlp`).
2. Compilar o binário (`cargo build --release`) e o app.
3. **Abrir o KGet pelo Dock/Finder** (não pelo terminal). É esse cenário que reproduz o bug.
4. Colar uma URL do YouTube e baixar. Resultado esperado: o download começa sem erro.
5. Regressão pela CLI: `kget "<url do youtube>" --ytdlp -O ~/Downloads` continua funcionando.
6. Regressão com o yt-dlp ausente (`brew uninstall yt-dlp`): a mensagem de erro continua aparecendo corretamente.
7. Se possível, testar num Mac Intel (`/usr/local/bin`) e com instalação via `pipx` (`~/.local/bin`).
8. *[auditoria]* Testar com `KGET_YTDLP_PATH` apontando para um yt-dlp dentro de um venv.
9. *[auditoria]* Confirmar que, depois do download, o item fica **"Concluído"** (e não "Falhou") e que *Abrir arquivo* abre o vídeo certo (depende do [P0-6](#p0-6-yt-dlp-deadlock-no-stderr-sem-progresso-e-vídeo-marcado-como-falha)).

#### 5.5 Contorno até a atualização

Abrir o app pelo terminal, para que ele herde o PATH do shell:

```bash
/Applications/KGet.app/Contents/MacOS/KGet
```

#### 5.6 Checklist (original + auditoria)

- [ ] `buildEnvironment()` acrescenta Homebrew, MacPorts e `~/.local/bin` ao PATH
- [ ] `ytdlp_binary()` com fallback de caminhos absolutos (versão sem `which`, com cache)
- [ ] Campo de caminho personalizado nas configurações (`KGET_YTDLP_PATH`)
- [ ] Mensagem de erro com os diretórios pesquisados
- [ ] Testado abrindo o app pelo Dock/Finder (Apple Silicon e, se possível, Intel)
- [ ] Nota no changelog: "Corrigido: app macOS não encontrava yt-dlp instalado via Homebrew"

---

### P0-6. yt-dlp: deadlock no stderr, sem progresso e vídeo marcado como falha

**Arquivos:** `src/ytdlp/mod.rs` (`download_video`), `src/main.rs:485-499`, `DownloadManager.swift` (`finalizeDownload`, `buildArguments`)

**Problemas:**

1. **Deadlock possível.** `download_video` faz `stderr(Stdio::piped())` mas **nunca lê o stderr**. Quando o yt-dlp escreve mais de cerca de 64 KB de avisos (é comum: warnings de formato, nsig, retries), o buffer do pipe enche, o yt-dlp trava no `write` e o KGet fica parado esperando o stdout para sempre.
2. **A mensagem de erro do yt-dlp se perde.** Em caso de falha, só aparece `yt-dlp exited with code 1`.
3. **Sem progresso no app macOS e no `--jsonl`.** O yt-dlp imprime `[download]  42.3% of 120MiB at 3.2MiB/s ETA 00:31`, mas o app espera `PROGRESS: 42.3% … SPEED: … ETA: …`. Com `--jsonl`, nenhum evento de progresso é emitido.
4. **App macOS marca o vídeo como "Falhou".** `finalizeDownload` verifica `fullFilePath`, que foi calculado a partir da URL (`…/watch`). O arquivo real se chama `<título>.mp4`, então o resultado é "File was not created", mesmo com o download concluído. *Abrir arquivo* também não funciona.
5. **Áudio:** `--quality audio` usa `bestaudio/best` junto com `--merge-output-format mp4`, o que gera `.webm`/`.m4a` com nome inconsistente. O certo para áudio é `-x --audio-format m4a`.
6. `output_dir.trim_end_matches('/')` não trata `\` no Windows. Use `Path::join`.

**Correção (Rust):**

```rust
// src/ytdlp/mod.rs
pub struct VideoProgress {
    pub percent: f64,
    pub speed: String,
    pub eta: String,
}

pub enum VideoEvent {
    Progress(VideoProgress),
    Log(String),
    /// Caminho final do arquivo, depois do merge/pós-processamento
    Saved(String),
}

pub fn download_video<F>(
    url: &str,
    output_dir: &str,
    quality: &VideoQuality,
    on_event: F,
) -> Result<String, Box<dyn Error + Send + Sync>>
where
    F: Fn(VideoEvent) + Send + Sync,
{
    let bin = ytdlp_binary().ok_or_else(ytdlp_not_found_message)?;
    let template = std::path::Path::new(output_dir).join("%(title)s.%(ext)s");

    let mut cmd = Command::new(&bin);
    cmd.arg("-f").arg(quality.yt_dlp_format())
        .arg("-o").arg(&template)
        .arg("--no-playlist")
        .arg("--newline")
        .arg("--progress")
        // Linha de progresso fácil de interpretar, independente do idioma ou da versão
        .arg("--progress-template")
        .arg("download:KGETPROG %(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s")
        // Caminho final, impresso depois do merge
        .arg("--print").arg("after_move:KGETSAVED %(filepath)s");
    if *quality == VideoQuality::AudioOnly {
        cmd.arg("-x").arg("--audio-format").arg("m4a");
    } else {
        cmd.arg("--merge-output-format").arg("mp4");
    }
    cmd.arg("--").arg(url) // "--" impede que uma URL iniciada por "-" vire flag
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("Failed to start {bin}: {e}"))?;

    // Drena o stderr em outra thread (evita o deadlock) e guarda as últimas linhas
    let stderr = child.stderr.take().unwrap();
    let stderr_tail = std::thread::spawn(move || {
        let mut tail = std::collections::VecDeque::with_capacity(20);
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tail.len() == 20 { tail.pop_front(); }
            tail.push_back(line);
        }
        tail.into_iter().collect::<Vec<_>>().join("\n")
    });

    let mut saved: Option<String> = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        if let Some(rest) = line.strip_prefix("KGETPROG ") {
            let mut it = rest.split('|');
            let pct = it.next().unwrap_or("").trim().trim_end_matches('%').parse().unwrap_or(0.0);
            on_event(VideoEvent::Progress(VideoProgress {
                percent: pct,
                speed: it.next().unwrap_or("").trim().to_string(),
                eta: it.next().unwrap_or("").trim().to_string(),
            }));
        } else if let Some(path) = line.strip_prefix("KGETSAVED ") {
            saved = Some(path.trim().to_string());
            on_event(VideoEvent::Saved(path.trim().to_string()));
        } else {
            on_event(VideoEvent::Log(line));
        }
    }

    let status = child.wait()?;
    let err_tail = stderr_tail.join().unwrap_or_default();
    if !status.success() {
        let last = err_tail.lines().rev().find(|l| l.contains("ERROR")).unwrap_or(err_tail.as_str());
        return Err(format!("yt-dlp failed (exit {}): {}", status.code().unwrap_or(-1), last.trim()).into());
    }
    saved.ok_or_else(|| "yt-dlp finished but did not report the output file".into())
}
```

Em `main.rs`, traduza os eventos para os formatos já entendidos pelo app e pelo JSONL:

```rust
let saved = download_video(&args.url, output_dir, &quality, |ev| match ev {
    VideoEvent::Progress(p) if args.jsonl => emit_jsonl(json!({
        "event": "progress", "progress": p.percent / 100.0, "percent": p.percent,
        "speed": p.speed, "eta": p.eta })),
    VideoEvent::Progress(p) if !quiet_mode =>
        println!("PROGRESS: {:.1}% SPEED: {} ETA: {}", p.percent, p.speed, p.eta),
    VideoEvent::Saved(path) if args.jsonl => emit_jsonl(json!({ "event": "saved", "path": path })),
    VideoEvent::Saved(path) => println!("SAVED: {path}"),
    VideoEvent::Log(l) if !quiet_mode => println!("{l}"),
    _ => {}
})?;
```

**Correção (Swift):** atualizar o caminho real quando chegar `SAVED:` e usá-lo na finalização.

```swift
// DownloadManager.swift, handleStdout (depois de bufferizar por linha; ver P1-19)
if let range = line.range(of: "SAVED: ") {
    let path = String(line[range.upperBound...]).trimmingCharacters(in: .whitespaces)
    updateDownload(downloadId) { $0.fullFilePath = path }
}
```

`Download.filename` passa a derivar de `fullFilePath`, e não da URL:

```swift
var filename: String { (fullFilePath as NSString).lastPathComponent }
```

---

### P0-7. SHA-256 informado não é verificado em vários caminhos

**Arquivo:** `src/advanced_download.rs:474-567`

**Problemas:**

1. Toda a verificação está dentro de `if !self.quiet_mode || self.status_callback.is_some()`. Uso como biblioteca (`quiet = true`, sem callback de status) com `set_expected_sha256(...)` **nunca verifica o hash** nem o tamanho.
2. No fallback sem suporte a Range (`!supports_range`), a função faz `return Ok(())` **antes** de qualquer verificação, então `--sha256` é ignorado quando o servidor não suporta Range.
3. O prompt "Would you like to verify its integrity? (y/N)" lê o stdin mesmo sem terminal (por exemplo, quando o app macOS ou um script chama o binário). Hoje isso "funciona" por acaso, porque o stdin chega em EOF.

**Correção** (já incorporada no `finish()` do P0-1): a verificação do hash **não depende** de `quiet`. O prompt interativo só aparece quando há terminal:

```rust
use std::io::IsTerminal;

let wants_iso_prompt = is_iso
    && self.expected_sha256.is_none()
    && self.resume_policy == ResumePolicy::Ask
    && !self.quiet_mode
    && std::io::stdin().is_terminal();
```

---

### P0-8. Download simples: grava direto no destino, não retoma e sobrescreve sem aviso

**Arquivos:** `src/download.rs` (`download`), `src/advanced_download.rs` (`download_whole`)

**Problemas:**

1. O download grava direto em `final_path`. Se ele cair no meio, fica um arquivo **truncado com o nome final**, indistinguível de um arquivo completo. O app macOS mostra "Concluído" se `exit 0 && size > 0`, mas basta o servidor fechar a conexão sem erro (`read` → 0 antes do `Content-Length`) para o laço terminar e devolver `Ok(())`, porque **não há conferência de bytes recebidos contra `Content-Length`**.
2. Os retries cobrem só o `send()`. Uma queda no meio do corpo **não é retentada** (deveria retomar com `Range: bytes=N-`).
3. Um arquivo existente é **sobrescrito sem aviso** (`File::create`).
4. `download_whole` (fallback do turbo) **não verifica o status HTTP**: um `404`/`500` com página HTML é gravado como se fosse o arquivo.
5. HTTP 429 (rate limit) é tratado como erro permanente (`4xx`), e `Retry-After` é ignorado.

**Correção (núcleo):**

```rust
// src/download.rs, depois de resolver final_path
let part_path = final_path.with_file_name(format!(
    "{}.part",
    final_path.file_name().unwrap().to_string_lossy()
));
let mut downloaded: u64 = if options.resume {
    std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0)
} else {
    0
};

let mut attempt = 0u32;
loop {
    let mut req = apply_headers(client.get(target), &options.extra_headers);
    if downloaded > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={downloaded}-"));
    }
    let resp = match req.send() {
        Ok(r) => r,
        Err(e) if attempt < max_retries => { attempt += 1; backoff(attempt); continue; }
        Err(e) => return Err(e.into()),
    };
    let status = resp.status();
    let append = match status.as_u16() {
        206 if downloaded > 0 => true,
        200 => { downloaded = 0; false } // servidor ignorou o Range: recomeça
        416 if downloaded > 0 => break,  // .part já está completo
        429 | 500..=599 if attempt < max_retries => {
            attempt += 1;
            std::thread::sleep(retry_after(&resp).unwrap_or_else(|| backoff_delay(attempt)));
            continue;
        }
        _ if !status.is_success() => return Err(format!("HTTP error: {status}").into()),
        _ => false,
    };
    let expected_total = resp.content_length().map(|l| l + if append { downloaded } else { 0 });
    let mut dest = std::fs::OpenOptions::new()
        .create(true).write(true).append(append).truncate(!append)
        .open(&part_path)?;
    let mut dest = std::io::BufWriter::with_capacity(256 * 1024, &mut dest);

    match copy_with_progress(resp, &mut dest, &mut downloaded, expected_total, /* callbacks + throttle */) {
        Ok(()) => {
            dest.flush()?;
            if let Some(total) = expected_total {
                if downloaded != total {
                    // corpo menor que o anunciado: tenta retomar
                    if attempt < max_retries { attempt += 1; backoff(attempt); continue; }
                    return Err(format!("Incomplete download: {downloaded} of {total} bytes").into());
                }
            }
            break;
        }
        Err(e) if attempt < max_retries => { attempt += 1; backoff(attempt); continue; }
        Err(e) => return Err(e),
    }
}
std::fs::rename(&part_path, &final_path)?;

fn retry_after(resp: &reqwest::blocking::Response) -> Option<Duration> {
    resp.headers().get(reqwest::header::RETRY_AFTER)?
        .to_str().ok()?.trim().parse::<u64>().ok()
        .map(|s| Duration::from_secs(s.min(300)))
}
```

Em `download_whole`:

```rust
let response = self.apply_headers(self.client.get(&self.url)).send()?.error_for_status()?;
```

`DownloadOptions` ganha `resume: bool` e `overwrite: OverwritePolicy` (ver [F-3](#f-3-flags---continue---no-clobber---retries---timeout)). Para manter compatibilidade, implemente `Default` e recomende `..Default::default()` na documentação.

---

### P0-9. `cargo test` e o exemplo da biblioteca não compilam

**Arquivos:** `examples/lib_usage.rs`, doctests em `src/lib.rs`, `src/download.rs`, `src/advanced_download.rs`

**Problema.** Na 1.7.0, `AdvancedDownloader::new` passou a retornar `Result` e `DownloadOptions` ganhou `extra_headers`, mas o exemplo e 10 doctests não foram atualizados. Por isso `cargo test` (sem filtros), `cargo clippy --all-targets` e o `run-tests.sh` **falham**. Quem copia o exemplo do README ou da documentação no docs.rs recebe código que não compila.

**Correção.**

```rust
// Padrão para todos os exemplos de AdvancedDownloader
let mut downloader = AdvancedDownloader::new(
    "https://example.com/large.iso".to_string(),
    "large.iso".to_string(),
    false,
    ProxyConfig::default(),
    Optimizer::new(),
)?;  // <- antes faltava o `?`
downloader.set_progress_callback(|p| println!("{:.1}%", p * 100.0));
downloader.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

```rust
// Padrão para DownloadOptions
let options = DownloadOptions {
    quiet_mode: true,
    output_path: Some("./downloads/file.zip".to_string()),
    ..Default::default()
};
```

Além disso, marque `DownloadOptions` com `#[non_exhaustive]` **ou** documente o uso de `..Default::default()`, para que novos campos (como o `resume` do P0-8) não quebrem quem usa a biblioteca.

---

### P0-10. Testes e doctests alteram a configuração real do usuário

**Arquivos:** `src/config.rs` (doctest do módulo, linhas 13-23), `tests/cli_tests.rs::test_cli_nonexistent_proxy`

**Problema.** O doctest `config.save().unwrap()` não está marcado como `no_run`, e o teste de CLI com `--proxy` grava em `~/.config/kget` ou `~/Library/Application Support/kget` (consequência do P0-3). O resultado está na seção 1.1.

**Correção.**
1. Marcar o doctest com ` ```rust,no_run`.
2. Suportar `KGET_CONFIG_DIR` para isolar os testes:

```rust
// src/config.rs
pub fn config_dir() -> Result<PathBuf, io::Error> {
    if let Some(dir) = std::env::var_os("KGET_CONFIG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    dirs::config_dir()
        .map(|d| d.join("kget"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Not able to find config directory"))
}
```

`queue.rs::history_path()` passa a usar a mesma função. Nos testes de CLI:

```rust
let tmp = tempfile::tempdir().unwrap();
Command::cargo_bin("kget").unwrap().env("KGET_CONFIG_DIR", tmp.path())...
```

---

## 3. P1: alta prioridade

### P1-1. Saída JSONL contaminada e inundada

**Arquivos:** `src/torrent/native.rs:112, 134, 364`, `src/utils.rs` (`auto_extract`), `src/main.rs` (batch e `--at`), `src/download.rs:404-412`

- O cliente nativo de torrent faz `println!("PROGRESS: …")`, `println!("FILES: …")` e `println!("FILE_PROGRESS: …")` **sempre**, mesmo com `quiet` ou `--jsonl`. Isso **quebra o parser JSONL** (essas linhas não são JSON).
- `auto_extract` deixa `unzip`/`tar`/`7z` herdarem o stdout, e a listagem de arquivos extraídos cai no meio do stream JSONL.
- O modo batch e o `--at` usam `println!` mesmo com `--jsonl`.
- `download.rs` chama o `status_callback` **a cada 8 KiB**. Um arquivo de 4 GB gera cerca de 500 mil eventos JSONL.

**Correção:**

```rust
// torrent/native.rs: só imprimir o formato de texto quando não estiver em quiet,
// e repassar os mesmos dados via callback
if !self.quiet {
    println!("PROGRESS: ...");
    emit_file_progress_json(file_infos, &stats.file_progress);
}
```

```rust
// utils.rs, auto_extract
let mut command = std::process::Command::new(cmd);
command.args(&args);
if quiet {
    command.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
}
let status = command.status()...;
```

```rust
// src/progress.rs: limitador reaproveitável
pub struct Throttle { last: std::time::Instant, every: std::time::Duration, last_pct: f64 }
impl Throttle {
    pub fn new(ms: u64) -> Self {
        Self { last: std::time::Instant::now() - std::time::Duration::from_secs(1),
               every: std::time::Duration::from_millis(ms), last_pct: -1.0 }
    }
    /// true se já passou o intervalo, se o percentual andou pelo menos 0,1% ou se chegou a 100%
    pub fn ready(&mut self, pct: f64) -> bool {
        let ok = pct >= 100.0 || (self.last.elapsed() >= self.every && (pct - self.last_pct).abs() >= 0.1);
        if ok { self.last = std::time::Instant::now(); self.last_pct = pct; }
        ok
    }
}
```

Use o `Throttle::new(200)` em `download.rs`, `advanced_download.rs` (substituindo o `last_print_time` manual) e no FTP/SFTP.

### P1-2. Torrent nativo ignora a configuração e não pode ser cancelado

**Arquivo:** `src/torrent/native.rs`

- `disable_dht: false`, `listen_port_range: Some(6881..6889)` e `enable_upnp_port_forwarding: true` estão fixos no código. `--no-dht`, `--torrent-port`, `--max-peers` e as opções de `TorrentConfig` **não têm efeito**.
- O proxy é ignorado. Pelo menos um aviso deve ser emitido quando houver proxy configurado.
- Não há cancelamento: o botão Cancelar da GUI egui não para o torrent.
- `format_eta(119.0)` resulta em `"2m 59s"`, porque `{:.0}` arredonda `1.98` para cima. Use divisão inteira.
- O JSON de `FILES:` é montado à mão e só escapa `"`. Nomes com `\` ou com quebra de linha geram JSON inválido. Use `serde_json`.

```rust
// download_magnet_native recebe &TorrentConfig e um cancel token
let port = cfg.port.unwrap_or(6881);
let opts = SessionOptions {
    disable_dht: !cfg.dht_enabled,
    disable_dht_persistence: true,
    listen_port_range: Some(port..port.saturating_add(8)),
    enable_upnp_port_forwarding: cfg.port.is_none(),
    ..Default::default()
};
// no laço de progresso:
if cancel.load(Ordering::Relaxed) {
    session.stop().await;
    return Err("Download cancelled".into());
}
```

```rust
fn format_eta(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m {}s", s / 60, s % 60),
        _ => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

fn emit_files_json(file_infos: &FileInfoList) {
    let v: Vec<_> = file_infos.iter()
        .map(|(name, size)| serde_json::json!({ "name": name, "size": size }))
        .collect();
    println!("FILES: {}", serde_json::Value::Array(v));
}
```

Ainda em torrent: no build padrão (sem `torrent-native`), o magnet é **aberto no cliente externo** e a função devolve `Ok(())` com progresso de 1.0. O histórico registra "completed" e o JSONL emite `completed` sem que nada tenha sido baixado. Devolva um resultado distinto (`TorrentOutcome::HandedOff`) e registre o status `handed_off` no histórico.

### P1-3. GUI egui mistura downloads simultâneos

**Arquivos:** `src/app.rs`, `src/gui.rs`

- `WorkerToGuiMessage` não tem ID. A GUI usa um único `active_download_id`: ao iniciar um segundo download com o primeiro ainda em andamento, **o progresso do primeiro aparece no segundo** e o primeiro fica "Downloading" para sempre.
- O worker tem **um único `cancel_token`** para todas as threads, e `Start` faz `cancel_token.store(false)`. Um download novo "descancela" o anterior, e um Cancel para todos.
- O download simples **não recebe o cancel token**, então o Cancel não para downloads sem turbo.
- `extra_headers` sempre vazio, e a GUI não grava no histórico.

**Correção: identificar cada job.**

```rust
// src/app.rs
pub type JobId = u64;

pub enum DownloadCommand {
    Start { id: JobId, url: String, output_path: String, is_advanced: bool,
            verify_iso: bool, expected_sha256: Option<String> },
    Cancel { id: JobId },
}

pub enum WorkerToGuiMessage {
    Progress { id: JobId, fraction: f32, bytes: u64, total: Option<u64>, speed_bps: u64 },
    Status { id: JobId, text: String },
    Completed { id: JobId, path: String },
    Error { id: JobId, message: String },
    Cancelled { id: JobId },
}

// no worker
let mut cancels: HashMap<JobId, Arc<AtomicBool>> = HashMap::new();
DownloadCommand::Start { id, .. } => {
    let token = Arc::new(AtomicBool::new(false));
    cancels.insert(id, token.clone());
    // passa `token` para AdvancedDownloader, para o download simples (novo campo
    // `cancel: Option<Arc<AtomicBool>>` em DownloadOptions) e para o torrent
}
DownloadCommand::Cancel { id } => {
    if let Some(t) = cancels.get(&id) { t.store(true, Ordering::SeqCst); }
}
```

Na GUI, `process_status_updates` faz `find(|d| d.id == msg.id())`. O mesmo contrato com `id` serve de base para os eventos JSONL ([F-6](#f-6-protocolo-jsonl-completo-e-migração-do-app-macos)).

### P1-4. FTP/SFTP/WebDAV/Metalink/torrent sem detecção automática

**Arquivos:** `src/main.rs`, `src/interactive.rs`

`kget ftp://ftp.gnu.org/...` **sem** `--ftp` cai no download HTTP e falha com erro do reqwest ("URL scheme is not allowed"). O mesmo vale para `sftp://`. O modo interativo não suporta yt-dlp nem WebDAV.

**Correção: um único despachante na biblioteca.**

```rust
// src/protocol.rs (novo; reexportado em lib.rs)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol { Http, Ftp, Sftp, WebDav, Torrent, Metalink, Video }

pub fn detect(url: &str) -> Protocol {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("magnet:?") || lower.ends_with(".torrent") { return Protocol::Torrent; }
    if lower.starts_with("ftp://") || lower.starts_with("ftps://") { return Protocol::Ftp; }
    if lower.starts_with("sftp://") { return Protocol::Sftp; }
    if crate::webdav::is_webdav_url(&lower) { return Protocol::WebDav; }
    if crate::metalink::is_metalink(&lower) { return Protocol::Metalink; }
    if crate::ytdlp::is_video_url(&lower) { return Protocol::Video; }
    Protocol::Http
}
```

`main.rs`, `interactive.rs`, `app.rs` e a GUI passam a usar `detect()`. As flags `--ftp`/`--sftp`/... continuam valendo como override explícito.

### P1-5. FTP: path codificado, falha sem `SIZE`, sem timeout e sem retomada

**Arquivo:** `src/ftp/client.rs`

- `url.path()` vem percent-encoded: `ftp://host/my%20file.zip` pede `/my%20file.zip` ao servidor. O usuário e a senha também vêm codificados.
- `ftp.size(path)?` aborta em servidores que não implementam `SIZE`.
- Não há timeout de conexão ou leitura (`FtpStream::connect` pode travar indefinidamente).
- O diretório de saída não é criado, e não há `quit()` no final.
- `FtpConfig.passive_mode` e `default_port` nunca são lidos. O usuário e a senha do proxy SOCKS5 são ignorados.
- Não há retomada (`REST`) nem `ftps://`.

```rust
let path = urlencoding::decode(url.path())?.into_owned();
let username = urlencoding::decode(url.username())?.into_owned();
let password = url.password().map(|p| urlencoding::decode(p).map(|c| c.into_owned())).transpose()?;

let addr = (host, port).to_socket_addrs()?.next().ok_or("Cannot resolve host")?;
let mut ftp = FtpStream::connect_timeout(addr, Duration::from_secs(20))?;
ftp.get_ref().set_read_timeout(Some(Duration::from_secs(60)))?;

let size = ftp.size(&path).ok().map(|s| s as u64); // spinner quando o servidor não informa SIZE

if let Some(parent) = Path::new(&self.output_path).parent().filter(|p| !p.as_os_str().is_empty()) {
    std::fs::create_dir_all(parent)?;
}

// retomada
let offset = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
if offset > 0 { ftp.resume_transfer(offset as usize)?; }
// ... retr ...
let _ = ftp.quit();
```

### P1-6. SFTP: sem timeout, `key_path` ignorado, host novo não é gravado

**Arquivo:** `src/sftp/mod.rs`

- `TcpStream::connect` não tem timeout. Use `connect_timeout` e `sess.set_timeout(60_000)`.
- `SftpConfig.key_path` nunca é usado. Tente esse caminho primeiro, antes de `id_ed25519`/`id_rsa`.
- O path remoto não é decodificado (`%20`), e `sftp://host/~/arquivo` não funciona. Decodifique e trate `/~/` como relativo à home do usuário.
- A documentação diz "mirrors OpenSSH accept-new", mas a chave de um host novo **não é gravada** no `known_hosts`, então o aviso se repete em toda conexão. Grave com `known_hosts.add(...)` + `write_file(...)`, ou documente o comportamento real.
- Não há retomada nem callback de progresso para JSONL.

### P1-7. Histórico pode ser apagado inteiro e guarda credenciais

**Arquivo:** `src/queue.rs`

1. `save()` usa `fs::write` sem gravação atômica. Se o processo for morto no meio, o JSON fica truncado, `try_load` falha em silêncio e retorna lista vazia, e **o próximo `save()` apaga todo o histórico**.
2. O app macOS dispara **vários `kget` em paralelo**. Cada processo faz `load → record → save`, então **o último a gravar ganha** e entradas se perdem.
3. A URL é gravada inteira, incluindo `sftp://user:SENHA@host` e `webdavs://user:SENHA@…`. São **credenciais em texto puro** no `history.json`.
4. `bytes_total`, `sha256` e `filename` (real, vindo de Content-Disposition ou do yt-dlp) nunca são preenchidos.
5. O histórico cresce sem limite, e `record` substitui entradas pela URL, o que apaga tentativas anteriores.

```rust
use fs2::FileExt;

impl DownloadHistory {
    /// Lê, aplica a alteração e grava sob lock exclusivo. Seguro entre processos.
    pub fn update<F: FnOnce(&mut Vec<HistoryEntry>)>(f: F) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let path = history_path();
        if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
        let lock = std::fs::OpenOptions::new().create(true).write(true).open(path.with_extension("lock"))?;
        lock.lock_exclusive()?;

        let mut entries = match std::fs::read_to_string(&path) {
            Ok(s) if !s.trim().is_empty() => match serde_json::from_str(&s) {
                Ok(v) => v,
                Err(_) => {
                    // preserva o arquivo corrompido em vez de sobrescrevê-lo
                    let _ = std::fs::rename(&path, path.with_extension("json.corrupt"));
                    Vec::new()
                }
            },
            _ => Vec::new(),
        };
        f(&mut entries);
        const MAX: usize = 1000;
        if entries.len() > MAX { entries.drain(..entries.len() - MAX); }

        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&entries)?)?;
        std::fs::rename(tmp, &path)?;
        lock.unlock()?;
        Ok(())
    }
}

/// Remove usuário e senha da URL antes de gravar.
pub fn redact_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut u) if u.password().is_some() || !u.username().is_empty() => {
            let _ = u.set_password(None);
            let _ = u.set_username("");
            u.to_string()
        }
        _ => raw.to_string(),
    }
}
```

### P1-8. Config antigo ou corrompido impede o binário de iniciar

**Arquivo:** `src/config.rs`

`Config::load()?` em `main.rs` **aborta o programa** se o `config.json` não tiver um campo (por exemplo, um config criado por versões antigas ou editado à mão), porque só dois campos de `TorrentConfig` têm `#[serde(default)]`. O comentário no código ("The error occurs here if the existing JSON file does not have the field.") mostra que o problema já era conhecido. A gravação também não é atômica, e o arquivo com senha de proxy fica com permissão 0644.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]                       // em Config e em TODAS as sub-structs
pub struct Config { /* ... */ }

impl Default for OptimizationConfig { /* valores atuais de Config::default() */ }
impl Default for TorrentConfig { /* ... */ }
impl Default for FtpConfig { /* ... */ }
impl Default for SftpConfig { /* ... */ }

pub fn load() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
    let path = Self::get_config_path()?;
    let Ok(text) = fs::read_to_string(&path) else { return Ok(Self::default()) };
    match serde_json::from_str(&text) {
        Ok(c) => Ok(c),
        Err(e) => {
            eprintln!("Warning: invalid config ({e}); using defaults. Backup: {}.bak", path.display());
            let _ = fs::copy(&path, path.with_extension("json.bak"));
            Ok(Self::default())
        }
    }
}

pub fn save(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = Self::get_config_path()?;
    if let Some(p) = path.parent() { fs::create_dir_all(p)?; }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}
```

### P1-9. Batch da CLI: threads sem limite, nomes colidem e flags são ignoradas

**Arquivo:** `src/main.rs:378-453`

- `thread::spawn` para **cada URL**: um arquivo com 2000 URLs abre 2000 conexões ao mesmo tempo.
- O nome vem só da URL. Duas URLs `…/download` ou `…/file.zip` de hosts diferentes **gravam no mesmo arquivo ao mesmo tempo**.
- `--sha256`, `-a`, `--ytdlp`, FTP e torrent são ignorados no batch. Não há histórico nem eventos JSONL.
- A lib já tem `BatchBuilder` com concorrência limitada, mas a CLI não o usa.

**Correção:** usar `kget::batch(urls).concurrency(args.jobs)` (nova flag `-j/--jobs`, padrão 4) e garantir nomes únicos:

```rust
/// "file.zip" vira "file (1).zip", "file (2).zip"... se o nome já estiver reservado
pub fn unique_path(dir: &Path, name: &str, taken: &mut HashSet<PathBuf>) -> PathBuf {
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("download");
    let ext = p.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    let mut candidate = dir.join(name);
    let mut i = 1;
    while candidate.exists() || taken.contains(&candidate) {
        candidate = dir.join(format!("{stem} ({i}){ext}"));
        i += 1;
    }
    taken.insert(candidate.clone());
    candidate
}
```

`BatchBuilder::download_all` precisa da mesma lógica, porque hoje também sofre colisão de nomes. `download_all_async` descarta resultados de tarefas que entraram em pânico (`if let Ok(r) = h.await`), o que desalinha a lista de resultados com as URLs. Converta o pânico em `BatchResult { result: Err(KgetError::Other("panicked")) }`.

### P1-10. Builder: repete 404, retry corrompe arquivo e velocidade média errada

**Arquivos:** `src/builder.rs`, `src/error.rs`

- Os erros de `download.rs`/`advanced_download.rs` são `Box<dyn Error>` com texto. `From<Box<dyn Error>>` vira `KgetError::Other`, então **404, 403 e 410 são repetidos** 3 vezes, e `retry_on_status` **nunca é consultado**.
- Qualquer mensagem que contenha "cancel" (por exemplo, a URL `…/cancel-policy.pdf`) vira `KgetError::Cancelled`.
- Com `connections > 1`, o retry cai no P0-1, e a segunda tentativa "conclui" com zeros.
- `avg_speed_bps = bytes / duration.as_secs()`: 1,9 s conta como 1 s, e menos de 1 s devolve o total de bytes como velocidade.
- `.spawn()` emite `Progress { speed_bps: 0, eta_secs: None }` sempre e não suporta cancelamento nem retry.
- Com `connections > 1` e sem `quiet`, ainda pode aparecer o prompt de ISO no stdin (P0-7).

**Correção.** Erros estruturados no núcleo (sem depender de texto):

```rust
// src/error.rs
pub enum KgetError {
    // ...existentes...
    /// Resposta HTTP com status de erro
    Http { status: u16, url: String },
}

impl KgetError {
    pub fn is_retryable(&self, cfg: &RetryConfig) -> bool {
        match self {
            KgetError::Http { status, .. } => cfg.retry_on_status.contains(status),
            KgetError::Network(_) => true,
            _ => false,
        }
    }
}
```

`download.rs` passa a retornar `KgetError::Http { status, url }` em vez de `format!("HTTP error: {status}")`. O retry do builder usa `e.is_retryable(&self.retry)`. Para a velocidade:

```rust
let secs = duration.as_secs_f64().max(0.001);
let avg_speed_bps = (bytes_downloaded as f64 / secs) as u64;
```

### P1-11. Metalink: v3 anunciado e não suportado, e só SHA-256 verificado

**Arquivo:** `src/metalink/mod.rs`

- A documentação diz "older Metalink 3.x format accepted", mas o parser só procura `<file>` como **filho direto** da raiz. No v3, a estrutura é `<metalink><files><file>…<resources><url>…<verification><hash>`, e o resultado é sempre "contains no downloadable files".
- O manifesto traz SHA-512 e MD5, e `checksum.rs` já calcula os dois, mas o código diz "only sha-256 supported inline" e **não verifica**.
- Depois que um espelho falha, o arquivo parcial/pré-alocado continua lá e o próximo espelho "retoma" (P0-1). Apague o `.part` antes de trocar de espelho, ou reaproveite-o **somente** quando o recurso for o mesmo.
- O `<size>` do manifesto não é comparado com o arquivo.
- O exemplo da documentação usa `"~/Downloads"`, mas o `~` não é expandido e cria uma pasta literal chamada `~`.

```rust
// parser: aceita v3 e v4
for file_node in root.descendants().filter(|n| n.is_element() && n.tag_name().name() == "file") {
    for child in file_node.descendants().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "hash" => { /* type="sha-256" | "sha256" | "sha-512" | "sha512" | "md5" */ }
            "url" => {
                // v3 usa preference (maior = melhor); v4 usa priority (menor = melhor)
                let priority = child.attribute("priority").and_then(|p| p.parse().ok())
                    .or_else(|| child.attribute("preference").and_then(|p| p.parse::<u32>().ok()).map(|p| 101u32.saturating_sub(p)))
                    .unwrap_or(999);
                // ...
            }
            _ => {}
        }
    }
}
```

```rust
// verificação com qualquer algoritmo
use crate::checksum::{compute_checksum, ChecksumAlgorithm};
let checks = [
    (file.sha256.as_deref(), ChecksumAlgorithm::Sha256),
    (file.sha512.as_deref(), ChecksumAlgorithm::Sha512),
    (file.md5.as_deref(), ChecksumAlgorithm::Md5),
];
if let Some((expected, algo)) = checks.into_iter().find_map(|(h, a)| h.map(|h| (h, a))) {
    let got = compute_checksum(&dest, &algo)?;
    if got != expected {
        let _ = std::fs::remove_file(&dest);
        last_error = Some(format!("{} mismatch on mirror {}", algo.name(), mirror.url).into());
        continue;
    }
}
```

Expansão de til, reaproveitável em CLI, interativo, metalink e builder:

```rust
// src/utils.rs
pub fn expand_tilde(p: &str) -> String {
    if p == "~" || p.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(p.trim_start_matches('~').trim_start_matches('/')).to_string_lossy().into_owned();
        }
    }
    p.to_string()
}
```

### P1-12. Download simples: brotli automático, falsos positivos em `..` e nome do servidor rejeitado

**Arquivo:** `src/download.rs`

- O client chama `.no_gzip().no_deflate()`, mas **não chama `.no_brotli()`**, e a feature `brotli` está ligada. O reqwest envia `Accept-Encoding: br` e **descomprime o corpo**. O arquivo gravado deixa de ser o arquivo do servidor (um `.tar.br` vira um `.tar` com nome `.tar.br`), o hash não confere e o `Content-Length` usado na checagem de disco é o comprimido. A correção do P0-4 (remover as features de compressão do reqwest) resolve.
- `validate_filename` rejeita qualquer nome que **contenha** `..`, como `v1..2.tar.gz` ou `relatório...pdf`. O risco real é só o componente `..` exato, e como `/` e `\` já são rejeitados, basta recusar `"."` e `".."`.
- Um Content-Disposition com `/` ou com caractere de controle **aborta o download**. Para nome vindo do servidor, **saneie** em vez de rejeitar (como `metalink::sanitise_filename` já faz).
- `get_filename_from_url_or_default` não decodifica `%20` e afins (salva `my%20file.zip`).

```rust
pub fn validate_filename(filename: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    if filename.is_empty() || filename == "." || filename == ".." {
        return Err("Invalid filename".into());
    }
    // ...demais regras atuais, sem o `contains("..")`
}

/// Para nomes vindos do servidor ou da URL: nunca falha, sempre devolve algo seguro.
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name.chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    let cleaned = if cleaned.is_empty() { "download".to_string() } else { cleaned };
    // Limita a 255 bytes sem cortar um caractere UTF-8 no meio
    let mut end = cleaned.len().min(255);
    while !cleaned.is_char_boundary(end) { end -= 1; }
    cleaned[..end].to_string()
}

// utils.rs
pub fn get_filename_from_url_or_default(url_str: &str, default: &str) -> String {
    url::Url::parse(url_str).ok()
        .and_then(|u| u.path_segments()?.next_back().map(str::to_string))
        .filter(|s| !s.is_empty())
        .map(|s| urlencoding::decode(&s).map(|c| c.into_owned()).unwrap_or(s))
        .map(|s| sanitize_filename(&s))
        .unwrap_or_else(|| default.to_string())
}
```

(`sanitize_filename` vai para `utils.rs`, e o `metalink` passa a reutilizá-lo.)

### P1-13. A pergunta "verificar ISO?" da CLI usa o caminho errado

**Arquivo:** `src/main.rs:604-613`

Depois do download simples, a verificação de ISO procura `get_filename_from_url_or_default(&args.url)` no diretório atual, **ignorando `-O` e o Content-Disposition**. Com `-O ~/Downloads/x.iso`, o resultado é "No such file or directory". O mesmo vale para o `--extract` em `main.rs:632-646`.

**Correção:** fazer `download()` devolver o caminho final, com uma nova função para não quebrar a API atual:

```rust
// src/download.rs
pub fn download_to_path(target: &str, proxy: ProxyConfig, optimizer: Optimizer,
                        options: DownloadOptions, cb: Option<&(dyn Fn(String) + Send + Sync)>)
    -> Result<PathBuf, Box<dyn Error + Send + Sync>> { /* corpo atual, retornando final_path */ }

pub fn download(/* mesma assinatura */) -> Result<(), Box<dyn Error + Send + Sync>> {
    download_to_path(target, proxy, optimizer, options, cb).map(|_| ())
}
```

`main.rs` usa o `PathBuf` devolvido para a ISO, para o `--extract`, para o histórico (`filename` e `bytes_total`) e no evento JSONL `completed { path }`.

### P1-14. WebDAV: parsing de credenciais frágil e app macOS rejeita `webdav://`

**Arquivos:** `src/webdav/mod.rs`, `DownloadManager.swift` (`DownloadValidator`)

- `parse_webdav_url` usa `rest.find('@')` na URL **inteira**. Um `@` no caminho (`/users/@me/file`) ou uma senha com `@` (`p@ss` codificado como `p%40ss`) quebra o parsing, e as credenciais não são percent-decoded.
- `DownloadValidator.validate` aceita só http/https/ftp/sftp/magnet. **Colar `webdav://` no app mostra "Supported: …"**, mesmo com `isDownloadableURL` e `handleURL` aceitando WebDAV.
- WebDAV usa sempre o download simples, sem opção de turbo.

```rust
fn parse_webdav_url(raw: &str) -> Result<(String, Option<String>, Option<String>), url::ParseError> {
    let mut u = url::Url::parse(raw)?;
    let scheme = if u.scheme().eq_ignore_ascii_case("webdavs") { "https" } else { "http" };
    let user = (!u.username().is_empty())
        .then(|| urlencoding::decode(u.username()).map(|c| c.into_owned()).unwrap_or_default());
    let pass = u.password().map(|p| urlencoding::decode(p).map(|c| c.into_owned()).unwrap_or_default());
    let _ = u.set_username("");
    let _ = u.set_password(None);
    // Url::set_scheme não troca entre esquemas "especiais" e "não especiais"; por isso, string
    let http_url = format!("{scheme}{}", &u.as_str()[u.scheme().len()..]);
    Ok((http_url, user, pass))
}
```

Com isso, o `base64_encode` próprio pode sair: `RequestBuilder::basic_auth(user, pass)` faz o mesmo. Avise (em status) quando houver credenciais em `webdav://` sem TLS.

```swift
// DownloadValidator.validate: incluir
|| trimmed.lowercased().hasPrefix("webdav://")
|| trimmed.lowercased().hasPrefix("webdavs://")
// e a mensagem: "Supported: http, https, ftp, sftp, webdav, and magnet links."
```

### P1-15. App macOS: corrida de dados em `processes` e `progressTimers`

**Arquivo:** `DownloadManager.swift`

`terminationHandler` roda numa **thread de background** e chama `cleanupProcess`, que faz `processes.removeValue(...)`, enquanto `runProcess`/`terminateProcess` mexem no mesmo dicionário na main thread. Dicionários Swift não são thread-safe, então isso pode causar crash aleatório (EXC_BAD_ACCESS) com vários downloads terminando ao mesmo tempo.

```swift
process.terminationHandler = { [weak self] proc in
    DispatchQueue.main.async {
        self?.cleanupProcess(downloadId: downloadId, process: proc)
        self?.finalizeDownload(downloadId: downloadId, exitCode: proc.terminationStatus)
    }
}
```

Marque também a classe com `@MainActor`, ou concentre os acessos a `processes` e `progressTimers` na main queue.

### P1-16. App macOS: qualquer linha com "error" ou "failed" vira erro

**Arquivo:** `DownloadManager.swift` (`OutputParser.containsError`, `processErrorOutput`)

Linhas como `Server error 503 on attempt 1, retrying…`, um arquivo `error-codes.pdf`, `Saving to: …/failed-tests.log` ou avisos do yt-dlp **gravam `dl.error`** com o primeiro trecho que aparecer. Em um download que terminou bem, o erro fica oculto, mas, se o download falhar depois, a mensagem mostrada é a errada.

**Correção:** decidir sucesso ou falha **só pelo exit code**. Como mensagem de erro, usar a **última** linha do stderr que comece com `Error:` (formato de `main()`) ou o evento JSONL `error` ([F-6](#f-6-protocolo-jsonl-completo-e-migração-do-app-macos)).

### P1-17. App macOS: Share Extension corta URLs com `&`

**Arquivo:** `ShareViewController.swift`

`addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed)` **não** codifica `&`, `=` nem `+`. Uma URL `https://cdn/x.zip?token=a&exp=1` vira `kget://download?url=https://cdn/x.zip?token=a&exp=1`, e o app recebe só `…?token=a`. **Links assinados (S3, CDN, Google Drive) quebram.**

```swift
var allowed = CharacterSet.urlQueryAllowed
allowed.remove(charactersIn: "&=+?#")
guard let encoded = urlString.addingPercentEncoding(withAllowedCharacters: allowed),
      let kgetURL = URL(string: "kget://download?url=\(encoded)") else { complete(); return }
```

### P1-18. App macOS: `BinaryLocator` não procura o Homebrew do Apple Silicon

**Arquivo:** `DownloadManager.swift` (`BinaryLocator.findKgetBinary`)

Fora do bundle, a busca olha `/usr/local/bin/kget`, mas **não** `/opt/homebrew/bin/kget`, que é onde a fórmula `Formula/kget.rb` instala em Macs M1/M2/M3/M4. Também tenta `currentDirectoryPath + "/target/release/kget"`, que num app aberto pelo Finder é `/`.

```swift
let paths: [String?] = [
    Bundle.main.executableURL?.deletingLastPathComponent().appendingPathComponent("kget-bin").path,
    "/opt/homebrew/bin/kget",
    "/usr/local/bin/kget",
    NSHomeDirectory() + "/.cargo/bin/kget",
]
return paths.compactMap { $0 }.first { FileManager.default.isExecutableFile(atPath: $0) }
```

### P1-19. App macOS: saída lida em blocos, não em linhas

**Arquivo:** `DownloadManager.swift` (`handleStdout`)

`availableData` devolve blocos arbitrários. Uma linha `PROGRESS: 45.2% (…)` pode chegar partida em dois blocos, e `FILES: [ … ]` (JSON grande de torrent) quase sempre chega partido, então a lista de arquivos do torrent fica vazia. `extractJSONArray` também para no primeiro `]`, o que quebra nomes que contêm `]`.

```swift
private var stdoutBuffers: [UUID: Data] = [:]   // acessado só na main queue

stdoutPipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
    let chunk = handle.availableData
    guard !chunk.isEmpty else { return }
    DispatchQueue.main.async {
        guard let self else { return }
        var buf = self.stdoutBuffers[downloadId, default: Data()]
        buf.append(chunk)
        while let nl = buf.firstIndex(of: 0x0A) {
            let lineData = buf[buf.startIndex..<nl]
            buf.removeSubrange(buf.startIndex...nl)
            if let line = String(data: lineData, encoding: .utf8) {
                self.handleLine(line, downloadId: downloadId)
            }
        }
        self.stdoutBuffers[downloadId] = buf
    }
}
```

`extractJSONArray` passa a pegar **a linha inteira** depois do prefixo (`FILES: `) e a decodificar com `JSONSerialization`.

### P1-20. App macOS: não deixa baixar de novo uma URL já concluída

**Arquivo:** `DownloadManager.swift` (`startDownload`)

Qualquer URL presente na lista, inclusive com status `completed`, `failed` ou `cancelled`, bloqueia um novo download com "This download is already in the list.". O histórico tem um botão "Baixar novamente" que não funciona enquanto o item antigo estiver na lista.

```swift
if let existing = downloads.first(where: {
    DownloadValidator.normalize($0.url) == normalizedURL && [.pending, .downloading, .verifying].contains($0.status)
}) { /* mantém o comportamento atual só para downloads ativos */ }
```

### P1-21. Transmission: timeout fixo de 30 min e navegador aberto sem pedir

**Arquivo:** `src/torrent/transmission.rs`

- `max_attempts = 1800`, com um poll por segundo: **qualquer torrent que leve mais de 30 minutos falha**, mesmo baixando normalmente. Troque por detecção de travamento ("sem progresso por N minutos").
- `open_url_system(&transmission_web_url)` **abre o navegador** em todo download. Deixe isso opcional (`KGET_TRANSMISSION_OPEN_WEB=1`).
- Um `download_dir` relativo é interpretado pelo daemon a partir do diretório *dele*. Use `std::fs::canonicalize` antes de enviar.
- Em caso de falha da RPC, o fallback abre o cliente externo e devolve `Ok(())`, com o mesmo problema de "completed falso" do P1-2.

### P1-22. Modo interativo: `~` não é expandido e caminhos com espaço quebram

**Arquivo:** `src/interactive.rs`

- O próprio `help` usa `download -a -o ~/Downloads/ubuntu.iso …`, mas `~` não é expandido e cria uma pasta `./~/Downloads`.
- `split_whitespace` quebra `-o "/Users/eu/Meus Downloads/x.iso"`.
- Não suporta yt-dlp nem WebDAV, nem `-H`.
- Colar só a URL (sem `download`) dá "Unknown command".

```rust
// Cargo.toml: shlex = "1.3"
let parts: Vec<String> = match shlex::split(input) {
    Some(p) => p,
    None => { eprintln!("Unbalanced quotes"); continue; }
};
// URL colada sozinha
if parts.len() == 1 && (parts[0].contains("://") || parts[0].starts_with("magnet:?")) {
    run_download(parse_download_args(&[parts[0].as_str()])?, &config)?;
    continue;
}
// -o: args.output = Some(kget::utils::expand_tilde(parts[i]))
```

---

## 4. P2: média e baixa prioridade

| # | Onde | Problema | Correção |
|---|---|---|---|
| P2-1 | `src/optimization.rs` | **Cache e compressão são código morto.** `get_cached_file`, `cache_file`, `compress` e `decompress` nunca são chamados. `--no-cache` e `config set compression/cache` do modo interativo não fazem nada. Ainda há bugs: `decompress` trata `28 b5 2f fd` como brotli (é o *magic* do **zstd**; brotli não tem magic), e `compress` (nível 4-6) usa `lz4::block::compress`, enquanto `decompress` usa `lz4::Decoder` (formato *frame*), que são formatos incompatíveis. | Remover a compressão e o cache na 1.7.1 (marcar como `#[deprecated]` na API pública, tirar `flate2`, `lz4` e `brotli` das dependências) e esconder `--no-cache` (`#[arg(hide = true)]`). Se o cache for mantido, chavear pela URL **+ ETag** e validar com `If-None-Match`. |
| P2-2 | `Cargo.toml` | Dependências não usadas: `ftp = "3.0"` (o código usa `suppaftp`), `console`, `futures`, `tempfile` (só nos testes; já está em dev-deps) e `opener` (opcional, nunca referenciado). A feature `async` está vazia e o `tokio` "full" **sempre** compila, mesmo no build padrão. | Remover as dependências mortas. Tornar `tokio` opcional: `async = ["dep:tokio"]`, `torrent-native = ["librqbit", "dep:tokio"]`, `torrent-transmission = [..., "dep:tokio"]`, com features mínimas (`rt-multi-thread`, `time`, `macros`). Reduz o tempo de build e o tamanho do binário. |
| P2-3 | `src/main.rs:1` | `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` faz o **binário CLI de release no Windows** não ter console. Com `AttachConsole` a saída aparece, mas o prompt pelo stdin e o pipe `kget … \| findstr` se comportam mal, e o `cmd` não espera o processo terminar. | Aplicar o subsistema "windows" só quando compilado com `gui` **e** gerar um segundo binário (`kget-gui.exe`); ou manter console e esconder a janela no modo `--gui` via `FreeConsole`. |
| P2-4 | `src/main.rs` (`local_tz_offset_secs`) | Fuso horário obtido executando `date +%z`: no Windows retorna 0 (`--at` usa UTC) e ignora mudanças de horário de verão durante a espera. | Usar o crate `time` (`UtcOffset::current_local_offset`) ou `chrono::Local`. Recalcular o alvo ao acordar. |
| P2-5 | `src/main.rs` | `parse_extra_headers` exige `": "` exato. `-H "Referer:https://x"` é **ignorado em silêncio**. | `split_once(':')` + `trim()`. Avisar quando o header for inválido. |
| P2-6 | `src/progress.rs` | O template paralelo usa `{chunks}`, que não é uma chave do indicatif, e exibe "Chunks:  active". | Remover a linha ou usar `{msg}`. |
| P2-7 | `src/queue.rs` | O `id` do histórico é `now as u32` + hash de 32 bits da URL, e colide ao baixar a mesma URL duas vezes no mesmo segundo. `clear_completed` também remove os `Cancelled`, mas a documentação da CLI diz só "completed". | Usar um contador + timestamp em nanossegundos. Alinhar a documentação. |
| P2-8 | `src/download.rs` | Buffer de 8 KiB e `write_all` sem `BufWriter`: muitas syscalls em links rápidos. | Buffer de 64-256 KiB + `BufWriter`. |
| P2-9 | `src/checksum.rs` | O sidecar não aceita o formato mais comum, "`arquivo.iso.sha256` só com o hash", nem o formato com um espaço (`<hash> <arquivo>`). Hash de 64 caracteres sempre é tratado como SHA-256 (BLAKE3 também tem 64). | Ver [F-8](#f-8-sidecar-de-checksum-só-com-o-hash). |
| P2-10 | `src/advanced_download.rs` | `#[allow(dead_code)] proxy`, `_is_iso` não usado, `use hex;` redundante e a `ResumePolicy::Ask` documentada como "pergunta antes de retomar", quando só é usada no prompt de ISO. | Limpar e documentar o comportamento real. |
| P2-11 | `src/ytdlp/mod.rs` | `VideoQuality::from_str` conflita com `FromStr` (warning do clippy). `odysee.com/@` está na lista do Rust e não na do Swift (listas duplicadas). | Implementar `std::str::FromStr`. O app deve perguntar ao binário (`kget --is-video <url>`, ou o evento JSONL `started { protocol }`) em vez de manter uma lista própria. |
| P2-12 | Clippy | 34 warnings na lib e 3 no binário (`collapsible_if` ×25, `derivable_impls` ×2, `manual_div_ceil`, `iter().last()` → `next_back()` etc.). | `cargo clippy --fix` e depois `-D warnings` no CI. |
| P2-13 | `src/torrent/mod.rs` | `download_magnet` recebe parâmetros com prefixo `_` que **são usados** (`_output_dir`, `_quiet`...), o que confunde a leitura. | Renomear sem `_` e usar `#[cfg_attr(..., allow(unused))]` onde precisar. |
| P2-14 | `src/sftp/mod.rs` | O aviso de "host not in known_hosts" vai para o stderr mesmo em `quiet`. | Enviar via callback ou respeitar o modo quiet (mantendo o aviso no stderr só em modo interativo). |
| P2-15 | `macos-app` `Download.filename` | Usa a URL. Com Content-Disposition ou yt-dlp, o nome exibido difere do arquivo real. | Derivar de `fullFilePath` (P0-6) e atualizar via evento `saved`. |
| P2-16 | `macos-app` `SHA256Calculator` | Duplica a verificação do Rust (o hash do ISO é calculado duas vezes quando `--sha256` foi informado). | Usar o hash que o binário já emite (`SHA256: …` / evento JSONL `verified`). |
| P2-17 | Repositório | **Não há CI**: `.github/` só tem templates de issue. Os erros do P0-9 passaram despercebidos. | Ver [F-10](#f-10-ci-no-github-actions). |
| P2-18 | `README`/`LIB.md`/traduções | Exemplos com a API antiga (P0-9) e menções a features quebradas (Metalink v3, cache). | Atualizar junto com a release ([seção 7](#7-plano-de-release-171)). |

---

## 5. Features novas para a 1.7.1

> Critério: entram só features pequenas, que se apoiam nas correções acima e melhoram diretamente o download.

### F-1. Retomada real com `.part` + `.kget`

Já especificada em [P0-1](#p0-1-retomada-do-modo-turbo-gera-arquivo-de-zeros)/[P0-2](#p0-2-chunk-truncado-é-aceito-como-completo)/[P0-8](#p0-8-download-simples-grava-direto-no-destino-não-retoma-e-sobrescreve-sem-aviso). Do ponto de vista do usuário: **Ctrl+C a qualquer momento e o mesmo comando continua de onde parou**, nos modos simples e turbo, inclusive em Pausar/Retomar no app macOS. Pausar = `process.interrupt()` (SIGINT); retomar = executar de novo com `-c`.

Para isso, `main.rs` precisa tratar SIGINT e gravar o estado antes de sair:

```rust
// Cargo.toml: ctrlc = "3.4"
let cancel = Arc::new(AtomicBool::new(false));
{
    let c = cancel.clone();
    ctrlc::set_handler(move || c.store(true, Ordering::SeqCst))?;
}
downloader.set_cancel_token(cancel.clone());
// Ao cancelar, AdvancedDownloader grava o .kget e retorna Err("Download cancelled").
// Exit code 130 (convenção para SIGINT).
```

### F-2. Pausar/Retomar no app macOS

```swift
func pauseDownload(_ d: Download) {
    processes[d.id]?.interrupt()               // SIGINT → kget grava o estado e sai
    updateStatus(for: d.id, to: .paused)       // novo case em DownloadStatus
}
func resumeDownload(_ d: Download) {
    executeDownload(d, advanced: d.isAdvanced, extraArgs: ["-c"])
}
```

### F-3. Flags `--continue`, `--no-clobber`, `--retries`, `--timeout`

```rust
/// Continue a partially downloaded file (.part)
#[arg(short = 'c', long = "continue")]
continue_download: bool,

/// Do not overwrite an existing file; skip the download instead
#[arg(long = "no-clobber", conflicts_with = "continue_download")]
no_clobber: bool,

/// Maximum attempts per request (default 3)
#[arg(long = "retries", default_value_t = 3)]
retries: u32,

/// I/O timeout in seconds (connect and each read)
#[arg(long = "timeout", default_value_t = 60)]
timeout: u64,
```

```rust
// src/lib.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverwritePolicy {
    #[default]
    Overwrite,
    Skip,     // --no-clobber
    Rename,   // "file (1).zip" (usado pelo batch, P1-9)
}
```

### F-4. Caminho configurável do yt-dlp

Variável `KGET_YTDLP_PATH` + campo nas configurações do app macOS (seções [5.3.2](#532-rust-fallback-com-caminhos-absolutos) e [5.3.3](#533-opcional--recomendado-configuração-pelo-usuário)). Opcionalmente, um campo `ytdlp_path` em `config.json`, lido quando a variável não estiver definida.

### F-5. `kget --doctor`

Diagnóstico em um comando, útil para quem roda o app pelo Dock e para issues no GitHub:

```rust
#[arg(long = "doctor")]
doctor: bool,

if args.doctor {
    println!("KGet {}", env!("CARGO_PKG_VERSION"));
    println!("Config:   {}", kget::config::config_dir()?.display());
    println!("PATH:     {}", std::env::var("PATH").unwrap_or_default());
    match kget::ytdlp_binary() {
        Some(p) => println!("yt-dlp:   {p}"),
        None => println!("yt-dlp:   NOT FOUND\n{}", kget::ytdlp::ytdlp_not_found_message()),
    }
    for tool in ["unzip", "tar", "7z"] {
        println!("{tool:<9} {}", if which_in_path(tool) { "ok" } else { "missing (needed for --extract)" });
    }
    println!("Features: gui={} torrent-native={} transmission={}",
        cfg!(feature = "gui"), cfg!(feature = "torrent-native"), cfg!(feature = "torrent-transmission"));
    let cfg = Config::load()?;
    println!("Proxy:    {}", if cfg.proxy.enabled { cfg.proxy.url.as_deref().unwrap_or("?") } else { "disabled" });
    println!("Limit:    {:?}", cfg.optimization.speed_limit);
    return Ok(());
}
```

O app macOS pode mostrar a saída do `--doctor` em **Settings → About → Diagnostics**, o que ajuda a identificar o ambiente de cada Mac (Apple Silicon ou Intel, Homebrew em `/opt/homebrew` ou `/usr/local`).

### F-6. Protocolo JSONL completo e migração do app macOS

O `--jsonl` já existe, mas é parcial: os modos FTP, SFTP, WebDAV, Metalink, yt-dlp e batch não emitem progresso, e o app macOS ainda depende de texto (`PROGRESS:`, `Length:`, `FILES:`). Na 1.7.1, o JSONL vira o contrato estável, **versionado**:

```jsonc
{"v":1,"event":"started","id":"…","url":"…","protocol":"http","advanced":true,"output":"/abs/path"}
{"v":1,"event":"meta","total":1048576,"filename":"x.iso","resumable":true}
{"v":1,"event":"progress","bytes":524288,"total":1048576,"percent":50.0,"speed_bps":3145728,"eta_secs":12}
{"v":1,"event":"status","message":"Calculating SHA256..."}
{"v":1,"event":"files","files":[{"name":"a.mkv","size":123}]}          // torrent
{"v":1,"event":"verified","algorithm":"sha256","hash":"…","ok":true}
{"v":1,"event":"saved","path":"/abs/path/x.iso","bytes":1048576}
{"v":1,"event":"completed"}
{"v":1,"event":"error","code":"http_404","message":"…","retryable":false}
```

Implementação na lib, com um único tipo serializável compartilhado por CLI, GUI e app:

```rust
// src/events.rs
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum JsonEvent {
    Started { url: String, protocol: String, advanced: bool, output: String },
    Meta { total: Option<u64>, filename: String, resumable: bool },
    Progress { bytes: u64, total: Option<u64>, percent: Option<f64>, speed_bps: u64, eta_secs: Option<u64> },
    Status { message: String },
    Files { files: Vec<FileEntry> },
    Verified { algorithm: String, hash: String, ok: bool },
    Saved { path: String, bytes: u64 },
    Completed,
    Error { code: String, message: String, retryable: bool },
}

pub fn emit(ev: &JsonEvent) {
    let mut v = serde_json::to_value(ev).unwrap();
    v["v"] = 1.into();
    println!("{v}");
}
```

No app macOS, `buildArguments` passa a incluir `--jsonl`, e `handleLine` decodifica com `JSONDecoder`:

```swift
struct KEvent: Decodable {
    let event: String
    let bytes: Int64?; let total: Int64?; let percent: Double?
    let speed_bps: Double?; let eta_secs: Double?
    let path: String?; let message: String?; let code: String?
    let files: [KFile]?
}

private func handleLine(_ line: String, downloadId: UUID) {
    guard let data = line.data(using: .utf8),
          let ev = try? JSONDecoder().decode(KEvent.self, from: data) else { return }
    updateDownload(downloadId) { dl in
        switch ev.event {
        case "meta":     if let t = ev.total { dl.expectedBytes = t; dl.totalSize = Formatters.formatBytes(t) }
        case "progress": dl.progress = ev.percent ?? dl.progress
                         if let s = ev.speed_bps { dl.speed = Formatters.formatSpeed(s); dl.speedRaw = s }
                         if let e = ev.eta_secs { dl.eta = Formatters.formatTime(e) }
        case "files":    dl.torrentFiles = (ev.files ?? []).enumerated().map { TorrentFile(id: $0.offset, name: $0.element.name, size: $0.element.size, downloaded: 0, progress: 0) }
        case "saved":    if let p = ev.path { dl.fullFilePath = p }
        case "error":    dl.error = ev.message
        default: break
        }
    }
}
```

Com isso, a **sondagem do tamanho do arquivo a cada 0,5 s** (`updateFileBasedProgress`) pode ser removida. Hoje ela é a única fonte de progresso do download simples no app, porque `download.rs` só emite `PROGRESS:` via callback, e o callback só existe com `--jsonl`. Ela também mostra progresso falso quando não há `Content-Length` (`currentSize / 1_000_000 * 5`).

### F-7. `--jobs N` no batch

Especificada em [P1-9](#p1-9-batch-da-cli-threads-sem-limite-nomes-colidem-e-flags-são-ignoradas). Também vale aceitar `-` como arquivo de batch (ler URLs do stdin): `cat urls.txt | kget --batch - -j 8`.

### F-8. Sidecar de checksum só com o hash

```rust
// src/checksum.rs, no início de parse_sidecar
let meaningful: Vec<&str> = content.lines().map(str::trim)
    .filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
if meaningful.len() == 1 && meaningful[0].chars().all(|c| c.is_ascii_hexdigit()) {
    if let Some(algo) = ChecksumAlgorithm::from_hex_len(meaningful[0]) {
        return Some((algo, meaningful[0].to_lowercase()));
    }
}
// GNU com um espaço: "<hash> <arquivo>"
// let gnu = line.split_once("  ").or_else(|| line.split_once(" *")).or_else(|| line.split_once(' '));
```

E na CLI: `--checksum-url <url>` (equivalente ao `.verify_from()` do builder) e `--checksum sha512:<hex>` para qualquer algoritmo, além do `--sha256` atual.

### F-9. Arquivos `.torrent` (locais e remotos)

Hoje `is_supported_magnet_link` rejeita tudo que não seja magnet, e o app mostra `showTorrentFileWarning()`. O `librqbit` já aceita `.torrent`:

```rust
// torrent/native.rs, add_torrent
let add = if self.magnet.starts_with("magnet:") || self.magnet.starts_with("http") {
    AddTorrent::from_url(&self.magnet)
} else {
    AddTorrent::from_bytes(std::fs::read(&self.magnet)?)
};
```

`download_magnet` aceita caminhos e URLs `.torrent` (`protocol::detect` → `Torrent`), e o app macOS passa a abrir `.torrent` pelo Finder em vez de mostrar o aviso.

### F-10. CI no GitHub Actions

`.github/workflows/ci.yml`:

```yaml
name: CI
on:
  push: { branches: [main] }
  pull_request:
jobs:
  rust:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    env:
      KGET_CONFIG_DIR: ${{ runner.temp }}/kget-config   # P0-10
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: clippy, rustfmt }
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt -- --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test --all-targets
      - run: cargo test --doc
      - run: cargo check --features torrent-native
      - name: GUI build (Linux deps)
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y libxkbcommon-dev libgtk-3-dev libdbus-1-dev
          cargo check --features gui
  macos-app:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - run: cd macos-app && swift build -c release
```

---

## 6. Testes a adicionar

Todos em `tests/mock_server_tests.rs` (wiremock), salvo indicação, e com `KGET_CONFIG_DIR` temporário.

| Teste | Cobre |
|---|---|
| `advanced_resume_after_interrupt_completes_correctly`: pré-cria `.part` + `.kget` com o chunk 0 completo e o chunk 1 pela metade; espera **só** as requisições `Range` que faltam e o arquivo final idêntico | P0-1 |
| `advanced_preallocated_file_without_state_is_not_trusted`: arquivo de zeros com o tamanho total e **sem** `.kget`; espera que o download aconteça e que o conteúdo bata | P0-1 (versão definitiva da reprodução da auditoria) |
| `advanced_short_chunk_is_retried_from_offset`: o primeiro GET do chunk devolve só 1000 bytes; o mock seguinte exige `Range: bytes=<start+1000>-<end>` | P0-2 |
| `advanced_503_then_206_succeeds_with_backoff` | P0-2 |
| `advanced_quiet_with_sha256_mismatch_fails` | P0-7 |
| `advanced_no_range_server_still_verifies_sha256` | P0-7 |
| `simple_body_shorter_than_content_length_errors_or_resumes` | P0-8 |
| `simple_leaves_part_file_on_failure_and_no_final_file` | P0-8 |
| `simple_429_respects_retry_after` | P0-8 |
| `download_does_not_brotli_decode`: o servidor envia `Content-Encoding: br` e o arquivo gravado deve ter os bytes crus | P1-12 |
| `cli_flags_do_not_persist_config` (`cli_tests.rs`): roda `kget -l 100 …` e verifica que o `config.json` em `KGET_CONFIG_DIR` não mudou | P0-3 |
| `proxy_http_type_applies_to_https_targets`: mock de proxy que conta as conexões `CONNECT` | P0-4 |
| `history_concurrent_writers_keep_all_entries`: 20 threads chamando `DownloadHistory::update` | P1-7 |
| `history_redacts_credentials` | P1-7 |
| `config_missing_fields_loads_defaults` (`unit_tests.rs`) | P1-8 |
| `ytdlp_binary_honours_env_override` e `ytdlp_binary_finds_in_fallback_dir` (`unit_tests.rs`, com um executável falso em um tempdir e `PATH` mínimo) | P0-5 |
| `metalink_v3_manifest_parses` e `metalink_sha512_is_verified` | P1-11 |
| `jsonl_output_is_valid_json_per_line` (`cli_tests.rs`): cada linha do stdout com `--jsonl` deve passar no `serde_json::from_str` | P1-1 / F-6 |
| `protocol_detect_table` (`unit_tests.rs`) | P1-4 |
| `filename_sanitize_and_percent_decode` | P1-12 |

---

## 7. Plano de release 1.7.1

### 7.1 Ordem sugerida de implementação

1. **Infraestrutura:** P0-9 (exemplos e doctests), P0-10 (`KGET_CONFIG_DIR`), F-10 (CI). Com isso, todo o resto passa a ser testado automaticamente.
2. **Núcleo HTTP:** P0-4 (`http.rs`), P0-8 + F-3 (download simples com `.part`), P0-1 + P0-2 + P0-7 (turbo), P1-12.
3. **CLI:** P0-3, P1-4, P1-13, P1-1, P1-9 + F-7, F-1 (Ctrl+C), F-5.
4. **yt-dlp:** P0-5 + P0-6 + F-4.
5. **Persistência:** P1-7, P1-8.
6. **Protocolos:** P1-5, P1-6, P1-11, P1-14, P1-2, P1-21, F-8, F-9.
7. **Frontends:** F-6 (JSONL + app macOS), P1-15 a P1-20, F-2, P1-3 (egui), P1-22.
8. **Limpeza:** seção 4 (P2).

### 7.2 Arquivos com versão a atualizar (1.7.0 → 1.7.1)

- `Cargo.toml` (`version`), seguido de `cargo update -p Kget` para atualizar o `Cargo.lock`
- `macos-app/KGet/Info.plist` (linhas 52 e 77)
- `macos-app/ShareExtension/Info.plist` (linhas 16 e 18)
- `Formula/kget.rb` (URL do tag; o SHA256 é atualizado pelo `release.sh`)
- `README.md` (título, DMG, trechos `Kget = "1.7.x"`), `LIB.md`
- `translations/README.pt-BR.md`, `translations/README.es.md`, `translations/LIB.pt-br.md`, `translations/LIB.es.md`
- `CHANGELOG.md` + `translations/CHANGELOG.pt-BR.md` + `translations/CHANGELOG.es.md`
- `docs/ROADMAP.md` (nova seção "v1.7.1")

### 7.3 Rascunho do CHANGELOG

```markdown
## [1.7.1] - 2026-XX-XX

### Fixed
- **Turbo mode could silently produce corrupted files:** resuming an interrupted parallel download
  (including Cancel → Retry in the macOS app and automatic retries in `kget::builder`) treated the
  preallocated file as complete; chunks cut short by the server were accepted as complete. Downloads now
  use `<file>.part` + a `<file>.kget` resume state, validate every chunk, and retry from the exact offset.
- **CLI flags were permanently written to `config.json`** (`--proxy`, `--proxy-pass`, `-l`, `--no-cache`, torrent flags).
  They now apply only to the current run; use `--save-config` to persist (proxy passwords are never saved).
- **HTTP proxies did not apply to HTTPS downloads; SOCKS5 proxies did not work;** invalid proxies were silently ignored.
- **macOS app could not find yt-dlp installed via Homebrew** when launched from the Dock/Finder.
- **macOS app marked finished video downloads as failed** and showed no progress for yt-dlp downloads.
- yt-dlp could hang when it wrote a lot of warnings (stderr pipe was never drained).
- `--sha256` was ignored in quiet/library mode and when the server did not support range requests.
- Simple downloads no longer leave a truncated file with the final name; responses shorter than
  `Content-Length` are retried; HTTP 429 honours `Retry-After`.
- Brotli responses were transparently decompressed, altering the saved file.
- `cargo test`, `examples/lib_usage.rs`, and 10 doctests did not compile.
- Running the test suite modified the user's real configuration.
- History file could be wiped or lose entries with concurrent downloads; credentials in URLs are no longer stored.
- An old or partial `config.json` no longer prevents KGet from starting.
- FTP/SFTP/WebDAV URLs are detected from the scheme without `--ftp`/`--sftp`; percent-encoded paths fixed.
- Metalink 3.x manifests are now parsed; SHA-512/MD5 hashes are verified.
- macOS: Share Extension no longer truncates URLs containing `&`; `webdav://` accepted; Apple Silicon
  Homebrew `kget` found; crash-prone concurrent access in the download manager fixed.

### Added
- `-c/--continue`, `--no-clobber`, `--retries`, `--timeout`, `--jobs`, `--save-config`, `--doctor`.
- `KGET_YTDLP_PATH` and a "yt-dlp path" field in the macOS settings.
- Pause/Resume in the macOS app.
- Versioned JSONL event protocol (`"v":1`) for all protocols; the macOS app now uses it.
- Checksum sidecar files that contain only the hash.
- `.torrent` files (local paths and URLs) with the native client.
- GitHub Actions CI (Linux, macOS, Windows).

### Removed / Deprecated
- Unused cache/compression code paths in `Optimizer` (deprecated); `--no-cache` hidden.
```

### 7.4 Teste manual antes do tag (macOS)

Rode nos Macs disponíveis, idealmente um Apple Silicon e um Intel, porque o caminho do Homebrew muda (`/opt/homebrew` vs `/usr/local`):

1. App aberto **pelo Dock**: vídeo do YouTube (P0-5/P0-6), WebDAV (P1-14), link com `?a=1&b=2` via Share (P1-17).
2. Turbo: iniciar uma ISO grande → Pausar/Cancelar → Retomar. Conferir o SHA-256 contra o oficial (P0-1/P0-2).
3. Definir limite de velocidade → baixar → apagar o limite → baixar de novo: a velocidade deve voltar ao normal (P0-3).
4. Três downloads simultâneos terminando juntos: o app não pode travar (P1-15).
5. `kget --doctor` no terminal e pela tela de diagnóstico do app (F-5).

---

## 8. Checklist geral

**P0**
- [ ] P0-1 Retomada turbo com `.part` + `.kget`
- [ ] P0-2 Chunk validado, retry a partir do offset, `write_all_at`, pool dedicado
- [ ] P0-3 Flags da CLI não persistem; `--save-config`; app sempre passa `-l`
- [ ] P0-4 `http::build_client` único; feature `socks`; erro de proxy é erro
- [ ] P0-5 yt-dlp encontrado pelo app (Swift PATH + Rust fallback + `KGET_YTDLP_PATH` + mensagem)
- [ ] P0-6 yt-dlp: stderr drenado, progresso, caminho final, áudio `-x`
- [ ] P0-7 Hash sempre verificado; prompt de ISO só com TTY
- [ ] P0-8 Download simples com `.part`, retomada, conferência de `Content-Length`, 429
- [ ] P0-9 Exemplo e doctests compilando
- [ ] P0-10 `KGET_CONFIG_DIR`; doctest `no_run`

**P1**
- [ ] P1-1 JSONL limpo + `Throttle`
- [ ] P1-2 Torrent nativo respeita config, cancela e corrige ETA/JSON
- [ ] P1-3 GUI egui com IDs por job
- [ ] P1-4 `protocol::detect`
- [ ] P1-5 FTP (decode, SIZE opcional, timeouts, REST)
- [ ] P1-6 SFTP (timeouts, `key_path`, `~`, known_hosts)
- [ ] P1-7 Histórico atômico, com lock, sem credenciais
- [ ] P1-8 Config com `serde(default)`, atômico, 0600
- [ ] P1-9 Batch com `--jobs` e nomes únicos
- [ ] P1-10 Erros HTTP estruturados no builder; velocidade média
- [ ] P1-11 Metalink v3 + SHA-512/MD5 + `~`
- [ ] P1-12 Sem brotli automático; `sanitize_filename`; decode de `%xx`
- [ ] P1-13 Caminho real para ISO/extract/histórico
- [ ] P1-14 WebDAV via `url::Url`; app aceita `webdav://`
- [ ] P1-15 a P1-20 Correções do app macOS
- [ ] P1-21 Transmission sem limite de 30 min nem navegador automático
- [ ] P1-22 Interativo com `shlex`, `~` e URL colada

**P2 e features**
- [ ] P2-1 a P2-18 (tabela da seção 4)
- [ ] F-1 a F-10
- [ ] Versões atualizadas (7.2), CHANGELOG (7.3), teste manual (7.4)
