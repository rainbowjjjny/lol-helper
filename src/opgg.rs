use crate::types::{CounterDisplay, CounterEntry, MatchEntry, OpggCache};
use regex::Regex;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

const OPGG_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

/// 获取 exe 同目录下的 opgg_data.json 路径
fn data_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.parent()
        .unwrap_or(std::path::Path::new("."))
        .join("opgg_data.json")
}

/// 加载本地缓存
pub fn load_local_data() -> OpggCache {
    let path = data_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => OpggCache::default(),
    }
}

/// 保存本地缓存
pub fn save_local_data(cache: &OpggCache) {
    let path = data_path();
    if let Ok(json) = serde_json::to_string(cache) {
        let _ = std::fs::write(path, json);
    }
}

/// 从 RSC push 数据中解析满足条件的 data 数组
fn parse_rsc_push_data(html: &str, predicate: &dyn Fn(&Value) -> bool) -> Option<Vec<Value>> {
    let re = Regex::new(r"self\.__next_f\.push\(\[").unwrap();
    for m in re.find_iter(html) {
        let start = m.end();
        let rest = &html[start..];
        let close = rest.find("])</script>");
        let Some(close_pos) = close else { continue };
        let push_raw = &rest[..close_pos + 1];
        if push_raw.len() < 500 {
            continue;
        }

        let wrapped = format!("[{push_raw}");
        let parsed: Value = match serde_json::from_str(&wrapped) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let Some(arr) = parsed.as_array() else { continue };
        for elem in arr {
            let Some(s) = elem.as_str() else { continue };
            let colon = s.find(':');
            let Some(colon_pos) = colon else { continue };
            let inner: Value = match serde_json::from_str(&s[colon_pos + 1..]) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(found) = find_data_array(&inner, predicate, 0) {
                return Some(found);
            }
        }
    }
    None
}

/// 递归搜索 data 数组
fn find_data_array(
    obj: &Value,
    predicate: &dyn Fn(&Value) -> bool,
    depth: usize,
) -> Option<Vec<Value>> {
    if depth > 5 {
        return None;
    }
    match obj {
        Value::Object(map) => {
            for (k, v) in map {
                if k == "data" {
                    if let Some(arr) = v.as_array() {
                        if !arr.is_empty() && arr[0].is_object() && predicate(&arr[0]) {
                            return Some(arr.clone());
                        }
                    }
                }
                if let Some(r) = find_data_array(v, predicate, depth + 1) {
                    return Some(r);
                }
            }
            None
        }
        Value::Array(arr) => {
            for item in arr {
                if let Some(r) = find_data_array(item, predicate, depth + 1) {
                    return Some(r);
                }
            }
            None
        }
        _ => None,
    }
}

/// 英雄+位置条目
#[derive(Debug, Clone)]
pub struct ChampPosEntry {
    pub key: String,
    pub name: String,
    pub position: String,
}

/// 从 OP.GG 获取英雄+位置列表
pub async fn fetch_champion_position_list(
    client: &reqwest::Client,
) -> Result<(Vec<ChampPosEntry>, std::collections::HashMap<String, String>), String> {
    let url = "https://www.op.gg/zh-cn/lol/champions?position=all&region=global";
    let resp = client
        .get(url)
        .header("User-Agent", OPGG_UA)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let html = resp.text().await.map_err(|e| e.to_string())?;

    let arr = parse_rsc_push_data(&html, &|v| {
        v.get("key").is_some() && v.get("name").is_some() && v.get("positionName").is_some()
    });

    let Some(arr) = arr else {
        return Ok((vec![], std::collections::HashMap::new()));
    };

    let mut entries = Vec::new();
    let mut name_map = std::collections::HashMap::new();

    for item in &arr {
        let key = item.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let pos = item
            .get("positionName")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !key.is_empty() && !name.is_empty() && !pos.is_empty() {
            entries.push(ChampPosEntry {
                key: key.to_string(),
                name: name.to_string(),
                position: pos.to_string(),
            });
            name_map.entry(key.to_string()).or_insert(name.to_string());
        }
    }

    Ok((entries, name_map))
}

/// 从 OP.GG 获取指定英雄的克制数据
async fn fetch_counters_from_opgg(
    client: &reqwest::Client,
    slug: &str,
    position: &str,
) -> Result<Vec<CounterEntry>, String> {
    let pos_slug = crate::types::opgg_pos_slug(position);
    let url = if !pos_slug.is_empty() {
        format!(
            "https://www.op.gg/champions/{slug}/counters/{pos_slug}?region=global&tier=emerald_plus"
        )
    } else {
        format!("https://www.op.gg/champions/{slug}/counters?region=global&tier=emerald_plus")
    };

    let mut html = String::new();
    for attempt in 0..2 {
        match client
            .get(&url)
            .header("User-Agent", OPGG_UA)
            .timeout(std::time::Duration::from_secs(8))
            .send()
            .await
        {
            Ok(resp) => {
                match resp.text().await {
                    Ok(t) => { html = t; break; }
                    Err(e) => {
                        if attempt == 1 { return Err(e.to_string()); }
                    }
                }
            }
            Err(e) => {
                if attempt == 1 { return Err(e.to_string()); }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    let arr = parse_rsc_push_data(&html, &|v| {
        v.get("win_rate").is_some() && v.get("champion").is_some()
    });

    let Some(arr) = arr else {
        return Ok(vec![]);
    };

    let mut result = Vec::new();
    for r in &arr {
        let key = r
            .get("champion")
            .and_then(|c| c.get("key"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let win_rate = r
            .get("win_rate")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let games = r.get("play").and_then(|v| v.as_i64()).unwrap_or(0);
        result.push(CounterEntry {
            key,
            win_rate,
            games,
        });
    }

    Ok(result)
}

fn counter_key(slug: &str, position: &str) -> String {
    if position.is_empty() {
        slug.to_string()
    } else {
        format!("{slug}:{position}")
    }
}

/// 全量采集所有英雄克制数据
pub async fn fetch_all_counters(
    progress: Option<Arc<dyn Fn(usize, usize, &str) + Send + Sync>>,
) -> Result<OpggCache, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let (entries, name_map) = fetch_champion_position_list(&client).await?;
    if entries.is_empty() {
        return Err("无法获取英雄列表".into());
    }

    let total = entries.len();
    let counters = Arc::new(Mutex::new(std::collections::HashMap::new()));
    let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let sem = Arc::new(Semaphore::new(10));

    let mut handles = Vec::new();
    for entry in entries {
        let client = client.clone();
        let sem = sem.clone();
        let counters = counters.clone();
        let done = done.clone();
        let progress = progress.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            let ckey = counter_key(&entry.key, &entry.position);
            match fetch_counters_from_opgg(&client, &entry.key, &entry.position).await {
                Ok(data) if !data.is_empty() => {
                    counters.lock().unwrap().insert(ckey, data);
                }
                _ => {}
            }
            let d = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if let Some(ref p) = progress {
                p(d, total, &format!("{}({})", entry.name, entry.position));
            }
        });
        handles.push(handle);
    }

    for h in handles {
        let _ = h.await;
    }

    let counters = Arc::try_unwrap(counters).unwrap().into_inner().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();

    let cache = OpggCache {
        champions: name_map,
        counters,
        updated_at: now,
        total_entries: total,
    };

    save_local_data(&cache);

    if let Some(ref p) = progress {
        p(total, total, "完成");
    }

    Ok(cache)
}

/// 从缓存中获取指定英雄的克制数据
pub fn get_counters_for_champion(
    cache: &OpggCache,
    slug: &str,
    lcu_position: &str,
) -> Vec<CounterDisplay> {
    if slug.is_empty() {
        return vec![];
    }
    let opgg_pos = crate::types::lcu_pos_to_opgg(lcu_position);

    // 优先匹配 slug:position
    let key = counter_key(slug, opgg_pos);
    let mut counters = cache.counters.get(&key).cloned();

    // fallback: 尝试该英雄任意位置
    if counters.is_none() {
        for (k, v) in &cache.counters {
            if k.starts_with(&format!("{slug}:")) || k == slug {
                counters = Some(v.clone());
                break;
            }
        }
    }

    let Some(counters) = counters else {
        return vec![];
    };

    counters
        .iter()
        .map(|c| {
            let cn_name = cache
                .champions
                .get(&c.key)
                .cloned()
                .unwrap_or_else(|| c.key.clone());
            CounterDisplay {
                name: cn_name,
                key: c.key.clone(),
                win_rate: c.win_rate,
                games: c.games,
            }
        })
        .collect()
}

/// URL 路径段百分号编码
fn percent_encode_path(s: &str) -> String {
    let mut result = String::new();
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_' || *b == b'.' || *b == b'~' {
            result.push(*b as char);
        } else {
            result.push_str(&format!("%{:02X}", b));
        }
    }
    result
}

/// 构造 OP.GG 页面 URL（供调试显示）
pub fn match_history_url(region: &str, game_name: &str, tag_line: &str) -> String {
    let encoded = percent_encode_path(&format!("{game_name}-{tag_line}"));
    format!("https://www.op.gg/zh-cn/lol/summoners/{region}/{encoded}")
}

const OPGG_API: &str = "https://lol-api-summoner.op.gg";

/// 从 OP.GG API 查询 summoner_id
async fn opgg_lookup_summoner(
    client: &reqwest::Client,
    region: &str,
    game_name: &str,
    tag_line: &str,
) -> Result<String, String> {
    let riot_id = percent_encode_path(&format!("{game_name}#{tag_line}"));
    let url = format!("{OPGG_API}/api/v3/{region}/summoners?riot_id={riot_id}&hl=zh_CN");
    let resp = client
        .get(&url)
        .header("User-Agent", OPGG_UA)
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map_err(|e| format!("查询召唤师失败: {e}"))?;
    let json: Value = resp.json().await.map_err(|e| format!("解析响应失败: {e}"))?;
    let sid = json.get("data")
        .and_then(|d| d.as_array())
        .and_then(|arr| arr.first())
        .and_then(|s| s.get("summoner_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "未找到召唤师".to_string())?;
    Ok(sid.to_string())
}

/// 从 OP.GG API 获取玩家最近对局记录
pub async fn fetch_match_history(
    client: &reqwest::Client,
    region: &str,
    game_name: &str,
    tag_line: &str,
) -> Result<Vec<MatchEntry>, String> {
    // 1. 查询 summoner_id
    let summoner_id = opgg_lookup_summoner(client, region, game_name, tag_line).await?;

    // 2. 获取对局列表
    let url = format!(
        "{OPGG_API}/api/{region}/summoners/{summoner_id}/games?limit=20&game_type=total&hl=zh_CN&ended_at="
    );
    let resp = client
        .get(&url)
        .header("User-Agent", OPGG_UA)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("获取对局失败: {e}"))?;
    let json: Value = resp.json().await.map_err(|e| format!("解析对局失败: {e}"))?;

    let games = json.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();

    // 查找目标玩家的名字（用于匹配 participants）
    let target_name = game_name.to_lowercase();

    let mut entries = Vec::new();
    for game in &games {
        let duration = game.get("game_length_second").and_then(|v| v.as_i64()).unwrap_or(0);
        let game_type = game.get("game_type").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let queue_id = game.get("queue_id").and_then(|v| v.as_i64()).unwrap_or(0);

        // 解析时间戳（ISO 8601 → ms）
        let created_at = game.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
        let timestamp_ms = chrono::DateTime::parse_from_rfc3339(created_at)
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0);

        // 从 participants 中找目标玩家
        let participants = game.get("participants").and_then(|p| p.as_array());
        let me = participants.and_then(|arr| {
            arr.iter().find(|p| {
                p.get("summoner")
                    .and_then(|s| s.get("game_name"))
                    .and_then(|v| v.as_str())
                    .map(|n| n.to_lowercase() == target_name)
                    .unwrap_or(false)
            })
        });

        let Some(me) = me else { continue };
        let stats = me.get("stats");
        let champion_id = me.get("champion_id").and_then(|v| v.as_i64()).unwrap_or(0);

        let win = stats.and_then(|s| s.get("result")).and_then(|v| v.as_str()).unwrap_or("") == "WIN";
        let kills = stats.and_then(|s| s.get("kill")).and_then(|v| v.as_i64()).unwrap_or(0);
        let deaths = stats.and_then(|s| s.get("death")).and_then(|v| v.as_i64()).unwrap_or(0);
        let assists = stats.and_then(|s| s.get("assist")).and_then(|v| v.as_i64()).unwrap_or(0);

        entries.push(MatchEntry {
            champion_id,
            champion_key: String::new(),
            champion_name: String::new(),
            win,
            kills,
            deaths,
            assists,
            game_duration_secs: duration,
            timestamp_ms,
            queue_id,
            game_type,
        });
    }

    Ok(entries)
}
