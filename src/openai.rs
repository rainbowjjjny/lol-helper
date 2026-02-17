use crate::config::AppConfig;
use crate::types::pos_cn;

/// 调用 ChatGPT 分析对线策略
pub async fn call_chatgpt_analysis(
    config: &AppConfig,
    my_champ: &str,
    enemy_champ: &str,
    position: &str,
    win_rate: f64,
) -> String {
    if config.openai_api_key.is_empty() || config.openai_api_key == "sk-proj-xxx" {
        return "错误：未在 config.toml 中设置 openai_api_key。".into();
    }

    let pos_text = pos_cn(position);
    let pos_text = if pos_text.is_empty() {
        "未知位置"
    } else {
        pos_text
    };

    let mut prompt = format!(
        "我在英雄联盟中使用【{my_champ}】在{pos_text}对线【{enemy_champ}】。"
    );
    if win_rate > 0.0 {
        prompt += &format!(
            "\n根据数据，{my_champ} 对 {enemy_champ} 的胜率为 {win_rate:.1}%。"
        );
    }
    prompt += &format!(
        "\n\n请用中文简洁分析：\n\
         1. 对线思路（如何打、关键时间点、注意事项）\n\
         2. 推荐出装（核心装备 + 鞋子 + 可选装备）\n\
         3. 推荐符文\n\
         4. 召唤师技能建议\n\
         请保持简洁实用，不超过300字。"
    );

    let payload = serde_json::json!({
        "model": config.openai_model,
        "messages": [
            {
                "role": "system",
                "content": "你是一个英雄联盟高分段分析师，擅长对线策略和出装推荐。请用简洁的中文回答。"
            },
            {
                "role": "user",
                "content": prompt
            }
        ],
        "max_completion_tokens": 800,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header(
            "Authorization",
            format!("Bearer {}", config.openai_api_key),
        )
        .json(&payload)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await;

    match resp {
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            if !status.is_success() {
                return format!("ChatGPT 请求失败 ({status})：{body}");
            }
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(val) => val
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("无法解析响应")
                    .trim()
                    .to_string(),
                Err(e) => format!("响应解析失败：{e}"),
            }
        }
        Err(e) => format!("ChatGPT 请求失败：{e}"),
    }
}
