FROM rust:slim

RUN apt-get update && apt-get install -y \
    git \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt
RUN rustup component add clippy