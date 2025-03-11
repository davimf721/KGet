# Changelog

Todas as mudanças notáveis neste projeto serão documentadas neste arquivo.

O formato é baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.0.0/),
e este projeto adere a [Semantic Versioning](https://semver.org/lang/pt-BR/).

## [0.1.3] - 2024-03-11

### Adicionado
- Sistema de compressão adaptativa
  - Gzip (níveis 1-3)
  - LZ4 (níveis 4-6)
  - Brotli (níveis 7-9)
- Sistema de cache inteligente
- Suporte a proxy HTTP/HTTPS/SOCKS5
- Autenticação de proxy
- Download paralelo
- Suporte a retomada de downloads

### Modificado
- Melhorada a barra de progresso
- Otimizado o sistema de chunks
- Melhorado o tratamento de erros

### Corrigido
- Bugs na manipulação de arquivos grandes
- Problemas com nomes de arquivo especiais
- Erros de timeout em downloads lentos

## [0.1.2] - 2024-03-10

### Adicionado
- Suporte a downloads resumíveis
- Verificação de espaço em disco
- Validação de nomes de arquivo

### Modificado
- Melhorado o sistema de retry
- Otimizada a performance geral

### Corrigido
- Bugs na barra de progresso
- Problemas com URLs especiais

## [0.1.1] - 2024-03-09

### Adicionado
- Suporte a múltiplos URLs
- Modo silencioso
- Renomeação de arquivos de saída

### Modificado
- Melhorada a interface CLI
- Otimizada a performance

### Corrigido
- Bugs de inicialização
- Problemas com caracteres especiais

## [0.1.0] - 2024-03-08

### Adicionado
- Download básico de arquivos
- Barra de progresso
- Detecção de tipo MIME
- Verificação de espaço em disco
- Retry automático em falhas
- Suporte a diferentes tipos MIME
- Informações detalhadas de download
- Modo avançado com chunks paralelos
- Suporte a proxy
- Compressão automática
- Sistema de cache
- Controle de velocidade
- Controle de conexão

### Modificado
- Interface CLI otimizada
- Sistema de progresso melhorado
- Tratamento de erros aprimorado

### Corrigido
- Bugs de inicialização
- Problemas com URLs especiais
- Erros de timeout
- Bugs na barra de progresso
- Problemas com caracteres especiais
- Bugs na manipulação de arquivos grandes
- Erros de timeout em downloads lentos
- Problemas com nomes de arquivo especiais