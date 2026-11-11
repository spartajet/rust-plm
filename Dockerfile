# RustPLM 服务端镜像（多阶段；骨架版：debian 而非 musl，TLS 反代见 ADR-0014 后续增量）
FROM rust:1-slim AS build
WORKDIR /build
# 依赖层缓存：先拷清单
COPY Cargo.toml Cargo.lock ./
COPY crates/core-model/Cargo.toml crates/core-model/Cargo.toml
COPY crates/core-files/Cargo.toml crates/core-files/Cargo.toml
COPY crates/mod-doc/Cargo.toml crates/mod-doc/Cargo.toml
COPY crates/api-gateway/Cargo.toml crates/api-gateway/Cargo.toml
COPY crates/plm-server/Cargo.toml crates/plm-server/Cargo.toml
COPY spikes/metadata-spike/Cargo.toml spikes/metadata-spike/Cargo.toml
COPY spikes/bom-spike/Cargo.toml spikes/bom-spike/Cargo.toml
RUN mkdir -p crates/core-model/src crates/core-files/src crates/mod-doc/src \
            crates/api-gateway/src crates/plm-server/src \
            spikes/metadata-spike/src spikes/bom-spike/src \
 && echo "" > crates/core-model/src/lib.rs && echo "" > crates/core-files/src/lib.rs \
 && echo "" > crates/mod-doc/src/lib.rs && echo "" > crates/api-gateway/src/lib.rs \
 && echo "fn main(){}" > crates/plm-server/src/main.rs \
 && echo "" > spikes/metadata-spike/src/main.rs && echo "" > spikes/bom-spike/src/main.rs
# 迁移目录也被 sqlx::migrate! 在编译期读取
COPY crates/core-model/migrations crates/core-model/migrations
COPY crates/mod-doc/migrations crates/mod-doc/migrations
RUN cargo build --release -p plm-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /build/target/release/plm-server /app/plm-server
WORKDIR /app
ENV PLM_DATA_DIR=/data
EXPOSE 8080
VOLUME /data
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD curl -fsS http://localhost:8080/healthz || exit 1
ENTRYPOINT ["/app/plm-server"]
