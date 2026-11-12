//! casbin 求值器：模型 + 从自有 PG 表重建（策略源是表，casbin 只是引擎——ADR-0011）。
//! 起步为空（None 语义在 SecService.enf 处 fail-closed），reload_policies 装载真实策略。

use core_model::{DomainError, Result};
use sqlx::PgPool;

const MODEL: &str = r#"
[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[role_definition]
g = _, _

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = g(r.sub, p.sub) && r.obj == p.obj && r.act == p.act
"#;

/// 空求值器（无任何策略）
pub async fn empty_enforcer() -> Result<casbin::Enforcer> {
    use casbin::CoreApi;
    let model = casbin::DefaultModel::from_str(MODEL)
        .await
        .map_err(|e| DomainError::Internal(format!("casbin model: {e}")))?;
    casbin::Enforcer::new_raw(model, casbin::MemoryAdapter::default())
        .await
        .map_err(|e| DomainError::Internal(format!("casbin enforcer: {e}")))
}

/// 从 PG 策略表构建 Enforcer：p 行（角色权限）+ g 行（用户-角色）
pub async fn build_enforcer(pool: &PgPool) -> Result<casbin::Enforcer> {
    use casbin::MgmtApi;
    let mut e = empty_enforcer().await?;

    let perms: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT role_code, obj, act FROM sec_role_perm ORDER BY role_code, obj, act",
    )
    .fetch_all(pool)
    .await?;
    for (role, obj, act) in perms {
        e.add_policy(vec![role, obj, act])
            .await
            .map_err(|err| DomainError::Internal(format!("casbin add_policy: {err}")))?;
    }

    let groups: Vec<(String, String)> = sqlx::query_as(
        "SELECT username, role_code FROM sec_user_role ORDER BY username, role_code",
    )
    .fetch_all(pool)
    .await?;
    for (user, role) in groups {
        e.add_grouping_policy(vec![user, role])
            .await
            .map_err(|err| DomainError::Internal(format!("casbin add_grouping: {err}")))?;
    }
    Ok(e)
}
