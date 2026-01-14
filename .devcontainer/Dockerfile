FROM mcr.microsoft.com/devcontainers/dotnet:9.0

RUN apt-get update && apt-get install -y \
    git \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Install nvm and Node.js
RUN curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
RUN bash -i -c 'nvm install 24'
RUN bash -i -c 'nvm use 24 && nvm alias default 24'

# Install kubectl
RUN curl -LO "https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl" \
    && curl -LO "https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl.sha256" \
    && echo "$(cat kubectl.sha256)  kubectl" | sha256sum --check \
    && install -o root -g root -m 0755 kubectl /usr/local/bin/kubectl \
    && rm -rf kubectl kubectl.sha256

# Install Helm
RUN curl -O https://get.helm.sh/helm-v4.0.0-linux-amd64.tar.gz \
    && tar -zxvf helm-v4.0.0-linux-amd64.tar.gz \
    && mv linux-amd64/helm /usr/local/bin/helm \
    && rm -rf helm-v4.0.0-linux-amd64.tar.gz linux-amd64

# Install NATS CLI
RUN curl -sSL https://github.com/nats-io/natscli/releases/download/v0.3.0/nats-0.3.0-linux-amd64.zip -o nats.zip \
    && unzip nats.zip \
    && mv nats-0.3.0-linux-amd64/nats /usr/local/bin/nats \
    && rm -rf nats.zip nats-0.3.0-linux-amd64