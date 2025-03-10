# Guia de Contribuição para o KelpsGet

Obrigado por considerar contribuir com o KelpsGet! Este documento fornece diretrizes e informações importantes para contribuidores.

## Índice

1. [Código de Conduta](#código-de-conduta)
2. [Como Posso Contribuir?](#como-posso-contribuir)
3. [Processo de Desenvolvimento](#processo-de-desenvolvimento)
4. [Estilo de Código](#estilo-de-código)
5. [Commits e Pull Requests](#commits-e-pull-requests)
6. [Reportando Bugs](#reportando-bugs)

## Código de Conduta

Este projeto segue um Código de Conduta. Ao participar, você deve seguir este código. Por favor, leia [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Como Posso Contribuir?

### 🐛 Reportando Bugs
- Use o template de issue para bugs
- Inclua passos detalhados para reproduzir
- Forneça informações do ambiente (OS, versão do Rust, etc.)
- Inclua logs de erro relevantes

### 💡 Sugerindo Melhorias
- Use o template de issue para features
- Explique o caso de uso
- Descreva o comportamento esperado
- Forneça exemplos de uso

### 📝 Documentação
- Corrija erros de digitação
- Adicione exemplos de uso
- Melhore explicações
- Traduza documentação

### 👩‍💻 Código
- Implemente novas features
- Corrija bugs
- Melhore performance
- Adicione testes

## Processo de Desenvolvimento

1. **Fork o repositório**
2. **Clone seu fork**
```bash
git clone https://github.com/seu-usuario/KelpsGet.git
cd KelpsGet
```

3. **Crie uma branch**
```bash
git checkout -b feature/sua-feature
# ou
git checkout -b fix/seu-fix
```

4. **Desenvolva**
- Escreva testes
- Siga o estilo de código
- Mantenha commits atômicos

5. **Teste**
```bash
cargo test
cargo clippy
cargo fmt --all -- --check
```

6. **Push e Pull Request**
```bash
git push origin feature/sua-feature
```

## Estilo de Código

### Rust
- Siga o [Rust Style Guide](https://rust-lang.github.io/api-guidelines/)
- Use `rustfmt` e `clippy`
- Documente funções públicas
- Escreva testes unitários

### Commits
- Use commits atômicos
- Siga o padrão Conventional Commits:
  - `feat:` nova funcionalidade
  - `fix:` correção de bug
  - `docs:` documentação
  - `test:` testes
  - `refactor:` refatoração
  - `style:` formatação
  - `chore:` manutenção

### Exemplo de Commit
```
feat: adiciona suporte a proxy SOCKS5

- Implementa cliente SOCKS5
- Adiciona testes de integração
- Atualiza documentação
```

## Commits e Pull Requests

### Pull Request
- Use o template fornecido
- Referencie issues relacionadas
- Descreva as mudanças
- Inclua testes
- Atualize documentação

### Revisão
- Responda a feedback prontamente
- Mantenha discussões construtivas
- Faça squash de commits quando necessário

## Reportando Bugs

### Template de Bug
```markdown
**Descrição**
[Descrição clara e concisa do bug]

**Para Reproduzir**
1. Faça '...'
2. Execute '...'
3. Veja erro

**Comportamento Esperado**
[O que deveria acontecer]

**Logs**
```rust
[Coloque logs aqui]
```

**Ambiente**
- OS: [ex: Ubuntu 20.04]
- Rust: [ex: 1.70.0]
- KelpsGet: [ex: 0.1.3]


## Dúvidas?

- Abra uma issue
- Envie um email para [davimoreiraf@gmail.com]

---

Agradecemos suas contribuições! 🚀
