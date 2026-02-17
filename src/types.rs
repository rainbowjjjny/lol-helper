use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 敌方英雄信息
#[derive(Debug, Clone)]
pub struct EnemyInfo {
    pub champion_id: i64,
    pub name: String,
    pub slug: String,
    pub pos: String,
}

/// OP.GG 克制条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterEntry {
    pub key: String,
    pub win_rate: f64,
    pub games: i64,
}

/// OP.GG 本地缓存结构
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpggCache {
    #[serde(default)]
    pub champions: HashMap<String, String>, // key -> 中文名
    #[serde(default)]
    pub counters: HashMap<String, Vec<CounterEntry>>, // "slug:POS" -> counters
    #[serde(default)]
    pub updated_at: f64,
    #[serde(default)]
    pub total_entries: usize,
}

/// 克制数据（带中文名，用于 UI 展示）
#[derive(Debug, Clone)]
pub struct CounterDisplay {
    pub name: String,
    pub key: String,
    pub win_rate: f64,
    pub games: i64,
}

/// LCU 认证信息
#[derive(Debug, Clone)]
pub struct LcuAuth {
    pub port: u16,
    pub password: String,
}

/// LCU 英雄数据
#[derive(Debug, Clone, Deserialize)]
pub struct ChampionSummary {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub alias: String,
}

/// LCU assignedPosition → OP.GG positionName 映射
pub fn lcu_pos_to_opgg(lcu_pos: &str) -> &'static str {
    match lcu_pos {
        "TOP" => "TOP",
        "JUNGLE" => "JUNGLE",
        "MIDDLE" => "MID",
        "BOTTOM" => "ADC",
        "UTILITY" => "SUPPORT",
        _ => "",
    }
}

/// 位置中文名
pub fn pos_cn(p: &str) -> &'static str {
    match p {
        "TOP" => "上路",
        "JUNGLE" => "打野",
        "MIDDLE" | "MID" => "中路",
        "BOTTOM" | "ADC" => "下路",
        "UTILITY" | "SUPPORT" => "辅助",
        _ => "",
    }
}

/// OP.GG 位置 slug
pub fn opgg_pos_slug(pos: &str) -> &'static str {
    match pos {
        "TOP" => "top",
        "JUNGLE" => "jungle",
        "MID" => "mid",
        "ADC" => "adc",
        "SUPPORT" => "support",
        _ => "",
    }
}

/// 生成 slug（与 Python to_opgg_slug 一致）
pub fn to_opgg_slug(alias: &str, name: &str) -> String {
    let s = if !alias.is_empty() { alias } else { name };
    s.trim().to_lowercase().replace(' ', "-")
}

/// 检查字符串是否包含中文字符
pub fn looks_like_chinese(s: &str) -> bool {
    s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// 玩家信息（队友 + 对手）
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TeamMateInfo {
    pub summoner_name: String,
    pub tag_line: String,
    pub puuid: String,
    pub account_id: i64,
    pub champion_id: i64,
    pub champion_name: String,
    pub position: String,
    pub rank_tier: String,
    pub rank_division: String,
    pub rank_lp: i32,
    pub is_ally: bool,
}

/// 历史对局记录（来源 OP.GG）
#[derive(Debug, Clone)]
pub struct MatchEntry {
    pub champion_id: i64,
    pub champion_key: String,
    pub champion_name: String,
    pub win: bool,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub game_duration_secs: i64,
    pub timestamp_ms: i64,
    pub queue_id: i64,
    pub game_type: String,
}

/// 段位中文名
pub fn rank_cn(tier: &str) -> &'static str {
    match tier {
        "IRON" => "黑铁",
        "BRONZE" => "青铜",
        "SILVER" => "白银",
        "GOLD" => "黄金",
        "PLATINUM" => "铂金",
        "EMERALD" => "翡翠",
        "DIAMOND" => "钻石",
        "MASTER" => "大师",
        "GRANDMASTER" => "宗师",
        "CHALLENGER" => "王者",
        _ => "",
    }
}

/// OP.GG queueType → 中文
pub fn opgg_queue_cn(qt: &str) -> &str {
    match qt {
        "SOLORANKED" => "单双排",
        "FLEXRANKED" => "灵活排",
        "NORMAL" => "匹配",
        "ARAM" => "大乱斗",
        "URF" => "无限火力",
        "ARENA" => "斗魂竞技场",
        "BOT" => "人机",
        _ => qt,
    }
}

/// 队列类型名
pub fn queue_name(queue_id: i64) -> &'static str {
    match queue_id {
        420 => "单双排",
        440 => "灵活排",
        400 => "匹配",
        430 => "匹配",
        450 => "大乱斗",
        900 | 1010 => "无限火力",
        1700 => "斗魂竞技场",
        _ => "其他",
    }
}
