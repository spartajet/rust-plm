# ADR-0008 数据访问：sqlx + PostgreSQL（不用重 ORM）

- 状态：已接受（2026-11-11）
- 关联需求：ADR-0002（动态元数据查询）、BOM-06（性能）、F3

## 背景

备选：SeaORM（实体抽象好，但动态表/动态列场景下抽象反而碍事）、Diesel（编译期强类型，同样不适合运行期才知道的表结构）、sqlx（编译期校验静态 SQL + 运行期绑定动态 SQL 双模式，直连 Postgres 特性：JSONB/CTE/LISTEN-NOTIFY/pgvector）。

## 决策

sqlx + PostgreSQL 16：**静态域 SQL 走 sqlx::query! 编译期校验**；**元数据驱动的动态查询**由 core-model 的 QueryBuilder 生成（运行期参数绑定，绝无字符串拼接用户输入）；迁移统一 sqlx migrate；LISTEN/NOTIFY 用于进程内唤醒（投递器/缓存失效），pgvector 支撑 RAG（ADR-0010）。

## 后果

- ✅ 双模式并存：核心路径编译期安全 + 动态路径运行期校验与测试兜底；
- ✅ 递归 CTE/JSONB/分区/通知等 PG 特性全可用；
- ⚠️ 动态 SQL 层需自建小型测试矩阵（按类型×属性类型×操作符组合回归）。
