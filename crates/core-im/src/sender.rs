//! CardSender trait + 三平台机器人 webhook 实现（真实 HTTP）+ noop 缺省。

use crate::render::{dingtalk_card, dingtalk_signed_url, feishu_card, wecom_card, CardInput};
use async_trait::async_trait;
use core_model::{DomainError, Result};
use std::time::{SystemTime, UNIX_EPOCH};

#[async_trait]
pub trait CardSender: Send + Sync {
    async fn send_card(&self, input: &CardInput) -> Result<()>;
}

/// 缺省/测试：不出网
pub struct NoopSender;

#[async_trait]
impl CardSender for NoopSender {
    async fn send_card(&self, _input: &CardInput) -> Result<()> {
        Ok(())
    }
}

async fn post_json(
    http: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    let resp = http
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(|e| DomainError::Db(format!("IM webhook 请求失败: {e}")))?;
    let status = resp.status();
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| DomainError::Db(format!("IM webhook 响应非 JSON: {e}")))?;
    if !status.is_success() {
        return Err(DomainError::Db(format!("IM webhook HTTP {status}: {v}")));
    }
    Ok(v)
}

/// 钉钉自定义机器人（access_token webhook + 加签）
pub struct DingTalkSender {
    http: reqwest::Client,
    webhook: String,
    secret: String,
}

#[async_trait]
impl CardSender for DingTalkSender {
    async fn send_card(&self, input: &CardInput) -> Result<()> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let url = dingtalk_signed_url(&self.webhook, &self.secret, ts);
        let v = post_json(&self.http, &url, &dingtalk_card(input)).await?;
        if v.get("errcode").and_then(|c| c.as_i64()) != Some(0) {
            return Err(DomainError::Db(format!("钉钉机器人拒绝: {v}")));
        }
        Ok(())
    }
}

/// 企微群机器人
pub struct WeComSender {
    http: reqwest::Client,
    webhook: String,
}

#[async_trait]
impl CardSender for WeComSender {
    async fn send_card(&self, input: &CardInput) -> Result<()> {
        let v = post_json(&self.http, &self.webhook, &wecom_card(input)).await?;
        if v.get("errcode").and_then(|c| c.as_i64()) != Some(0) {
            return Err(DomainError::Db(format!("企微机器人拒绝: {v}")));
        }
        Ok(())
    }
}

/// 飞书自定义机器人
pub struct FeishuSender {
    http: reqwest::Client,
    webhook: String,
}

#[async_trait]
impl CardSender for FeishuSender {
    async fn send_card(&self, input: &CardInput) -> Result<()> {
        let v = post_json(&self.http, &self.webhook, &feishu_card(input)).await?;
        let ok = v.get("code").and_then(|c| c.as_i64()) == Some(0)
            || v.get("StatusCode").and_then(|c| c.as_i64()) == Some(0);
        if !ok {
            return Err(DomainError::Db(format!("飞书机器人拒绝: {v}")));
        }
        Ok(())
    }
}

/// 按配置装配（provider=none → Noop）
pub fn sender_for(
    provider: &str,
    webhook: &str,
    secret: &str,
    http: reqwest::Client,
) -> std::sync::Arc<dyn CardSender> {
    match provider {
        "dingtalk" => std::sync::Arc::new(DingTalkSender {
            http,
            webhook: webhook.to_string(),
            secret: secret.to_string(),
        }),
        "wecom" => std::sync::Arc::new(WeComSender {
            http,
            webhook: webhook.to_string(),
        }),
        "feishu" => std::sync::Arc::new(FeishuSender {
            http,
            webhook: webhook.to_string(),
        }),
        _ => std::sync::Arc::new(NoopSender),
    }
}
