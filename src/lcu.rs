use crate::types::{ChampionSummary, EnemyInfo, LcuAuth, TeamMateInfo, looks_like_chinese, to_opgg_slug};
use base64::Engine;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;

/// 英雄图标数据（一次性从 LCU 加载）
#[derive(Debug)]
pub struct ChampionIconData {
    pub icons: HashMap<i64, (Vec<u8>, u32, u32)>, // champion_id → (rgba, w, h)
    pub slug_to_id: HashMap<String, i64>,
    pub name_to_id: HashMap<String, i64>,
    pub id_to_name: HashMap<i64, String>,
}

/// LCU 状态更新消息
#[derive(Debug)]
pub struct LcuState {
    pub connected: bool,
    pub error: String,
    pub enemies: Vec<EnemyInfo>,
    pub teammates: Vec<TeamMateInfo>,
    pub my_pos: String,
    pub lane_enemy_id: Option<i64>,
    pub champion_lang: String,
    pub champion_data: Option<ChampionIconData>,
    pub auth: Option<LcuAuth>,
}

/// 查找 lockfile
fn find_lockfile(config_dir: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();

    // 用户配置的目录
    if !config_dir.is_empty() {
        candidates.push(PathBuf::from(config_dir).join("lockfile"));
    }

    // 环境变量
    if let Ok(dir) = std::env::var("LOL_LOCKFILE_DIR") {
        if !dir.is_empty() {
            candidates.push(PathBuf::from(&dir).join("lockfile"));
        }
    }

    // 常见路径
    let paths = [
        r"C:\Riot Games\League of Legends\lockfile",
        r"C:\Program Files\Riot Games\League of Legends\lockfile",
        r"C:\Program Files (x86)\Riot Games\League of Legends\lockfile",
        r"D:\Riot Games\League of Legends\lockfile",
        r"D:\League of Legends\lockfile",
        r"D:\Riot Games\LeagueClient\lockfile",
    ];
    for p in &paths {
        candidates.push(PathBuf::from(p));
    }

    // LOCALAPPDATA / PROGRAMDATA
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidates.push(PathBuf::from(&local).join(r"Riot Games\Riot Client\Config\lockfile"));
    }
    if let Ok(pd) = std::env::var("PROGRAMDATA") {
        candidates.push(PathBuf::from(&pd).join(r"Riot Games\Riot Client\Config\lockfile"));
    }

    candidates.into_iter().find(|p| p.exists())
}

/// 读取 lockfile
fn read_lockfile(path: &Path) -> Result<LcuAuth, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("读取 lockfile 失败: {e}"))?;
    let parts: Vec<&str> = raw.trim().split(':').collect();
    if parts.len() != 5 {
        return Err(format!("lockfile 格式不符: {raw}"));
    }
    let port: u16 = parts[2].parse().map_err(|_| "端口解析失败".to_string())?;
    Ok(LcuAuth {
        port,
        password: parts[3].to_string(),
    })
}

/// 创建忽略证书验证的 HTTP 客户端（仅用于 127.0.0.1 LCU）
pub fn lcu_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap()
}

/// LCU GET 请求
async fn lcu_get(
    client: &reqwest::Client,
    auth: &LcuAuth,
    path: &str,
    params: Option<&[(&str, &str)]>,
) -> Result<serde_json::Value, String> {
    let token = base64::engine::general_purpose::STANDARD
        .encode(format!("riot:{}", auth.password));

    let mut url = format!("https://127.0.0.1:{}{}", auth.port, path);
    if let Some(p) = params {
        let qs: Vec<String> = p.iter().map(|(k, v)| format!("{k}={v}")).collect();
        url = format!("{url}?{}", qs.join("&"));
    }

    let resp = client
        .get(&url)
        .header("Authorization", format!("Basic {token}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    resp.json().await.map_err(|e| e.to_string())
}

/// LCU GET 请求（返回原始字节，用于图片等）
async fn lcu_get_bytes(
    client: &reqwest::Client,
    auth: &LcuAuth,
    path: &str,
) -> Result<Vec<u8>, String> {
    let token = base64::engine::general_purpose::STANDARD
        .encode(format!("riot:{}", auth.password));
    let url = format!("https://127.0.0.1:{}{}", auth.port, path);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Basic {token}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
}

/// 后台 LCU 轮询任务
pub fn spawn_lcu_poller(
    rt: Arc<tokio::runtime::Runtime>,
    lockfile_dir: String,
    tx: mpsc::UnboundedSender<LcuState>,
    ctx: egui::Context,
) {
    rt.spawn(async move {
        let client = lcu_client();
        let mut champ_cache: HashMap<i64, ChampionSummary> = HashMap::new();
        let mut champion_lang = "unknown".to_string();
        let mut icon_data: Option<ChampionIconData> = None;
        // 队友缓存: summoner_id → (name, tag_line, puuid, account_id, tier, division, lp)
        let mut teammate_rank_cache: HashMap<i64, (String, String, String, i64, String, String, i32)> = HashMap::new();
        let mut my_summoner_id: i64 = 0;

        loop {
            let lockfile = find_lockfile(&lockfile_dir);
            let Some(lockfile_path) = lockfile else {
                let _ = tx.send(LcuState {
                    connected: false,
                    error: "找不到 lockfile（可在 config.toml 设置 lockfile_dir）".into(),
                    enemies: vec![],
                    teammates: vec![],
                    my_pos: String::new(),
                    lane_enemy_id: None,
                    champion_lang: champion_lang.clone(),
                    champion_data: None,
                    auth: None,
                });
                ctx.request_repaint();
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            };

            let auth = match read_lockfile(&lockfile_path) {
                Ok(a) => a,
                Err(e) => {
                    let _ = tx.send(LcuState {
                        connected: false,
                        error: e,
                        enemies: vec![],
                        teammates: vec![],
                        my_pos: String::new(),
                        lane_enemy_id: None,
                        champion_lang: champion_lang.clone(),
                        champion_data: None,
                        auth: None,
                    });
                    ctx.request_repaint();
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    continue;
                }
            };

            // 加载英雄列表
            if champ_cache.is_empty() {
                // 尝试 zh_CN
                if let Ok(val) = lcu_get(&client, &auth, "/lol-game-data/assets/v1/champion-summary.json", Some(&[("locale", "zh_CN")])).await {
                    if let Some(arr) = val.as_array() {
                        for item in arr {
                            if let Ok(c) = serde_json::from_value::<ChampionSummary>(item.clone()) {
                                champ_cache.insert(c.id, c);
                            }
                        }
                        let sample_name = champ_cache.values().next().map(|c| c.name.as_str()).unwrap_or("");
                        champion_lang = if looks_like_chinese(sample_name) { "zh_CN".into() } else { "non_zh".into() };
                    }
                }
                // zh_CN 结果非中文时尝试 zh_TW（适配繁体中文客户端）
                if champion_lang == "non_zh" {
                    champ_cache.clear();
                    if let Ok(val) = lcu_get(&client, &auth, "/lol-game-data/assets/v1/champion-summary.json", Some(&[("locale", "zh_TW")])).await {
                        if let Some(arr) = val.as_array() {
                            for item in arr {
                                if let Ok(c) = serde_json::from_value::<ChampionSummary>(item.clone()) {
                                    champ_cache.insert(c.id, c);
                                }
                            }
                            let sample_name = champ_cache.values().next().map(|c| c.name.as_str()).unwrap_or("");
                            if looks_like_chinese(sample_name) {
                                champion_lang = "zh_TW".into();
                            }
                        }
                    }
                }
                // fallback
                if champ_cache.is_empty() {
                    if let Ok(val) = lcu_get(&client, &auth, "/lol-game-data/assets/v1/champion-summary.json", None).await {
                        if let Some(arr) = val.as_array() {
                            for item in arr {
                                if let Ok(c) = serde_json::from_value::<ChampionSummary>(item.clone()) {
                                    champ_cache.insert(c.id, c);
                                }
                            }
                            champion_lang = "client_default".into();
                        }
                    }
                }
            }

            // 加载英雄图标（一次性）
            if icon_data.is_none() && !champ_cache.is_empty() {
                let mut slug_to_id = HashMap::new();
                let mut name_to_id = HashMap::new();
                for (&id, champ) in &champ_cache {
                    if id <= 0 { continue; }
                    slug_to_id.insert(to_opgg_slug(&champ.alias, &champ.name), id);
                    name_to_id.insert(champ.name.clone(), id);
                }

                let mut icons = HashMap::new();
                let ids: Vec<i64> = champ_cache.keys().copied().filter(|&id| id > 0).collect();
                let mut set = tokio::task::JoinSet::new();
                for id in ids {
                    let c = client.clone();
                    let a = auth.clone();
                    set.spawn(async move {
                        let path = format!("/lol-game-data/assets/v1/champion-icons/{id}.png");
                        let bytes = lcu_get_bytes(&c, &a, &path).await.ok()?;
                        let img = image::load_from_memory(&bytes).ok()?;
                        let rgba = img.to_rgba8();
                        let (w, h) = rgba.dimensions();
                        Some((id, rgba.into_raw(), w, h))
                    });
                }
                while let Some(result) = set.join_next().await {
                    if let Ok(Some((id, rgba, w, h))) = result {
                        icons.insert(id, (rgba, w, h));
                    }
                }

                let mut id_to_name = HashMap::new();
                for (&id, champ) in &champ_cache {
                    if id > 0 {
                        id_to_name.insert(id, champ.name.clone());
                    }
                }

                icon_data = Some(ChampionIconData { icons, slug_to_id, name_to_id, id_to_name });
            }

            // 缓存我的 summonerId（用于判断队伍归属）
            if my_summoner_id == 0 {
                if let Ok(me) = lcu_get(&client, &auth, "/lol-summoner/v1/current-summoner", None).await {
                    my_summoner_id = me.get("summonerId").and_then(|v| v.as_i64()).unwrap_or(0);
                }
            }

            // 获取选人 session
            match lcu_get(&client, &auth, "/lol-champ-select/v1/session", None).await {
                Ok(sess) => {
                    let their_team = sess.get("theirTeam").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    let my_team = sess.get("myTeam").and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    let local_cell = sess.get("localPlayerCellId").and_then(|v| v.as_i64());

                    // 找我的位置
                    let mut my_pos = String::new();
                    if let Some(cell) = local_cell {
                        for p in &my_team {
                            if p.get("cellId").and_then(|v| v.as_i64()) == Some(cell) {
                                my_pos = p.get("assignedPosition").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                break;
                            }
                        }
                    }

                    // 构建敌方列表
                    let mut enemies = Vec::new();
                    for p in &their_team {
                        let cid = p.get("championId").and_then(|v| v.as_i64()).unwrap_or(0);
                        let champ = champ_cache.get(&cid);
                        let name = champ.map(|c| c.name.as_str()).unwrap_or("未知英雄").to_string();
                        let slug = champ.map(|c| to_opgg_slug(&c.alias, &c.name)).unwrap_or_default();
                        let pos = p.get("assignedPosition").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        enemies.push(EnemyInfo {
                            champion_id: cid,
                            name,
                            slug,
                            pos,
                        });
                    }

                    // 构建全部玩家列表（并发获取未缓存的召唤师信息）
                    let mut teammates = Vec::new();
                    let mut to_fetch: Vec<(i64, i64, String, bool)> = Vec::new();
                    for (team, is_ally) in [(&my_team, true), (&their_team, false)] {
                        for p in team {
                            let sid = p.get("summonerId").and_then(|v| v.as_i64()).unwrap_or(0);
                            let cid = p.get("championId").and_then(|v| v.as_i64()).unwrap_or(0);
                            let pos = p.get("assignedPosition").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            if sid <= 0 { continue; }
                            let champ_name = champ_cache.get(&cid).map(|c| c.name.clone()).unwrap_or_default();
                            if let Some((name, tag, puuid, account_id, tier, div, lp)) = teammate_rank_cache.get(&sid) {
                                teammates.push(TeamMateInfo {
                                    summoner_name: name.clone(), tag_line: tag.clone(),
                                    puuid: puuid.clone(), account_id: *account_id,
                                    champion_id: cid, champion_name: champ_name, position: pos,
                                    rank_tier: tier.clone(), rank_division: div.clone(), rank_lp: *lp,
                                    is_ally,
                                });
                            } else {
                                to_fetch.push((sid, cid, pos, is_ally));
                            }
                        }
                    }
                    if !to_fetch.is_empty() {
                        let mut set = tokio::task::JoinSet::new();
                        for (sid, cid, pos, is_ally) in to_fetch {
                            let c = client.clone();
                            let a = auth.clone();
                            let cn = champ_cache.get(&cid).map(|c| c.name.clone()).unwrap_or_default();
                            set.spawn(async move {
                                let (name, tag, puuid, account_id) = match lcu_get(&c, &a, &format!("/lol-summoner/v1/summoners/{sid}"), None).await {
                                    Ok(val) => {
                                        let n = val.get("gameName").or(val.get("displayName"))
                                            .and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        let t = val.get("tagLine").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        let p = val.get("puuid").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        let aid = val.get("accountId").and_then(|v| v.as_i64()).unwrap_or(0);
                                        (n, t, p, aid)
                                    }
                                    Err(_) => (format!("玩家{sid}"), String::new(), String::new(), 0),
                                };
                                let (tier, div, lp) = if !puuid.is_empty() {
                                    match lcu_get(&c, &a, &format!("/lol-ranked/v1/ranked-stats/{puuid}"), None).await {
                                        Ok(val) => {
                                            let solo = val.get("queueMap").and_then(|q| q.get("RANKED_SOLO_5x5"));
                                            let t = solo.and_then(|s| s.get("tier")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let d = solo.and_then(|s| s.get("division")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let l = solo.and_then(|s| s.get("leaguePoints")).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                            (t, d, l)
                                        }
                                        Err(_) => (String::new(), String::new(), 0),
                                    }
                                } else {
                                    (String::new(), String::new(), 0)
                                };
                                Some((sid, TeamMateInfo {
                                    summoner_name: name, tag_line: tag, puuid, account_id,
                                    champion_id: cid, champion_name: cn, position: pos,
                                    rank_tier: tier, rank_division: div, rank_lp: lp,
                                    is_ally,
                                }))
                            });
                        }
                        while let Some(result) = set.join_next().await {
                            if let Ok(Some((sid, info))) = result {
                                teammate_rank_cache.insert(sid, (
                                    info.summoner_name.clone(), info.tag_line.clone(),
                                    info.puuid.clone(), info.account_id,
                                    info.rank_tier.clone(), info.rank_division.clone(), info.rank_lp,
                                ));
                                teammates.push(info);
                            }
                        }
                    }

                    // 找对线对手
                    let lane_enemy_id = if !my_pos.is_empty() {
                        enemies.iter().find(|e| e.pos == my_pos).map(|e| e.champion_id)
                    } else {
                        None
                    };

                    let _ = tx.send(LcuState {
                        connected: true,
                        error: String::new(),
                        enemies,
                        teammates,
                        my_pos,
                        lane_enemy_id,
                        champion_lang: champion_lang.clone(),
                        champion_data: icon_data.take(),
                        auth: Some(auth.clone()),
                    });
                    ctx.request_repaint();
                    tokio::time::sleep(std::time::Duration::from_millis(900)).await;
                }
                Err(e) => {
                    // 不在选人界面，检查是否已进入游戏
                    let mut handled = false;
                    if my_summoner_id > 0 {
                        if let Ok(gf) = lcu_get(&client, &auth, "/lol-gameflow/v1/session", None).await {
                            let phase = gf.get("phase").and_then(|v| v.as_str()).unwrap_or("");
                            if matches!(phase, "InProgress" | "GameStart" | "Reconnect" | "WaitingForStats") {
                                let game_data = gf.get("gameData");
                                let team_one = game_data.and_then(|g| g.get("teamOne")).and_then(|v| v.as_array()).cloned().unwrap_or_default();
                                let team_two = game_data.and_then(|g| g.get("teamTwo")).and_then(|v| v.as_array()).cloned().unwrap_or_default();

                                // 判断我在哪个队伍
                                let my_in_one = team_one.iter().any(|p| p.get("summonerId").and_then(|v| v.as_i64()) == Some(my_summoner_id));
                                let (my_team, their_team) = if my_in_one { (&team_one, &team_two) } else { (&team_two, &team_one) };

                                // 构建敌方列表
                                let mut enemies = Vec::new();
                                for p in their_team {
                                    let cid = p.get("championId").and_then(|v| v.as_i64()).unwrap_or(0);
                                    let champ = champ_cache.get(&cid);
                                    let name = champ.map(|c| c.name.as_str()).unwrap_or("未知英雄").to_string();
                                    let slug = champ.map(|c| to_opgg_slug(&c.alias, &c.name)).unwrap_or_default();
                                    let pos = p.get("selectedPosition").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    enemies.push(EnemyInfo { champion_id: cid, name, slug, pos });
                                }

                                // 构建全部玩家列表
                                let mut my_pos = String::new();
                                let mut teammates = Vec::new();
                                let mut to_fetch: Vec<(i64, i64, String, String, bool)> = Vec::new();
                                for (team, is_ally) in [(my_team, true), (their_team, false)] {
                                    for p in team {
                                        let sid = p.get("summonerId").and_then(|v| v.as_i64()).unwrap_or(0);
                                        let cid = p.get("championId").and_then(|v| v.as_i64()).unwrap_or(0);
                                        let pos = p.get("selectedPosition").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        let sname = p.get("summonerName").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                        if sid <= 0 { continue; }
                                        if is_ally && sid == my_summoner_id { my_pos = pos.clone(); }
                                        let champ_name = champ_cache.get(&cid).map(|c| c.name.clone()).unwrap_or_default();
                                        if let Some((cached_name, tag, puuid, account_id, tier, div, lp)) = teammate_rank_cache.get(&sid) {
                                            let display_name = if !sname.is_empty() { sname } else { cached_name.clone() };
                                            teammates.push(TeamMateInfo {
                                                summoner_name: display_name, tag_line: tag.clone(),
                                                puuid: puuid.clone(), account_id: *account_id,
                                                champion_id: cid, champion_name: champ_name, position: pos,
                                                rank_tier: tier.clone(), rank_division: div.clone(), rank_lp: *lp,
                                                is_ally,
                                            });
                                        } else {
                                            to_fetch.push((sid, cid, pos, sname, is_ally));
                                        }
                                    }
                                }
                                if !to_fetch.is_empty() {
                                    let mut set = tokio::task::JoinSet::new();
                                    for (sid, cid, pos, sname, is_ally) in to_fetch {
                                        let c = client.clone();
                                        let a = auth.clone();
                                        let cn = champ_cache.get(&cid).map(|c| c.name.clone()).unwrap_or_default();
                                        set.spawn(async move {
                                            let (name, tag, puuid, account_id) = match lcu_get(&c, &a, &format!("/lol-summoner/v1/summoners/{sid}"), None).await {
                                                Ok(val) => {
                                                    let n = if sname.is_empty() {
                                                        val.get("gameName").or(val.get("displayName"))
                                                            .and_then(|v| v.as_str()).unwrap_or("").to_string()
                                                    } else { sname };
                                                    let t = val.get("tagLine").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                    let p = val.get("puuid").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                    let aid = val.get("accountId").and_then(|v| v.as_i64()).unwrap_or(0);
                                                    (n, t, p, aid)
                                                }
                                                Err(_) => {
                                                    let n = if sname.is_empty() { format!("玩家{sid}") } else { sname };
                                                    (n, String::new(), String::new(), 0)
                                                }
                                            };
                                            let (tier, div, lp) = if !puuid.is_empty() {
                                                match lcu_get(&c, &a, &format!("/lol-ranked/v1/ranked-stats/{puuid}"), None).await {
                                                    Ok(val) => {
                                                        let solo = val.get("queueMap").and_then(|q| q.get("RANKED_SOLO_5x5"));
                                                        let t = solo.and_then(|s| s.get("tier")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                        let d = solo.and_then(|s| s.get("division")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                                                        let l = solo.and_then(|s| s.get("leaguePoints")).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                                        (t, d, l)
                                                    }
                                                    Err(_) => (String::new(), String::new(), 0),
                                                }
                                            } else {
                                                (String::new(), String::new(), 0)
                                            };
                                            Some((sid, TeamMateInfo {
                                                summoner_name: name, tag_line: tag, puuid, account_id,
                                                champion_id: cid, champion_name: cn, position: pos,
                                                rank_tier: tier, rank_division: div, rank_lp: lp,
                                                is_ally,
                                            }))
                                        });
                                    }
                                    while let Some(result) = set.join_next().await {
                                        if let Ok(Some((sid, info))) = result {
                                            teammate_rank_cache.insert(sid, (
                                                info.summoner_name.clone(), info.tag_line.clone(),
                                                info.puuid.clone(), info.account_id,
                                                info.rank_tier.clone(), info.rank_division.clone(), info.rank_lp,
                                            ));
                                            teammates.push(info);
                                        }
                                    }
                                }

                                let lane_enemy_id = if !my_pos.is_empty() {
                                    enemies.iter().find(|e| e.pos == my_pos).map(|e| e.champion_id)
                                } else {
                                    None
                                };

                                let _ = tx.send(LcuState {
                                    connected: true,
                                    error: String::new(),
                                    enemies,
                                    teammates,
                                    my_pos,
                                    lane_enemy_id,
                                    champion_lang: champion_lang.clone(),
                                    champion_data: icon_data.take(),
                                    auth: Some(auth.clone()),
                                });
                                ctx.request_repaint();
                                tokio::time::sleep(std::time::Duration::from_millis(900)).await;
                                handled = true;
                            }
                        }
                    }
                    if !handled {
                        let _ = tx.send(LcuState {
                            connected: true,
                            error: format!("不在选人界面：{}", &e[..e.len().min(140)]),
                            enemies: vec![],
                            teammates: vec![],
                            my_pos: String::new(),
                            lane_enemy_id: None,
                            champion_lang: champion_lang.clone(),
                            champion_data: icon_data.take(),
                            auth: Some(auth.clone()),
                        });
                        ctx.request_repaint();
                        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
                    }
                }
            }
        }
    });
}

