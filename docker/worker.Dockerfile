FROM rust:1.91-bookworm

ENV DEBIAN_FRONTEND=noninteractive \
    NPM_CONFIG_UPDATE_NOTIFIER=false \
    npm_config_yes=true

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash \
    ca-certificates \
    curl \
    git \
    gh \
    libssl-dev \
    nodejs \
    npm \
    pkg-config \
    python3 \
    python3-pip \
 && rm -rf /var/lib/apt/lists/*

RUN npm install -g @openai/codex @anthropic-ai/claude-code
RUN rustup component add clippy llvm-tools rustfmt
RUN cargo install cargo-llvm-cov --locked --version 0.6.22
RUN python3 -m pip install --no-cache-dir --break-system-packages pre-commit
RUN printf '%s\n' 'export PATH=/usr/local/cargo/bin:$PATH' >/etc/profile.d/tenex-rust-path.sh \
 && chmod 0644 /etc/profile.d/tenex-rust-path.sh \
 && for bin in /usr/local/cargo/bin/*; do ln -sf "$bin" "/usr/local/bin/$(basename "$bin")"; done

WORKDIR /workspace
CMD ["sleep", "infinity"]
