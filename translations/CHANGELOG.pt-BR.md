# Registro de Alterações

[English](../CHANGELOG.md) | [Português](CHANGELOG.pt-BR.md) | [Español](CHANGELOG.es.md)

Todas as mudanças notáveis neste projeto serão documentadas neste arquivo.

O formato é baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.0.0/),
e este projeto adere ao [Versionamento Semântico](https://semver.org/lang/pt-BR/).

## [0.1.4] - 2025-05-22

### Adicionado
- Interface Gráfica do Usuário (GUI) para downloads mais fáceis.
- Suporte a download via FTP.
- Suporte a download via SFTP (autenticação baseada em senha e chave).
- Suporte a download de Torrent via links magnéticos (integra com o daemon Transmission).
- Instruções detalhadas para configuração do daemon Transmission no README.

### Modificado
- Refinada a determinação do caminho de saída para alinhar o comportamento com o `wget`.
- Garantido que `final_path` seja sempre absoluto para prevenir erros de "Arquivo ou diretório não encontrado" no CWD (diretório de trabalho atual).
- Atualizado o README em Inglês, Português e Espanhol para refletir todas as novas funcionalidades e instruções de configuração.

### Corrigido
- Resolvido erro "Arquivo ou diretório não encontrado" ao baixar sem `-O`, garantindo caminhos absolutos.
- Corrigido `validate_filename` para verificar apenas o nome base do arquivo, não o caminho completo.
- Abordados problemas potenciais com `map_err` em `main.rs` para downloads de torrent e HTTP.

## [0.1.3] - 2025-03-11

### Adicionado
- Modo de download avançado com chunks paralelos e capacidade de retomada
- Suporte a compressão automática (gzip, brotli, lz4)
- Sistema de cache inteligente para downloads repetidos mais rápidos
- Limitação de velocidade e controle de conexão
- Suporte a documentação em múltiplos idiomas

### Modificado
- Melhorado o tratamento de erros e feedback ao usuário
- Aprimorada a barra de progresso com informações mais detalhadas
- Otimizado o uso de memória para downloads de arquivos grandes
- Atualizado o sistema de configuração de proxy

### Corrigido
- Corrigidos problemas de autenticação de proxy
- Resolvidos problemas de criação de diretório de cache
- Corrigido o tratamento de níveis de compressão
- Corrigido o tratamento de caminhos de arquivo no Windows

### Segurança
- Adicionado tratamento seguro de conexões proxy
- Melhorada a validação de URLs
- Aprimorada a sanitização de nomes de arquivo
- Adicionada verificação de espaço antes dos downloads

## [0.1.2] - 2025-03-10

### Adicionado
- Suporte a proxy (HTTP, HTTPS, SOCKS5)
- Autenticação de proxy
- Nomeação personalizada de arquivos de saída
- Detecção de tipo MIME

### Modificado
- Melhorado o cálculo de velocidade de download
- Aprimorada a exibição da barra de progresso
- Melhores mensagens de erro
- Documentação atualizada

### Corrigido
- Corrigidos problemas de timeout de conexão
- Resolvidos problemas de permissão de arquivos
- Corrigida a análise de URLs
- Corrigida a exibição da barra de progresso no Windows

## [0.1.1] - 2025-03-09

### Adicionado
- Modo silencioso para integração com scripts
- Barra de progresso básica
- Exibição do tamanho do arquivo
- Monitoramento de velocidade de download

### Modificado
- Melhorado o tratamento de erros
- Aprimorada a interface de linha de comando
- Melhor manipulação de arquivos
- Atualizadas as instruções de instalação

### Corrigido
- Corrigidos problemas de manipulação de caminhos
- Resolvidos problemas de permissão
- Corrigida a exibição de progresso
- Corrigido o comportamento de sobrescrita de arquivos

## [0.1.0] - 2025-03-08

### Adicionado
- Lançamento inicial
- Funcionalidade básica de download de arquivos
- Interface de linha de comando
- Tratamento básico de erros
- Suporte multiplataforma