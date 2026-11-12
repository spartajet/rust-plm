//! 卡片渲染（纯函数，可穷举单测）+ 钉钉加签 + 统一 HMAC 校验。

use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;

/// 卡片输入（评审事件语义）
pub struct CardInput<'a> {
    pub obj_no: &'a str,
    pub major: i32,
    pub minor: i32,
    /// review=审核中 / checked=待复核
    pub state: &'a str,
    pub actor: Option<&'a str>,
    /// 详情页链接（模板占位 {obj_no} 已替换）；None 则不带链接
    pub url: Option<String>,
}

fn title_of(input: &CardInput) -> String {
    let head = if input.state == "checked" {
        "复核请求"
    } else {
        "评审请求"
    };
    format!("{head}：{} v{}.{}", input.obj_no, input.major, input.minor)
}

fn body_of(input: &CardInput) -> String {
    let state_label = if input.state == "checked" {
        "待复核（第二级）"
    } else {
        "审核中"
    };
    let mut lines = vec![
        format!("- 对象：{}", input.obj_no),
        format!("- 版本：v{}.{}", input.major, input.minor),
        format!("- 状态：{state_label}"),
        format!(
            "- 提交人：{}",
            input.actor.unwrap_or("-")
        ),
    ];
    if let Some(u) = &input.url {
        lines.push(format!("\n[前往评审]({u})"));
    }
    lines.join("\n")
}

/// 钉钉：msgtype=markdown（自定义机器人；签名见 `dingtalk_sign`）
pub fn dingtalk_card(input: &CardInput) -> Value {
    json!({
        "msgtype": "markdown",
        "markdown": { "title": title_of(input), "text": format!("### {}\n\n{}", title_of(input), body_of(input)) }
    })
}

/// 企微：msgtype=markdown（群机器人）
pub fn wecom_card(input: &CardInput) -> Value {
    json!({
        "msgtype": "markdown",
        "markdown": { "content": format!("**{}**\n{}", title_of(input), body_of(input)) }
    })
}

/// 飞书：msg_type=text（自定义机器人）
pub fn feishu_card(input: &CardInput) -> Value {
    json!({
        "msg_type": "text",
        "content": { "text": format!("{}\n{}", title_of(input), body_of(input).replace("- ", "")) }
    })
}

/// 钉钉自定义机器人加签：sign = base64(HMAC-SHA256(secret, "{ts}\n{secret}"))，URL 编码后拼 query。
pub fn dingtalk_sign(secret: &str, timestamp_ms: i64) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(format!("{timestamp_ms}\n{secret}").as_bytes());
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

pub fn dingtalk_signed_url(webhook: &str, secret: &str, timestamp_ms: i64) -> String {
    let sign = dingtalk_sign(secret, timestamp_ms);
    // base64 含 + / = ——须 URL 编码
    let enc: String = sign
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect();
    let sep = if webhook.contains('?') { "&" } else { "?" };
    format!("{webhook}{sep}timestamp={timestamp_ms}&sign={enc}")
}

/// 统一回调签名校验：`X-IM-Sign` = hex(HMAC-SHA256(im_secret, body))；逐位异或折叠比较。
pub fn verify_hmac_hex(secret: &str, body: &[u8], sign_hex: &str) -> bool {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(body);
    let expected = mac.finalize().into_bytes();
    let expected_hex: String = expected.iter().map(|b| format!("{b:02x}")).collect();
    let a = expected_hex.as_bytes();
    let b = sign_hex.as_bytes();
    let mut diff = (a.len() ^ b.len()) as u64;
    for i in 0..a.len().min(b.len()) {
        diff |= (a[i] ^ b[i]) as u64;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> CardInput<'static> {
        CardInput {
            obj_no: "DWG-X",
            major: 2,
            minor: 0,
            state: "review",
            actor: Some("u-1"),
            url: Some("http://plm/docs/DWG-X".into()),
        }
    }

    #[test]
    fn dingtalk_card_contract() {
        let v = dingtalk_card(&sample());
        assert_eq!(v["msgtype"], "markdown");
        let text = v["markdown"]["text"].as_str().unwrap();
        assert!(text.contains("DWG-X") && text.contains("v2.0") && text.contains("前往评审"));
        assert!(v["markdown"]["title"].as_str().unwrap().contains("评审请求"));
    }

    #[test]
    fn wecom_card_contract() {
        let v = wecom_card(&sample());
        assert_eq!(v["msgtype"], "markdown");
        assert!(v["markdown"]["content"].as_str().unwrap().contains("v2.0"));
    }

    #[test]
    fn feishu_card_contract() {
        let v = feishu_card(&sample());
        assert_eq!(v["msg_type"], "text");
        assert!(v["content"]["text"].as_str().unwrap().contains("DWG-X"));
    }

    #[test]
    fn checked_state_title_and_url_absent() {
        let mut i = sample();
        i.state = "checked";
        assert!(title_of(&i).starts_with("复核请求"));
        i.url = None;
        assert!(!wecom_card(&i)["markdown"]["content"]
            .as_str()
            .unwrap()
            .contains("前往评审"));
    }

    #[test]
    fn dingtalk_sign_is_deterministic_and_url_encoded() {
        let s = dingtalk_sign("sec9", 1_700_000_000_000);
        assert_eq!(dingtalk_sign("sec9", 1_700_000_000_000), s, "同输入同签名");
        assert_ne!(dingtalk_sign("sec9", 1_700_000_000_001), s, "时间戳变化签名变化");
        let url = dingtalk_signed_url("https://oapi.dingtalk.com/robot/send?access_token=x", "sec9", 1);
        assert!(url.contains("timestamp=1&sign="));
        assert!(!url.contains('+') || url.contains("%2B"), "base64 的 + 必须 URL 编码");
    }

    #[test]
    fn hmac_hex_verify_both_ways() {
        // 与 python: hmac.new(b"s", b"body", sha256).hexdigest() 对齐的已知值
        let sign = {
            let mut mac = Hmac::<Sha256>::new_from_slice(b"s").unwrap();
            mac.update(b"body");
            let d = mac.finalize().into_bytes();
            d.iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        assert!(verify_hmac_hex("s", b"body", &sign));
        assert!(!verify_hmac_hex("s", b"body!", &sign));
        assert!(!verify_hmac_hex("s2", b"body", &sign));
        assert!(!verify_hmac_hex("s", b"body", &sign[..sign.len() - 1]));
    }
}
