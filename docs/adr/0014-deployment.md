# ADR-0014 部署与运行：单二进制 + 内置 rustls + Docker Compose 一键

- 状态：已接受（2026-11-11）
- 关联需求：F3、D-AC、C2（零运维）

## 决策

- 应用单二进制（rust-embed 内嵌前端静态资源）+ PostgreSQL 18 = 最小部署单元；
- **TLS 由 axum 内置 rustls 终止**（不自签自动生成、证书放数据卷；反代为可选项而非依赖）；
- Docker Compose：`docker compose up -d` 一键（app + pg16/18-alpine + minio 可选），三命名卷持久化，HEALTHCHECK 自愈，镜像 <150MB（musl 静态编译，amd64/arm64）；
- 升级 `pull && up -d`，sqlx migrate 自迁移，失败回滚镜像；离线内网提供 `docker save` 镜像包；
- 运维：`plm-cli`（backup/restore/体检/迁移预检/插件打包）。

## 后果

- ✅ 微微客户"有 Docker 就能跑"；无 Docker 时单机包同等能力；
- ⚠️ 内置 TLS 后单进程承担加解密——2C4G/50 并发场景开销可忽略；高并发大文件场景建议前置反代卸载（文档说明即可）。
