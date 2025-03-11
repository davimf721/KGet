# KelpsGet v0.1.3

Um clone moderno e leve do wget escrito em Rust para downloads de arquivos rápidos e confiáveis a partir da linha de comando.

[English](../README.md) | [Português](README.pt-BR.md) | [Español](README.es.md)

## Recursos
✅ Ferramenta CLI simples para download de arquivos via HTTP/HTTPS.<br>
✅ Barra de progresso com velocidade em tempo real e tempo estimado.<br>
✅ Nomes de saída personalizados (flag -O para renomear arquivos).<br>
✅ Detecção de tipo MIME e tratamento adequado de arquivos.<br>
✅ Multiplataforma (Linux, macOS, Windows).<br>
✅ Modo silencioso para scripts.<br>
✅ Verificação automática de espaço antes do download.<br>
✅ Tentativa automática de reconexão em caso de falha.<br>
✅ Validação de nome de arquivo.<br>
✅ Suporte para diferentes tipos MIME.<br>
✅ Exibição detalhada de informações do download.<br>
✅ Modo de download avançado com chunks paralelos e capacidade de retomada.<br>
✅ Suporte a proxy (HTTP, HTTPS, SOCKS5).<br>
✅ Compressão e cache automáticos.<br>
✅ Limitação de velocidade e controle de conexão.<br>

## Instalação
### Opção 1: Instalação via Cargo
```bash
cargo install kelpsget
```
### Opção 2: Baixe os Binários Pré-compilados
Baixe o binário mais recente para seu sistema operacional em [Release](https://github.com/davimf721/KelpsGet/releases)

### Linux/macOS:
```bash
chmod +x kelpsget  # Tornar executável
./kelpsget [URL]   # Executar diretamente
```
### Windows:
Execute o arquivo .exe diretamente.

## Exemplos de Uso
Download Básico:
```bash
kelpsget https://exemplo.com/arquivo.txt
```
Renomear o Arquivo de Saída:
```bash
kelpsget -O novo_nome.txt https://exemplo.com/arquivo.txt
```
Modo Silencioso:
```bash
kelpsget -q https://exemplo.com/arquivo.txt
```
Modo de Download Avançado (Paralelo e Resumível):
```bash
kelpsget -a https://exemplo.com/arquivo_grande.zip
```
Usando Proxy:
```bash
kelpsget -p http://proxy:8080 https://exemplo.com/arquivo.txt
```
Com Autenticação de Proxy:
```bash
kelpsget -p http://proxy:8080 --proxy-user usuario --proxy-pass senha https://exemplo.com/arquivo.txt
```
Limitação de Velocidade:
```bash
kelpsget -l 1048576 https://exemplo.com/arquivo.txt  # Limite de 1MB/s
```
Desabilitar Compressão:
```bash
kelpsget --no-compress https://exemplo.com/arquivo.txt
```
Desabilitar Cache:
```bash
kelpsget --no-cache https://exemplo.com/arquivo.txt
```

## Como Funciona
1. Barra de Progresso: Mostra velocidade de download, tempo estimado e bytes transferidos.
2. Nomeação Inteligente de Arquivos:
  - Usa o nome do arquivo da URL (ex: arquivo.txt de https://exemplo.com/arquivo.txt).
  - Usa index.html como padrão se a URL terminar com /.
3. Tratamento de Erros: Sai com código 1 em erros HTTP (ex: 404).
4. Verificação de Espaço: Verifica espaço disponível em disco antes do download.
5. Retry Automático: Tenta novamente o download em caso de falha na conexão.
6. Modo de Download Avançado:
  - Download em chunks paralelos para melhor performance
  - Suporta retomada de downloads interrompidos
  - Trata arquivos grandes de forma eficiente
7. Suporte a Proxy:
  - Suporte a proxy HTTP, HTTPS e SOCKS5
  - Autenticação de proxy
  - Configurações flexíveis de proxy
8. Recursos de Otimização:
  - Compressão automática (gzip, brotli, lz4)
  - Cache de arquivos para downloads repetidos mais rápidos
  - Limitação de velocidade
  - Controle de conexão

## Configuração
O KelpsGet usa um arquivo de configuração localizado em:
- Windows: `%APPDATA%\kelpsget\config.json`
- Linux/macOS: `~/.config/kelpsget/config.json`

Exemplo de configuração:
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
    "compression": true,
    "compression_level": 6,
    "cache_enabled": true,
    "cache_dir": "~/.cache/kelpsget",
    "speed_limit": null,
    "max_connections": 4
  }
}
```

## Recursos de Segurança
- Verificação de Espaço: Garante espaço suficiente em disco antes do download.
- Validação de Nome de Arquivo: Previne injeção de caminho.
- Tratamento de URL: Trata URLs de forma segura.
- Retry Automático: Tenta novamente em caso de falha na rede.
- Suporte Seguro a Proxy: Conexões proxy criptografadas.

## Contribuindo
Encontrou um bug ou quer adicionar uma funcionalidade? Abra uma issue ou envie um PR!

🚀 Faça downloads sem esforço com a velocidade e confiabilidade do Rust. 🚀

## 🔗 Links Importantes
- 📚 [Documentação](https://davimf721.github.io/KelpsGet/)
- 📦 [crates.io](https://crates.io/crates/kelpsget)
- 💻 [GitHub](https://github.com/davimf721/KelpsGet)
- 📝 [Changelog](CHANGELOG.md)

## 🎯 Próximos Passos
Estamos trabalhando nas seguintes melhorias:

- [ ] Suporte a downloads via FTP/SFTP
- [ ] Interface web para monitoramento de downloads
- [ ] Integração com serviços de cloud storage
- [ ] Sistema de plugins personalizados
- [ ] Suporte a downloads via torrent
- [ ] Melhorias na compressão adaptativa
- [ ] Otimização do sistema de cache
- [ ] Suporte a mais protocolos de proxy
- [ ] Interface gráfica desktop (GUI)
- [ ] Documentação em múltiplos idiomas

Quer contribuir com alguma dessas funcionalidades? Confira nosso [guia de contribuição](CONTRIBUTING.md)!

## Licença
Este projeto está licenciado sob a Licença MIT - veja o arquivo [LICENSE](LICENSE) para detalhes. 