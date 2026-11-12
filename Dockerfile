# RustPLM 服务端镜像（多阶段；骨架版：debian 而非 musl，TLS 反代见 ADR-0014 后续增量）
FROM rust:1-slim AS build
WORKDIR /build
# ① 依赖层缓存：先拷全部清单 + 桩源码，预编译依赖
# （workspace 成员清单须与 Cargo.toml members 全集一致——新增 crate 必须同步此层，CI docker job 已实证）
COPY Cargo.toml Cargo.lock ./
COPY crates/core-model/Cargo.toml crates/core-model/Cargo.toml
COPY crates/core-files/Cargo.toml crates/core-files/Cargo.toml
COPY crates/mod-doc/Cargo.toml crates/mod-doc/Cargo.toml
COPY crates/core-sec/Cargo.toml crates/core-sec/Cargo.toml
COPY crates/core-version/Cargo.toml crates/core-version/Cargo.toml
COPY crates/core-events/Cargo.toml crates/core-events/Cargo.toml
COPY crates/core-bom/Cargo.toml crates/core-bom/Cargo.toml
COPY crates/api-gateway/Cargo.toml crates/api-gateway/Cargo.toml
COPY crates/plm-server/Cargo.toml crates/plm-server/Cargo.toml
COPY spikes/metadata-spike/Cargo.toml spikes/metadata-spike/Cargo.toml
COPY spikes/bom-spike/Cargo.toml spikes/bom-spike/Cargo.toml
RUN mkdir -p crates/core-model/src crates/core-files/src crates/mod-doc/src \
            crates/core-sec/src crates/core-version/src crates/core-events/src crates/core-bom/src \
            crates/api-gateway/src crates/plm-server/src \
            spikes/metadata-spike/src spikes/bom-spike/src \
 && for d in core-model core-files mod-doc core-sec core-version core-events core-bom api-gateway; \
      do echo "" > crates/$d/src/lib.rs; done \
 && echo "fn main(){}" > crates/plm-server/src/main.rs \
 && echo "" > spikes/metadata-spike/src/main.rs && echo "" > spikes/bom-spike/src/main.rs \
 && cargo build --release -p plm-server

# ② 真实源码 + 迁移目录（sqlx::migrate! 编译期读取）→ 重建（缓存命中则只重编业务 crate）。
# touch 全部 .rs：COPY 保留构建上下文时戳（早于①桩层 RUN 时戳），cargo 会误判桩产物仍新鲜而跳过重编
COPY crates ./crates
RUN find crates -name '*.rs' -exec touch {} + && cargo build --release -p plm-server

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
