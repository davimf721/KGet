# Contributing to KGet

[English](CONTRIBUTING.md) | [Português](translations/CONTRIBUTING.pt-BR.md) | [Español](translations/CONTRIBUTING.es.md)

First off, thank you for considering contributing to KelpsGet! It's people like you that make KelpsGet such a great tool.

## Code of Conduct

This project and everyone participating in it is governed by our [Code of Conduct](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code. Please report unacceptable behavior to [davimf721@gmail.com](mailto:davimf721@gmail.com).

## How Can I Contribute?

### Reporting Bugs

Before creating bug reports, please check the existing issues as you might find out that you don't need to create one. When you are creating a bug report, please include as many details as possible:

* Use a clear and descriptive title
* Describe the exact steps which reproduce the problem
* Provide specific examples to demonstrate the steps
* Describe the behavior you observed after following the steps
* Explain which behavior you expected to see instead and why
* Include screenshots if possible
* Include the version of KelpsGet you're using
* Include your operating system and version

### Suggesting Enhancements

If you have a suggestion for the project, we'd love to hear about it! Just follow these steps:

* Use a clear and descriptive title
* Provide a step-by-step description of the suggested enhancement
* Provide specific examples to demonstrate the steps
* Describe the current behavior and explain which behavior you expected to see instead
* Explain why this enhancement would be useful to most KelpsGet users

### Pull Requests

* Fill in the required template
* Do not include issue numbers in the PR title
* Include screenshots and animated GIFs in your pull request whenever possible
* Follow the Rust styleguide
* Include thoughtfully-worded, well-structured tests
* Document new code
* End all files with a newline

## Development Process

1. Fork the repository
2. Clone your fork: `git clone https://github.com/your-username/KGet.git`
3. Create your feature branch: `git checkout -b feature/my-new-feature`
4. Make your changes
5. Run the tests: `cargo test`
6. Format your code: `cargo fmt`
7. Check with clippy: `cargo clippy`
8. Commit your changes: `git commit -am 'Add some feature'`
9. Push to the branch: `git push origin feature/my-new-feature`
10. Submit a pull request

## Development with Docker (recommended for contributors)

We provide a `Dockerfile` and `docker-compose.yml` to make development reproducible across machines. The container contains the Rust toolchain and common development tools so contributors can build, test and run examples without installing dependencies on the host.

Basic workflow

```bash
# Build the dev image
docker build -t kget-dev .

# Start an interactive shell bound to the repository (Linux/macOS)
docker run --rm -it -v "$(pwd)":/work -w /work kget-dev bash

# Windows PowerShell
docker run --rm -it -v ${PWD}:/work -w /work kget-dev powershell
```

Run common commands without entering the container shell:

```bash
# Build
docker run --rm -v "$(pwd)":/work -w /work kget-dev cargo build

# Run tests
docker run --rm -v "$(pwd)":/work -w /work kget-dev cargo test

# Run the example that demonstrates library usage
docker run --rm -v "$(pwd)":/work -w /work kget-dev cargo run --example lib_usage
```

Using docker-compose

```bash
docker-compose up --build
```

Notes & tips

- The development image is aimed at CLI development, CI and testing. Running the GUI inside a container requires X11/Wayland or OS-specific GUI forwarding (not enabled by default).
- If you want to try the GUI from a container on Linux, forward X11 and build/run with the `gui` feature:

```bash
# Build image (optional: add feature build-arg if your Dockerfile supports it)
docker build -t kget-gui .

# Run with X11 forwarding (Linux)
docker run --rm -it \
    -e DISPLAY=$DISPLAY \
    -v /tmp/.X11-unix:/tmp/.X11-unix \
    -v "$(pwd)":/work -w /work kget-gui cargo run --features gui -- --gui
```

- Volume mount (`-v "$(pwd)":/work`) lets you edit files on the host and build/test inside the container so CI and contributors see consistent results.

## Styleguides

### Git Commit Messages

* Use the present tense ("Add feature" not "Added feature")
* Use the imperative mood ("Move cursor to..." not "Moves cursor to...")
* Limit the first line to 72 characters or less
* Reference issues and pull requests liberally after the first line
* Consider starting the commit message with an applicable emoji:
    * 🎨 `:art:` when improving the format/structure of the code
    * 🐎 `:racehorse:` when improving performance
    * 🚱 `:non-potable_water:` when plugging memory leaks
    * 📝 `:memo:` when writing docs
    * 🐛 `:bug:` when fixing a bug
    * 🔥 `:fire:` when removing code or files
    * 💚 `:green_heart:` when fixing the CI build
    * ✅ `:white_check_mark:` when adding tests
    * 🔒 `:lock:` when dealing with security
    * ⬆️ `:arrow_up:` when upgrading dependencies
    * ⬇️ `:arrow_down:` when downgrading dependencies

### Rust Styleguide

* Use `cargo fmt` to format your code
* Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
* Use meaningful variable names
* Write documentation for public APIs
* Add tests for new functionality
* Keep functions small and focused
* Use error handling instead of panics
* Follow the standard library's naming conventions

### Documentation Styleguide

* Use [Markdown](https://daringfireball.net/projects/markdown/) for documentation
* Reference functions, classes, and modules in backticks
* Use section links when referring to other parts of the documentation
* Include code examples when possible
* Keep line length to a maximum of 80 characters
* Use descriptive link texts instead of "click here"

## Additional Notes

### Issue and Pull Request Labels

* `bug` - Something isn't working
* `enhancement` - New feature or request
* `documentation` - Improvements or additions to documentation
* `good first issue` - Good for newcomers
* `help wanted` - Extra attention is needed
* `question` - Further information is requested
* `invalid` - Something's wrong
* `wontfix` - This will not be worked on

## Recognition

Contributors who submit a valid pull request will be added to our [CONTRIBUTORS.md](CONTRIBUTORS.md) file.

Thank you for contributing to KelpsGet! 🚀
