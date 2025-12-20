# Usando o KGet como uma Crate (Biblioteca)

O KGet é um motor de download de alta performance para Rust. Ele oferece recursos avançados como divisão em chunks paralelos, I/O em stream e proteção da integridade do disco (Escritas Bufferizadas), tudo encapsulado em uma API amigável para o desenvolvedor.

[English](../LIB.md) | [Português](LIB.pt-br.md) | [Español](LIB.es.md)

## Instalação

Adicione o KGet ao seu `Cargo.toml`:

Sem GUI (recomendado para servidores/CI/builds mínimos):

```toml
[dependencies]
kget = "1.5.2"
```

Com GUI ativada (isso puxa dependências opcionais de GUI):

```toml
[dependencies]
kget = { version = "1.5.2", features = ["gui"] }
```

## Componentes Principais
A biblioteca expõe os seguintes blocos fundamentais:
- `download`: Função padrão para transferências de fluxo único (HTTP/HTTPS/FTP/SFTP).
- `AdvancedDownloader`: Uma struct para downloads paralelos multi-thread com otimização automática de RAM/Disco.
- `DownloadOptions`: Controla o comportamento da biblioteca (Modo silencioso, Caminho de saída, Verificação de ISO).
- `create_progress_bar`: Fábrica para criar barras de progresso no estilo KGet (verde, fluida, com ETA).
- `verify_iso_integrity`: Utilitário independente para cálculo de checksum SHA256.
- `Config` / `Optimizer`: Gerenciamento completo de configurações.

## Guia Prático (Cookbook)
A melhor forma de aprender é consultando nosso exemplo completo no [Livro de receitas](src/lib_usage.rs).

## Exemplo: Integração Simples

```rust
use kget::{download, DownloadOptions, Config, Optimizer};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = Config::default();
    let options = DownloadOptions {
        output_path: Some("arquivo_customizado.zip".into()),
        verify_iso: false,
        quiet_mode: false,
    };

    // Chamada simples de uma linha para o motor
    download("https://example.com/file.zip", config.proxy, Optimizer::new(config.optimization), options)?;
    Ok(())
}
```

## Comportamento: Biblioteca vs CLI
Para garantir que o KGet funcione perfeitamente como uma biblioteca, seguimos estas regras:
1. Sem Prompts de Stdin: As funções da biblioteca nunca usam `stdin`. Elas não pausarão seu programa para fazer perguntas.
2. Controle Programático: Use `DownloadOptions { verify_iso: true }` para forçar a verificação, ou `false` para ignorá-la.
3. Otimizado por Padrão: Mesmo como lib, o KGet usa `BufWriter` de 2MB por thread e streaming de 16KB para garantir que o sistema hospedeiro continue responsivo e o uso de RAM seja baixo (~30MB).

## Protocolos Suportados

- HTTP/HTTPS
- FTP
- SFTP
- Links Magnet (torrents, requer `transmission-daemon`)

## Mais

Veja [docs.rs/kget](https://docs.rs/kget) para a documentação completa da API.

---

-------------------------
KGet é construído com ❤️ em Rust para velocidade e confiabilidade.