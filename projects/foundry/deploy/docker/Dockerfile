# Build from the repository root: the workspace spans projects/*/crates and
# projects/*/services.
FROM rust:1.93.1-slim-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo ./.cargo
COPY projects/foundry ./projects/foundry
RUN cargo build --release -p foundry-control-plane

# The workflow editor is a separate SPA; its dist/ is not part of cargo.
FROM node:22-bookworm-slim AS editor
WORKDIR /src/web/workflow-editor
COPY projects/foundry/web/workflow-editor/package.json \
     projects/foundry/web/workflow-editor/package-lock.json ./
RUN npm ci
COPY projects/foundry/web/workflow-editor/ ./
RUN npm run build

# Runtime
FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home foundry
COPY --from=builder /src/target/release/foundry-control-plane /usr/local/bin/foundry-control-plane
COPY --from=editor /src/web/workflow-editor/dist /usr/local/share/foundry-editor
USER foundry
EXPOSE 8080
ENV FOUNDRY_ADDR=0.0.0.0:8080 \
    FOUNDRY_EDITOR_DIR=/usr/local/share/foundry-editor \
    RUST_LOG=foundry=info,tower_http=info
ENTRYPOINT ["foundry-control-plane"]
