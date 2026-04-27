# Guia de Contribuição para o KGet

[English](../CONTRIBUTING.md) | [Português](CONTRIBUTING.pt-BR.md) | [Español](CONTRIBUTING.es.md)

Primeiramente, obrigado por considerar contribuir com o KGet! São pessoas como você que tornam o KGet uma ferramenta incrível.

## Código de Conduta

Este projeto e todos os seus participantes são governados pelo nosso [Código de Conduta](../CODE_OF_CONDUCT.md). Ao participar, espera-se que você siga este código. Por favor, reporte comportamentos inaceitáveis para [davimf721@gmail.com](mailto:davimf721@gmail.com).

## Como Posso Contribuir?

### Reportando Bugs

Antes de criar relatórios de bugs, por favor verifique as issues existentes, pois você pode descobrir que não precisa criar uma nova. Quando você estiver criando um relatório de bug, por favor inclua o máximo de detalhes possível:

* Use um título claro e descritivo
* Descreva os passos exatos que reproduzem o problema
* Forneça exemplos específicos para demonstrar os passos
* Descreva o comportamento observado após seguir os passos
* Explique qual comportamento você esperava ver e por quê
* Inclua capturas de tela se possível
* Inclua a versão do KGet que você está usando
* Inclua seu sistema operacional e versão

### Sugerindo Melhorias

Se você tem uma sugestão para o projeto, adoraríamos ouvir! Basta seguir estes passos:

* Use um título claro e descritivo
* Forneça uma descrição passo a passo da melhoria sugerida
* Forneça exemplos específicos para demonstrar os passos
* Descreva o comportamento atual e explique qual comportamento você esperava ver
* Explique por que essa melhoria seria útil para a maioria dos usuários do KGet

### Pull Requests

* Preencha o template necessário
* Não inclua números de issues no título do PR
* Inclua capturas de tela e GIFs animados em seu pull request sempre que possível
* Siga o guia de estilo do Rust
* Inclua testes bem estruturados e bem documentados
* Documente o novo código
* Termine todos os arquivos com uma nova linha

## Processo de Desenvolvimento

1. Faça um fork do repositório
2. Clone seu fork: `git clone https://github.com/seu-usuario/KGet.git`
3. Crie sua branch de feature: `git checkout -b feature/minha-nova-feature`
4. Faça suas alterações
5. Execute os testes: `cargo test`
6. Formate seu código: `cargo fmt`
7. Verifique com clippy: `cargo clippy`
8. Faça commit de suas alterações: `git commit -am 'Adiciona alguma feature'`
9. Faça push para a branch: `git push origin feature/minha-nova-feature`
10. Envie um pull request

## Desenvolvimento com Docker (recomendado para contribuintes)

Fornecemos um `Dockerfile` e um `docker-compose.yml` para tornar o desenvolvimento reproduzível entre máquinas. O contêiner inclui a toolchain do Rust e ferramentas comuns para que contribuintes possam compilar, testar e executar exemplos sem instalar dependências localmente.

Fluxo básico

```bash
# Construir a imagem de desenvolvimento
docker build -t kget-dev .

# Iniciar um shell interativo mapeado para o repositório (Linux/macOS)
docker run --rm -it -v "$(pwd)":/work -w /work kget-dev bash

# Windows PowerShell
docker run --rm -it -v ${PWD}:/work -w /work kget-dev powershell
```

Comandos comuns sem entrar no container:

```bash
# Build
docker run --rm -v "$(pwd)":/work -w /work kget-dev cargo build

# Rodar testes
docker run --rm -v "$(pwd)":/work -w /work kget-dev cargo test

# Executar o exemplo que demonstra uso como biblioteca
docker run --rm -v "$(pwd)":/work -w /work kget-dev cargo run --example lib_usage
```

Usando `docker-compose`:

```bash
docker-compose up --build
```

Observações e dicas

- A imagem de desenvolvimento foca em fluxo CLI, CI e testes. Executar a GUI dentro de um container exige X11/Wayland ou encaminhamento específico da plataforma (não ativado por padrão).
- Para experimentar a GUI a partir de um container no Linux, encaminhe o X11 e construa/executar com a feature `gui`:

```bash
# Construir a imagem com GUI (opcional)
docker build -t kget-gui .

# Executar com encaminhamento X11 (exemplo Linux)
docker run --rm -it \
    -e DISPLAY=$DISPLAY \
    -v /tmp/.X11-unix:/tmp/.X11-unix \
    -v "$(pwd)":/work -w /work kget-gui cargo run --features gui -- --gui
```

- O mount de volume (`-v "$(pwd)":/work`) permite editar arquivos no host e compilar/testar no container, mantendo consistência com CI e outros contribuintes.

## Guias de Estilo

### Mensagens de Commit do Git

* Use o tempo presente ("Adiciona feature" não "Adicionada feature")
* Use o modo imperativo ("Mover cursor para..." não "Move cursor para...")
* Limite a primeira linha a 72 caracteres ou menos
* Referencie issues e pull requests livremente após a primeira linha
* Considere começar a mensagem do commit com um emoji aplicável:
    * 🎨 `:art:` ao melhorar o formato/estrutura do código
    * 🐎 `:racehorse:` ao melhorar a performance
    * 🚱 `:non-potable_water:` ao corrigir memory leaks
    * 📝 `:memo:` ao escrever documentação
    * 🐛 `:bug:` ao corrigir um bug
    * 🔥 `:fire:` ao remover código ou arquivos
    * 💚 `:green_heart:` ao corrigir o build do CI
    * ✅ `:white_check_mark:` ao adicionar testes
    * 🔒 `:lock:` ao lidar com segurança
    * ⬆️ `:arrow_up:` ao atualizar dependências
    * ⬇️ `:arrow_down:` ao fazer downgrade de dependências

### Guia de Estilo do Rust

* Use `cargo fmt` para formatar seu código
* Siga as [Diretrizes da API do Rust](https://rust-lang.github.io/api-guidelines/)
* Use nomes de variáveis significativos
* Escreva documentação para APIs públicas
* Adicione testes para novas funcionalidades
* Mantenha as funções pequenas e focadas
* Use tratamento de erros em vez de pânicos
* Siga as convenções de nomenclatura da biblioteca padrão

### Guia de Estilo da Documentação

* Use [Markdown](https://daringfireball.net/projects/markdown/) para documentação
* Referencie funções, classes e módulos em backticks
* Use links de seção ao se referir a outras partes da documentação
* Inclua exemplos de código quando possível
* Mantenha o comprimento da linha em no máximo 80 caracteres
* Use textos descritivos para links em vez de "clique aqui"

## Notas Adicionais

### Etiquetas de Issues e Pull Requests

* `bug` - Algo não está funcionando
* `melhoria` - Nova feature ou solicitação
* `documentação` - Melhorias ou adições à documentação
* `boa primeira issue` - Bom para iniciantes
* `precisa-se de ajuda` - Precisa de atenção extra
* `dúvida` - Mais informações são solicitadas
* `inválida` - Algo está errado
* `não será corrigido` - Não será trabalhado

## Reconhecimento

Contribuidores que enviarem um pull request válido serão adicionados ao nosso arquivo [CONTRIBUTORS.md](../CONTRIBUTORS.md).

Obrigado por contribuir com o KGet! 🚀
