# syntax=docker/dockerfile:1.7
#
# One image with the game server and the built web client.
#
#   docker build -t isle-settlers .
#   docker run -p 8080:8080 -v isle-data:/data isle-settlers
#
# Behind a TLS-intercepting proxy, pass its CA certificate as a build secret:
#   docker build --secret id=ca,src=/path/to/ca.crt -t isle-settlers .

ARG RUST_VERSION=1.97
ARG NODE_VERSION=22

# ---------------------------------------------------------------- Rust
FROM rust:${RUST_VERSION}-bookworm AS rust
WORKDIR /src
RUN --mount=type=secret,id=ca,required=false \
    if [ -s /run/secrets/ca ]; then \
      cp /run/secrets/ca /usr/local/share/ca-certificates/build-ca.crt && update-ca-certificates; \
    fi
RUN rustup target add wasm32-unknown-unknown \
 && cargo install wasm-bindgen-cli --version 0.2.130 --locked
# rust-toolchain.toml is not copied: the image's toolchain is used as is.
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p isle-server \
 && cargo build --locked -p isle-wasm --target wasm32-unknown-unknown --profile wasm-release \
 && mkdir -p /out \
 && cp target/release/isle-server /out/ \
 && wasm-bindgen target/wasm32-unknown-unknown/wasm-release/isle_wasm.wasm --out-dir /out/wasm --target web

# ---------------------------------------------------------------- web
FROM node:${NODE_VERSION}-bookworm-slim AS web
WORKDIR /src
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY web/package.json web/
RUN --mount=type=secret,id=ca,required=false \
    if [ -s /run/secrets/ca ]; then export NODE_EXTRA_CA_CERTS=/run/secrets/ca; fi \
 && npm install -g pnpm@10.28.0 \
 && pnpm install --frozen-lockfile
COPY web web
COPY --from=rust /out/wasm web/src/wasm
RUN node_modules/.bin/wasm-opt -Os --enable-bulk-memory --enable-nontrapping-float-to-int \
      --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue \
      web/src/wasm/isle_wasm_bg.wasm -o web/src/wasm/isle_wasm_bg.wasm \
 && pnpm --filter @isle/web build

# ---------------------------------------------------------------- runtime
FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --home-dir /app --shell /usr/sbin/nologin isle \
 && mkdir -p /data && chown isle /data
COPY --from=rust /out/isle-server /usr/local/bin/isle-server
COPY --from=web /src/web/dist /app/web
USER isle
ENV ISLE_DATA_DIR=/data \
    RUST_LOG=info
VOLUME /data
EXPOSE 8080
CMD ["isle-server", "--static-dir", "/app/web"]
