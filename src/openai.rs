use crate::config::AiEngine;
use crate::types::pos_cn;
use tokio::sync::mpsc;

/// 流式 AI 分析消息
pub enum AiStreamMsg {
    /// 追加文本片段
    Chunk(String),
    /// 流结束，附带完整文本用于缓存
    Done(String),
    /// 错误
    Error(String),
}

/// 构建提示词（返回 system_prompt, user_prompt）
fn build_prompts(
    my_champ: &str,
    enemy_champ: &str,
    position: &str,
    win_rate: f64,
) -> (String, String) {
    let system = "你是一个英雄联盟高分段对线分析师。回答要求：针对具体对局给出实战建议，出装要具体到装备名称，优劣势要结合出装和玩法一起说。用简洁中文回答。".to_string();

    let pos_text = pos_cn(position);
    let pos_text = if pos_text.is_empty() { "未知位置" } else { pos_text };

    let mut prompt = format!(
        "我在英雄联盟中使用【{my_champ}】在{pos_text}对线【{enemy_champ}】。"
    );
    if win_rate > 0.0 {
        prompt += &format!(
            "\n根据数据，{my_champ} 对 {enemy_champ} 的胜率为 {win_rate:.1}%。"
        );
    }
    prompt += "\n\n请用中文简洁分析：\n\
         1. 出门装推荐（起始装备+消耗品）\n\
         2. 优势期打法（什么时候强、怎么打、推荐出装路线）\n\
         3. 劣势期打法（什么时候弱、怎么苟、推荐出装路线）\n\
         4. 核心装备（按顺序列出）\n\
         5. 符文+召唤师技能\n\
         请保持简洁实用。";

    (system, prompt)
}

/// 流式调用 AI 分析（OpenAI 兼容接口）
pub async fn call_ai_stream(
    engine: &AiEngine,
    model: &str,
    my_champ: &str,
    enemy_champ: &str,
    position: &str,
    win_rate: f64,
    chunk_tx: mpsc::UnboundedSender<AiStreamMsg>,
    ctx: egui::Context,
) {
    let (system_prompt, user_prompt) = build_prompts(my_champ, enemy_champ, position, win_rate);
    call_ai_raw(engine, model, &system_prompt, &user_prompt, chunk_tx, ctx).await;
}

/// 通用流式调用（自定义 system/user prompt）
pub async fn call_ai_raw(
    engine: &AiEngine,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    chunk_tx: mpsc::UnboundedSender<AiStreamMsg>,
    ctx: egui::Context,
) {
    // 先发送调试头
    let debug_header = format!(
        "【引擎】{}\n【模型】{model}\n【Prompt】{user_prompt}\n\n────────────────\n\n",
        engine.name
    );
    let _ = chunk_tx.send(AiStreamMsg::Chunk(debug_header.clone()));
    ctx.request_repaint();

    let payload = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "max_completion_tokens": 4096,
        "stream": true,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&engine.api_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", engine.api_key))
        .json(&payload)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await;

    let mut resp = match resp {
        Ok(r) => {
            if !r.status().is_success() {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                let _ = chunk_tx.send(AiStreamMsg::Error(format!("请求失败 ({status})：{body}")));
                ctx.request_repaint();
                return;
            }
            r
        }
        Err(e) => {
            let _ = chunk_tx.send(AiStreamMsg::Error(format!("请求失败：{e}")));
            ctx.request_repaint();
            return;
        }
    };

    // 流式读取 SSE
    let mut full_text = debug_header;
    let mut buffer = String::new();

    loop {
        match resp.chunk().await {
            Ok(Some(bytes)) => {
                buffer.push_str(&String::from_utf8_lossy(&bytes));
                // 按行解析 SSE
                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim_end().to_string();
                    buffer = buffer[pos + 1..].to_string();

                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }
                    if let Some(data) = line.strip_prefix("data: ") {
                        if data.trim() == "[DONE]" {
                            let _ = chunk_tx.send(AiStreamMsg::Done(full_text));
                            ctx.request_repaint();
                            return;
                        }
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                            let content = val
                                .get("choices")
                                .and_then(|c| c.get(0))
                                .and_then(|c| c.get("delta"))
                                .and_then(|d| d.get("content"))
                                .and_then(|c| c.as_str())
                                .unwrap_or("");
                            if !content.is_empty() {
                                full_text.push_str(content);
                                let _ = chunk_tx.send(AiStreamMsg::Chunk(content.to_string()));
                                ctx.request_repaint();
                            }
                        }
                    }
                }
            }
            Ok(None) => {
                // 流结束
                let _ = chunk_tx.send(AiStreamMsg::Done(full_text));
                ctx.request_repaint();
                return;
            }
            Err(e) => {
                let _ = chunk_tx.send(AiStreamMsg::Error(format!("流读取失败：{e}")));
                ctx.request_repaint();
                return;
            }
        }
    }
}
