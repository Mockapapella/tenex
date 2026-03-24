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
    libnss-wrapper \
    libssl-dev \
    nodejs \
    npm \
    openssh-client \
    pkg-config \
    python3 \
    python3-pip \
 && libnss_wrapper_path="$(find /usr/lib -name libnss_wrapper.so -print -quit)" \
 && [ -n "$libnss_wrapper_path" ] \
 && ln -sf "$libnss_wrapper_path" /usr/local/lib/libnss_wrapper.so \
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
