-- 0014_cod_counter.sql — 增量16 COD 编码系统：取号计数器（COD-02 并发无重号）。
-- key=日期段（按日流水防溢出）；原子 upsert 取号无显式行锁也并发安全。

CREATE TABLE IF NOT EXISTS cod_counter (
    type_code text NOT NULL,
    key       text NOT NULL,
    value     bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (type_code, key)
);
