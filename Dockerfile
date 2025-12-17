FROM rust:1.92.0

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    curl \
    git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copia apenas manifestos (cache)
COPY Cargo.toml Cargo.lock ./

# Cria arquivos fake para satisfazer lib + bin
RUN mkdir src \
 && echo "pub fn fake() {}" > src/lib.rs \
 && echo "fn main() {}" > src/main.rs

# Baixa e compila dependências
RUN cargo build

# Remove código fake
RUN rm -rf src

# Código real vem via volume (docker-compose)
CMD ["cargo", "run"]
