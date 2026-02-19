FROM rust:slim

RUN apt-get update && apt-get install -y \
    git \
    curl \
    unzip \
    && rm -rf /var/lib/apt/lists/*

# Install NATS server
RUN curl -fsSL https://github.com/nats-io/nats-server/releases/download/v2.10.24/nats-server-v2.10.24-linux-amd64.tar.gz \
    | tar -xz -C /usr/local/bin --strip-components=1 nats-server-v2.10.24-linux-amd64/nats-server

# Install NATS CLI
RUN curl -fsSL https://github.com/nats-io/natscli/releases/download/v0.1.6/nats-0.1.6-linux-amd64.zip -o /tmp/nats-cli.zip \
    && unzip /tmp/nats-cli.zip -d /tmp/nats-cli \
    && mv /tmp/nats-cli/nats-0.1.6-linux-amd64/nats /usr/local/bin/nats \
    && rm -rf /tmp/nats-cli /tmp/nats-cli.zip

RUN rustup component add rustfmt
RUN rustup component add clippy