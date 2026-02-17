use crate::config::{AiEngine, AppConfig};
use crate::lcu::{self, LcuState};
use crate::opgg;
use crate::openai;
use crate::types::*;
use crate::win32;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc;
use egui::ColorImage;

fn favorites_path() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.parent().unwrap_or(std::path::Path::new(".")).join("favorites.json")
}

fn load_favorites() -> HashSet<String> {
    let path = favorites_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => HashSet::new(),
    }
}

fn save_favorites(fav: &HashSet<String>) {
    let path = favorites_path();
    if let Ok(json) = serde_json::to_string(fav) {
        let _ = std::fs::write(path, json);
    }
}

/// 后台任务消息
enum BgMsg {
    /// LCU 状态更新
    Lcu(LcuState),
    /// 全量更新进度
    UpdateProgress(usize, usize, String),
    /// 全量更新完成
    UpdateDone(Result<OpggCache, String>),
    /// AI 流式片段
    AiChunk(String),
    /// AI 流式结束（完整文本用于缓存）
    AiDone { cache_key: String, full_text: String },
    /// AI 错误
    AiError(String),
    /// 对局历史（OP.GG）
    MatchHistory {
        cache_key: String,
        name: String,
        url: String,
        entries: Result<Vec<MatchEntry>, String>,
    },
}

pub struct App {
    config: AppConfig,
    rt: Arc<tokio::runtime::Runtime>,
    tx: mpsc::UnboundedSender<BgMsg>,
    rx: mpsc::UnboundedReceiver<BgMsg>,

    // LCU 状态
    connected: bool,
    error: String,
    enemies: Vec<EnemyInfo>,
    my_pos: String,
    lane_enemy_id: Option<i64>,
    champion_lang: String,
    last_update_time: String,

    // 选项
    topmost: bool,
    autodock: bool,
    show_debug: bool,

    // 敌方选中
    selected_enemy_idx: Option<usize>,

    // 调试
    debug_slug: String,
    debug_hero_options: Vec<String>,
    debug_pos: String,
    debug_pos_options: Vec<String>,

    // OP.GG 缓存
    opgg_cache: OpggCache,

    // 克制数据
    counter_data: Vec<CounterDisplay>,
    counter_champ_name: String,
    counter_champ_slug: String,
    counter_sort_desc: bool,
    counter_sort_col: String,
    counter_error: String,
    counter_favorites: HashSet<String>,

    // 全量更新
    updating: bool,
    update_progress_text: String,

    // AI 分析
    ai_title: String,
    ai_text: String,
    ai_cache: HashMap<String, String>,
    ai_loading: bool,
    ai_engines: Vec<AiEngine>,
    ai_engine_idx: usize,
    ai_model_idx: usize,
    ai_cache_key: String,
    ai_chat_input: String,
    ai_chat_visible: bool,

    // LCU poller 是否已启动
    lcu_started: bool,
    // 调试：找到的窗口信息
    debug_lol_win: String,

    // 英雄图标
    icon_textures: HashMap<i64, egui::TextureHandle>,
    slug_to_id: HashMap<String, i64>,
    name_to_id: HashMap<String, i64>,
    champ_names: HashMap<i64, String>,

    // 玩家信息（全部10人）
    teammates: Vec<TeamMateInfo>,
    selected_teammate_idx: Option<usize>,

    // 手动选位
    selected_enemy_pos: String,

    // 对局历史
    lcu_auth: Option<LcuAuth>,
    match_history: Vec<MatchEntry>,
    match_history_cache: HashMap<String, Vec<MatchEntry>>,
    match_history_loading: bool,
    match_history_name: String,
    history_panel_open: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, config: AppConfig, rt: Arc<tokio::runtime::Runtime>) -> Self {
        // 加载中文字体
        let mut fonts = egui::FontDefinitions::default();
        let font_path = std::path::Path::new(r"C:\Windows\Fonts\msyh.ttc");
        if let Ok(font_data) = std::fs::read(font_path) {
            fonts.font_data.insert(
                "msyh".to_owned(),
                Arc::new(egui::FontData::from_owned(font_data)),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "msyh".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "msyh".to_owned());
        }
        cc.egui_ctx.set_fonts(fonts);

        let (tx, rx) = mpsc::unbounded_channel();

        // 加载本地缓存
        let opgg_cache = opgg::load_local_data();
        let counter_favorites = load_favorites();
        let ai_engines = config.get_engines();

        Self {
            config,
            rt,
            tx,
            rx,
            connected: false,
            error: String::new(),
            enemies: vec![],
            my_pos: String::new(),
            lane_enemy_id: None,
            champion_lang: "unknown".to_string(),
            last_update_time: "N/A".to_string(),
            topmost: true,
            autodock: true,
            show_debug: false,
            selected_enemy_idx: None,
            debug_slug: "ahri".to_string(),
            debug_hero_options: vec![
                "ahri", "yasuo", "zed", "lux", "jinx", "thresh", "leona",
                "darius", "garen", "leesin", "vayne", "ezreal", "kaisa",
                "syndra", "orianna", "irelia", "camille", "jax", "renekton",
                "yone", "katarina", "akali", "leblanc", "viego", "graves",
            ].into_iter().map(String::from).collect(),
            debug_pos: "MIDDLE".to_string(),
            debug_pos_options: vec![
                "TOP".into(),
                "JUNGLE".into(),
                "MIDDLE".into(),
                "BOTTOM".into(),
                "UTILITY".into(),
            ],
            opgg_cache,
            counter_data: vec![],
            counter_champ_name: String::new(),
            counter_champ_slug: String::new(),
            counter_sort_desc: true,
            counter_sort_col: "win_rate".to_string(),
            counter_error: String::new(),
            counter_favorites,
            updating: false,
            update_progress_text: String::new(),
            ai_title: "AI 对线分析".into(),
            ai_text: String::new(),
            ai_cache: HashMap::new(),
            ai_loading: false,
            ai_engines,
            ai_engine_idx: 0,
            ai_model_idx: 0,
            ai_cache_key: String::new(),
            ai_chat_input: String::new(),
            ai_chat_visible: false,
            lcu_started: false,
            debug_lol_win: String::new(),
            icon_textures: HashMap::new(),
            slug_to_id: HashMap::new(),
            name_to_id: HashMap::new(),
            champ_names: HashMap::new(),
            teammates: vec![],
            selected_teammate_idx: None,
            selected_enemy_pos: String::new(),
            lcu_auth: None,
            match_history: vec![],
            match_history_cache: HashMap::new(),
            match_history_loading: false,
            match_history_name: String::new(),
            history_panel_open: false,
        }
    }

    fn process_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                BgMsg::Lcu(state) => {
                    self.connected = state.connected;
                    // 只在有新数据时更新 enemies，断线保留旧数据
                    if !state.enemies.is_empty() || state.error.is_empty() {
                        self.enemies = state.enemies;
                    }
                    if !state.teammates.is_empty() {
                        self.teammates = state.teammates;
                    }
                    if !state.error.is_empty() {
                        self.error = state.error;
                    } else {
                        self.error.clear();
                    }
                    if !state.my_pos.is_empty() {
                        self.my_pos = state.my_pos;
                    }
                    self.lane_enemy_id = state.lane_enemy_id;
                    self.champion_lang = state.champion_lang;
                    if state.auth.is_some() {
                        self.lcu_auth = state.auth;
                    }
                    let now = chrono::Local::now();
                    self.last_update_time = now.format("%H:%M:%S").to_string();

                    // 加载英雄图标纹理（一次性）
                    if let Some(data) = state.champion_data {
                        self.slug_to_id = data.slug_to_id;
                        self.name_to_id = data.name_to_id;
                        self.champ_names = data.id_to_name;
                        for (id, (rgba, w, h)) in data.icons {
                            let image = ColorImage::from_rgba_unmultiplied(
                                [w as usize, h as usize],
                                &rgba,
                            );
                            self.icon_textures.entry(id).or_insert_with(|| {
                                ctx.load_texture(
                                    format!("champ_{id}"),
                                    image,
                                    egui::TextureOptions::LINEAR,
                                )
                            });
                        }
                    }
                }
                BgMsg::MatchHistory { cache_key, name, url, entries } => {
                    self.match_history_loading = false;
                    self.match_history_name = name;
                    match entries {
                        Ok(data) if !data.is_empty() => {
                            self.match_history_cache.insert(cache_key, data.clone());
                            self.match_history = data;
                        }
                        Ok(_) => {
                            self.match_history = vec![];
                            self.match_history_name += &format!("\n暂无数据\n{url}");
                        }
                        Err(e) => {
                            self.match_history = vec![];
                            self.match_history_name += &format!("\n错误: {e}\n{url}");
                        }
                    }
                }
                BgMsg::UpdateProgress(done, total, name) => {
                    self.update_progress_text = format!("更新中：{done}/{total} - {name}");
                }
                BgMsg::UpdateDone(result) => {
                    self.updating = false;
                    match result {
                        Ok(cache) => {
                            self.opgg_cache = cache;
                            self.update_progress_text.clear();
                            // 刷新当前克制数据
                            if !self.counter_champ_slug.is_empty() {
                                self.counter_data = opgg::get_counters_for_champion(
                                    &self.opgg_cache,
                                    &self.counter_champ_slug,
                                    &self.my_pos,
                                );
                                self.counter_error = if self.counter_data.is_empty() {
                                    "未找到克制数据".into()
                                } else {
                                    String::new()
                                };
                            }
                        }
                        Err(e) => {
                            self.update_progress_text = format!("更新失败：{e}");
                        }
                    }
                }
                BgMsg::AiChunk(chunk) => {
                    self.ai_text.push_str(&chunk);
                }
                BgMsg::AiDone { cache_key, full_text } => {
                    self.ai_loading = false;
                    self.ai_cache.insert(cache_key, full_text);
                }
                BgMsg::AiError(err) => {
                    self.ai_loading = false;
                    self.ai_text.push_str(&format!("\n\n错误：{err}"));
                }
            }
        }
    }

    fn data_time_text(&self) -> String {
        if self.opgg_cache.updated_at > 0.0 {
            let ts = self.opgg_cache.updated_at as i64;
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default();
            format!(
                "数据：{}/{} | 更新：{dt}",
                self.opgg_cache.counters.len(),
                self.opgg_cache.total_entries
            )
        } else {
            "本地无数据，请点击「全量更新」".into()
        }
    }

    fn load_counter_data(&mut self, slug: &str, name: &str, pos: &str) {
        self.counter_champ_slug = slug.to_string();
        self.counter_champ_name = name.to_string();
        self.counter_data =
            opgg::get_counters_for_champion(&self.opgg_cache, slug, pos);
        self.counter_error = if self.counter_data.is_empty() {
            "本地无数据，请先点击「全量更新」".into()
        } else {
            String::new()
        };
    }

    fn sort_counter_data(&mut self) {
        let desc = self.counter_sort_desc;
        let favs = &self.counter_favorites;
        // 所有排序：收藏优先（置顶），再按选中列排序
        match self.counter_sort_col.as_str() {
            "name" => {
                self.counter_data.sort_by(|a, b| {
                    favs.contains(&b.key).cmp(&favs.contains(&a.key))
                        .then_with(|| if desc { b.name.cmp(&a.name) } else { a.name.cmp(&b.name) })
                });
            }
            "games" => {
                self.counter_data.sort_by(|a, b| {
                    favs.contains(&b.key).cmp(&favs.contains(&a.key))
                        .then_with(|| if desc { b.games.cmp(&a.games) } else { a.games.cmp(&b.games) })
                });
            }
            "fav" => {
                self.counter_data.sort_by(|a, b| {
                    let fa = favs.contains(&a.key);
                    let fb = favs.contains(&b.key);
                    if desc { fb.cmp(&fa) } else { fa.cmp(&fb) }
                });
            }
            _ => {
                self.counter_data.sort_by(|a, b| {
                    favs.contains(&b.key).cmp(&favs.contains(&a.key))
                        .then_with(|| if desc {
                            b.win_rate.partial_cmp(&a.win_rate).unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a.win_rate.partial_cmp(&b.win_rate).unwrap_or(std::cmp::Ordering::Equal)
                        })
                });
            }
        }
    }

    fn start_update(&mut self, ctx: &egui::Context) {
        if self.updating {
            return;
        }
        self.updating = true;
        self.update_progress_text = "正在获取英雄列表…".into();

        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let progress_tx = tx.clone();
            let progress_ctx = ctx.clone();
            let progress = Arc::new(move |done: usize, total: usize, name: &str| {
                let _ = progress_tx.send(BgMsg::UpdateProgress(done, total, name.to_string()));
                progress_ctx.request_repaint();
            });
            let result = opgg::fetch_all_counters(Some(progress)).await;
            let _ = tx.send(BgMsg::UpdateDone(result));
            ctx.request_repaint();
        });
    }

    fn start_ai_analysis(
        &mut self,
        counter_name: &str,
        enemy_name: &str,
        position: &str,
        win_rate: f64,
        ctx: &egui::Context,
    ) {
        let engine = match self.ai_engines.get(self.ai_engine_idx) {
            Some(e) => e.clone(),
            None => {
                self.ai_text = "错误：未配置 AI 引擎，请在 config.toml 中设置。".into();
                return;
            }
        };
        let models = engine.get_models();
        let model = models.get(self.ai_model_idx).or(models.first())
            .cloned().unwrap_or_default();
        let cache_key = format!("{counter_name}|{enemy_name}|{position}|{}|{model}", engine.name);
        if let Some(cached) = self.ai_cache.get(&cache_key) {
            self.ai_title = "AI 分析（缓存）".into();
            self.ai_text = cached.clone();
            return;
        }

        self.ai_loading = true;
        self.ai_title = "AI 分析".into();
        self.ai_text.clear();
        self.ai_cache_key = cache_key.clone();

        let tx = self.tx.clone();
        let ctx2 = ctx.clone();
        let my_champ = counter_name.to_string();
        let enemy_champ = enemy_name.to_string();
        let pos = position.to_string();
        let ck = cache_key;

        self.rt.spawn(async move {
            let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel();

            // 启动流式请求
            let stream_ctx = ctx2.clone();
            let stream_handle = tokio::spawn(async move {
                openai::call_ai_stream(&engine, &model, &my_champ, &enemy_champ, &pos, win_rate, chunk_tx, stream_ctx).await;
            });

            // 转发流式消息到主 channel
            while let Some(msg) = chunk_rx.recv().await {
                match msg {
                    openai::AiStreamMsg::Chunk(text) => {
                        let _ = tx.send(BgMsg::AiChunk(text));
                    }
                    openai::AiStreamMsg::Done(full_text) => {
                        let _ = tx.send(BgMsg::AiDone { cache_key: ck, full_text });
                        ctx2.request_repaint();
                        break;
                    }
                    openai::AiStreamMsg::Error(err) => {
                        let _ = tx.send(BgMsg::AiError(err));
                        ctx2.request_repaint();
                        break;
                    }
                }
                ctx2.request_repaint();
            }

            let _ = stream_handle.await;
        });
    }
    fn start_ai_chat(&mut self, user_prompt: &str, ctx: &egui::Context) {
        let engine = match self.ai_engines.get(self.ai_engine_idx) {
            Some(e) => e.clone(),
            None => {
                self.ai_text = "错误：未配置 AI 引擎，请在 config.toml 中设置。".into();
                return;
            }
        };
        let models = engine.get_models();
        let model = models.get(self.ai_model_idx).or(models.first())
            .cloned().unwrap_or_default();

        self.ai_loading = true;
        self.ai_title = "AI 对话".into();
        self.ai_text.clear();

        let tx = self.tx.clone();
        let ctx2 = ctx.clone();
        let prompt = user_prompt.to_string();

        self.rt.spawn(async move {
            let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel();

            let stream_ctx = ctx2.clone();
            let stream_handle = tokio::spawn(async move {
                openai::call_ai_raw(&engine, &model, "你是一个有用的助手。用简洁中文回答。", &prompt, chunk_tx, stream_ctx).await;
            });

            while let Some(msg) = chunk_rx.recv().await {
                match msg {
                    openai::AiStreamMsg::Chunk(text) => {
                        let _ = tx.send(BgMsg::AiChunk(text));
                    }
                    openai::AiStreamMsg::Done(_) => {
                        let _ = tx.send(BgMsg::AiDone { cache_key: String::new(), full_text: String::new() });
                        ctx2.request_repaint();
                        break;
                    }
                    openai::AiStreamMsg::Error(err) => {
                        let _ = tx.send(BgMsg::AiError(err));
                        ctx2.request_repaint();
                        break;
                    }
                }
                ctx2.request_repaint();
            }

            let _ = stream_handle.await;
        });
    }

    fn start_fetch_match_history(&mut self, game_name: &str, tag_line: &str, display_name: &str, ctx: &egui::Context) {
        let cache_key = format!("{game_name}-{tag_line}");
        // 检查缓存
        if let Some(cached) = self.match_history_cache.get(&cache_key) {
            self.match_history = cached.clone();
            self.match_history_name = display_name.to_string();
            self.match_history_loading = false;
            return;
        }
        if game_name.is_empty() || tag_line.is_empty() { return; }
        self.match_history_loading = true;
        self.match_history_name = format!("{display_name} (加载中…)");
        self.match_history = vec![];

        let region = self.config.region.clone();
        let game_name = game_name.to_string();
        let tag_line = tag_line.to_string();
        let name = display_name.to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.rt.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap();
            let url = opgg::match_history_url(&region, &game_name, &tag_line);
            let result = opgg::fetch_match_history(&client, &region, &game_name, &tag_line).await;
            let ck = format!("{game_name}-{tag_line}");
            let _ = tx.send(BgMsg::MatchHistory { cache_key: ck, name, url, entries: result });
            ctx.request_repaint();
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 启动 LCU poller（需要 ctx）
        if !self.lcu_started {
            self.lcu_started = true;
            // 强制重置窗口大小（覆盖 persistence 缓存的旧尺寸）
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(365.0, 900.0)));

            let (lcu_tx, mut lcu_rx) = mpsc::unbounded_channel();
            let lockfile_dir = self.config.lockfile_dir.clone();
            lcu::spawn_lcu_poller(self.rt.clone(), lockfile_dir, lcu_tx, ctx.clone());

            // 转发 LCU 消息到主 channel
            let tx = self.tx.clone();
            let ctx2 = ctx.clone();
            self.rt.spawn(async move {
                while let Some(state) = lcu_rx.recv().await {
                    let _ = tx.send(BgMsg::Lcu(state));
                    ctx2.request_repaint();
                }
            });
        }

        self.process_messages(ctx);

        // 窗口吸附和最小化跟随
        let found_win = win32::find_lol_client_window();
        if let Some(ref lol_win) = found_win {
            let (scr_x, scr_y, scr_w, scr_h) = win32::virtual_screen_rect();
            self.debug_lol_win = format!(
                "找到窗口: L={} T={} R={} B={} min={} | 屏幕: x={} y={} w={} h={}",
                lol_win.left, lol_win.top, lol_win.right, lol_win.bottom,
                lol_win.minimized, scr_x, scr_y, scr_w, scr_h,
            );

            if self.autodock {
                let scale = ctx.pixels_per_point();
                // 物理像素 → 逻辑坐标
                let x = (lol_win.right as f32 + 6.0) / scale;
                let y = lol_win.top as f32 / scale;
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                    egui::pos2(x, y),
                ));
            }
        } else {
            self.debug_lol_win = "未找到 LOL 窗口".into();
        }

        // 置顶
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            if self.topmost {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            },
        ));

        // 对局历史侧面板
        let show_history = self.selected_teammate_idx.is_some();
        if show_history != self.history_panel_open {
            self.history_panel_open = show_history;
            // 动态调整窗口宽度
            if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
                let new_width = if show_history { rect.width() + 300.0 } else { (rect.width() - 300.0).max(365.0) };
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(new_width, rect.height())));
            }
        }
        if show_history {
            egui::SidePanel::right("match_history_panel")
                .exact_width(290.0)
                .show(ctx, |ui| { self.ui_match_history(ui); });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            self.ui_content(ui, ctx);
        });
    }
}

impl App {
    fn ui_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y;

        // === 选项 ===
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.topmost, "置顶");
            ui.checkbox(&mut self.autodock, "吸附");
            let debug_label = if self.show_debug { "调试 ▲" } else { "调试 ▼" };
            if ui.small_button(debug_label).clicked() {
                self.show_debug = !self.show_debug;
            }
        });

        if self.show_debug {
            if !self.debug_lol_win.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(100, 180, 255), &self.debug_lol_win);
            }
            let my_pos_text = if self.my_pos.is_empty() { "未识别" } else { pos_cn(&self.my_pos) };
            ui.label(format!(
                "连接：{} | 语言：{} | 位置：{} | 更新：{}",
                if self.connected { "YES" } else { "NO" },
                self.champion_lang, my_pos_text, self.last_update_time,
            ));
            if !self.error.is_empty() {
                ui.colored_label(egui::Color32::RED, &self.error);
            }
            ui.horizontal(|ui| {
                ui.label("添加敌方：");
                let hero_display = self.opgg_cache.champions.get(&self.debug_slug)
                    .cloned()
                    .unwrap_or_else(|| self.debug_slug.clone());
                egui::ComboBox::from_id_salt("debug_hero")
                    .selected_text(&hero_display)
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        for hero in &self.debug_hero_options {
                            let label = self.opgg_cache.champions.get(hero)
                                .cloned()
                                .unwrap_or_else(|| hero.clone());
                            ui.selectable_value(&mut self.debug_slug, hero.clone(), label);
                        }
                    });
                let pos_display = pos_cn(&self.debug_pos);
                egui::ComboBox::from_id_salt("debug_pos")
                    .selected_text(pos_display)
                    .show_ui(ui, |ui| {
                        for pos in &self.debug_pos_options {
                            let label = pos_cn(pos);
                            ui.selectable_value(&mut self.debug_pos, pos.clone(), label);
                        }
                    });
                if ui.button("添加").clicked() {
                    let slug = self.debug_slug.trim().to_lowercase().replace(' ', "-");
                    if !slug.is_empty() {
                        let cn_name = self.opgg_cache.champions.get(&slug).cloned().unwrap_or_else(|| slug.clone());
                        let pos = self.debug_pos.clone();
                        self.my_pos = pos.clone();
                        self.enemies.push(EnemyInfo { champion_id: -1, name: cn_name, slug, pos });
                        self.connected = true;
                        self.error.clear();
                        self.last_update_time = chrono::Local::now().format("%H:%M:%S").to_string();
                    }
                }
                if ui.button("清空").clicked() {
                    self.enemies.clear();
                    self.selected_enemy_idx = None;
                    self.lane_enemy_id = None;
                    self.my_pos.clear();
                }
            });
        }

        ui.separator();

        // === 敌方英雄 / 我方队友 ===
        let mut clicked_idx: Option<usize> = None;
        let mut clicked_teammate: Option<usize> = None;
        let half_w: f32 = 170.0;
        let list_h = row_h * 5.0;

        let section_h = list_h + row_h * 1.5; // 标题 + 列表
        let section_w = half_w * 2.0 + 12.0; // 两列 + 分隔符 + 间距
        ui.allocate_ui(egui::vec2(section_w.min(ui.available_width()), section_h), |ui| {
        ui.horizontal_top(|ui| {
            // 左侧：敌方英雄
            ui.vertical(|ui| {
                ui.set_width(half_w);
                ui.label("对面英雄（⭐对线）：");
                egui::ScrollArea::vertical()
                    .id_salt("enemy_scroll")
                    .max_height(list_h)
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        for (i, enemy) in self.enemies.iter().enumerate() {
                            let is_lane = self.lane_enemy_id.map_or(false, |id| id == enemy.champion_id);
                            let star = if is_lane { "⭐ " } else { "" };
                            let pc = pos_cn(&enemy.pos);
                            let pos_text = if pc.is_empty() { String::new() } else { format!(" [{pc}]") };
                            let text = format!("{}{}{pos_text}", star, enemy.name);
                            let selected = self.selected_enemy_idx == Some(i);
                            let clicked = ui.horizontal(|ui| {
                                let tex = self.icon_textures.get(&enemy.champion_id)
                                    .or_else(|| self.slug_to_id.get(&enemy.slug)
                                        .and_then(|id| self.icon_textures.get(id)));
                                if let Some(tex) = tex {
                                    ui.image((tex.id(), egui::vec2(20.0, 20.0)));
                                }
                                ui.selectable_label(selected, &text).clicked()
                            }).inner;
                            if clicked { clicked_idx = Some(i); }
                        }
                    });
            });

            ui.separator();

            // 右侧：全部玩家
            ui.vertical(|ui| {
                ui.set_width(half_w);
                ui.label("全部玩家：");
                egui::ScrollArea::vertical()
                    .id_salt("teammate_scroll")
                    .max_height(list_h)
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        for (i, mate) in self.teammates.iter().enumerate() {
                            let rank_text = if mate.rank_tier.is_empty() {
                                "未定级".to_string()
                            } else {
                                format!("{}{} {}LP", rank_cn(&mate.rank_tier), mate.rank_division, mate.rank_lp)
                            };
                            let team_color = if mate.is_ally {
                                egui::Color32::from_rgb(60, 140, 220)
                            } else {
                                egui::Color32::from_rgb(220, 70, 70)
                            };
                            let selected = self.selected_teammate_idx == Some(i);
                            let clicked = ui.horizontal(|ui| {
                                ui.colored_label(team_color, if mate.is_ally { "友" } else { "敌" });
                                if let Some(tex) = self.icon_textures.get(&mate.champion_id) {
                                    ui.image((tex.id(), egui::vec2(20.0, 20.0)));
                                }
                                ui.selectable_label(selected, format!("{} {}", mate.summoner_name, rank_text)).clicked()
                            }).inner;
                            if clicked { clicked_teammate = Some(i); }
                        }
                    });
            });
        });
        }); // allocate_ui

        if let Some(idx) = clicked_idx {
            self.selected_enemy_idx = Some(idx);
            let enemy = self.enemies[idx].clone();
            self.selected_enemy_pos = if enemy.pos.is_empty() { self.my_pos.clone() } else { enemy.pos.clone() };
            self.load_counter_data(&enemy.slug, &enemy.name, &self.selected_enemy_pos.clone());
        }
        if let Some(idx) = clicked_teammate {
            if self.selected_teammate_idx == Some(idx) {
                // 再次点击取消选中，关闭面板
                self.selected_teammate_idx = None;
            } else {
                self.selected_teammate_idx = Some(idx);
                let mate = self.teammates[idx].clone();
                self.start_fetch_match_history(&mate.summoner_name, &mate.tag_line, &mate.summoner_name, ctx);
            }
        }

        // === 数据管理 ===
        ui.horizontal_wrapped(|ui| {
            let data_text = if self.updating { self.update_progress_text.clone() } else { self.data_time_text() };
            ui.label(&data_text);
            if ui.add_enabled(!self.updating, egui::Button::new("全量更新")).clicked() {
                self.start_update(ctx);
            }
        });

        // === 克制数据表格（固定10行高度）===
        let mut pos_changed = false;
        ui.horizontal(|ui| {
            if self.counter_champ_name.is_empty() {
                ui.label("克制数据（选择上方英雄后显示）：");
            } else {
                ui.label(format!("克制 - {}", self.counter_champ_name));
                let old_pos = self.selected_enemy_pos.clone();
                let pos_display = pos_cn(&self.selected_enemy_pos);
                let pos_display = if pos_display.is_empty() { "选择位置" } else { pos_display };
                egui::ComboBox::from_id_salt("enemy_pos_select")
                    .selected_text(pos_display)
                    .width(60.0)
                    .show_ui(ui, |ui| {
                        for pos in &["TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY"] {
                            ui.selectable_value(&mut self.selected_enemy_pos, pos.to_string(), pos_cn(pos));
                        }
                    });
                if self.selected_enemy_pos != old_pos {
                    pos_changed = true;
                }
                let order = if self.counter_sort_desc { "降序" } else { "升序" };
                ui.label(format!("（{}个，{order}）", self.counter_data.len()));
            }
        });
        if pos_changed {
            let slug = self.counter_champ_slug.clone();
            let name = self.counter_champ_name.clone();
            let pos = self.selected_enemy_pos.clone();
            self.load_counter_data(&slug, &name, &pos);
        }
        if !self.counter_error.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(180, 120, 0), &self.counter_error);
        }

        self.sort_counter_data();
        let mut ai_trigger: Option<(String, f64)> = None;

        egui::ScrollArea::vertical()
            .id_salt("counter_scroll")
            .max_height(row_h * 11.0)
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Grid::new("counter_grid")
                    .striped(true)
                    .show(ui, |ui| {
                        // 表头
                        let mut header_click = None;
                        if ui.button("英雄").clicked() { header_click = Some("name"); }
                        if ui.button("克制率(%)").clicked() { header_click = Some("win_rate"); }
                        if ui.button("场次").clicked() { header_click = Some("games"); }
                        if ui.button("收藏").clicked() { header_click = Some("fav"); }
                        ui.end_row();

                        if let Some(col) = header_click {
                            if self.counter_sort_col == col {
                                self.counter_sort_desc = !self.counter_sort_desc;
                            } else {
                                self.counter_sort_col = col.to_string();
                                self.counter_sort_desc = true;
                            }
                            self.sort_counter_data();
                        }

                        let mut fav_toggle: Option<String> = None;
                        for row in &self.counter_data {
                            let icon_id = self.slug_to_id.get(&row.key)
                                .or_else(|| self.name_to_id.get(&row.name))
                                .copied();
                            let clicked = ui.horizontal(|ui| {
                                if let Some(tex) = icon_id.and_then(|id| self.icon_textures.get(&id)) {
                                    ui.image((tex.id(), egui::vec2(18.0, 18.0)));
                                }
                                ui.selectable_label(false, &row.name).clicked()
                            }).inner;
                            if clicked {
                                ai_trigger = Some((row.name.clone(), row.win_rate));
                            }
                            ui.label(format!("{:.2}%", row.win_rate));
                            ui.label(format!("{}", row.games));
                            let mut is_fav = self.counter_favorites.contains(&row.key);
                            if ui.checkbox(&mut is_fav, "").clicked() {
                                fav_toggle = Some(row.key.clone());
                            }
                            ui.end_row();
                        }

                        if let Some(key) = fav_toggle {
                            if !self.counter_favorites.remove(&key) {
                                self.counter_favorites.insert(key);
                            }
                            save_favorites(&self.counter_favorites);
                        }
                    });
            });

        if let Some((counter_name, win_rate)) = ai_trigger {
            let enemy_name = self.counter_champ_name.clone();
            let opgg_pos = lcu_pos_to_opgg(&self.my_pos).to_string();
            self.start_ai_analysis(&counter_name, &enemy_name, &opgg_pos, win_rate, ctx);
        }

        ui.separator();

        // === AI 分析面板（占满剩余空间）===
        ui.horizontal(|ui| {
            ui.label(&self.ai_title);
            if self.ai_engines.len() > 1 {
                let old_engine_idx = self.ai_engine_idx;
                let current_name = self.ai_engines.get(self.ai_engine_idx)
                    .map(|e| e.name.as_str()).unwrap_or("未配置");
                egui::ComboBox::from_id_salt("ai_engine_select")
                    .selected_text(current_name)
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for (i, engine) in self.ai_engines.iter().enumerate() {
                            ui.selectable_value(&mut self.ai_engine_idx, i, &engine.name);
                        }
                    });
                // 切换引擎时重置模型索引
                if self.ai_engine_idx != old_engine_idx {
                    self.ai_model_idx = 0;
                }
            } else if self.ai_engines.len() == 1 {
                ui.weak(&self.ai_engines[0].name);
            }
            // 模型选择（当前引擎有多个模型时显示）
            if let Some(engine) = self.ai_engines.get(self.ai_engine_idx) {
                let models = engine.get_models();
                if models.len() > 1 {
                    let current_model = models.get(self.ai_model_idx)
                        .or(models.first())
                        .map(|s| s.as_str()).unwrap_or("?");
                    egui::ComboBox::from_id_salt("ai_model_select")
                        .selected_text(current_model)
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            for (i, m) in models.iter().enumerate() {
                                ui.selectable_value(&mut self.ai_model_idx, i, m);
                            }
                        });
                }
            }
            if self.ai_loading {
                ui.spinner();
            }
            let chat_label = if self.ai_chat_visible { "对话 ▲" } else { "对话 ▼" };
            if ui.small_button(chat_label).clicked() {
                self.ai_chat_visible = !self.ai_chat_visible;
            }
        });
        // 对话输入框
        let mut send_chat = false;
        if self.ai_chat_visible {
            ui.horizontal(|ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.ai_chat_input)
                        .hint_text("输入自定义提示词…")
                        .desired_width(ui.available_width() - 45.0),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    send_chat = true;
                }
                if ui.add_enabled(!self.ai_loading && !self.ai_chat_input.trim().is_empty(), egui::Button::new("发送")).clicked() {
                    send_chat = true;
                }
            });
        }
        if send_chat && !self.ai_chat_input.trim().is_empty() && !self.ai_loading {
            let prompt = self.ai_chat_input.clone();
            self.ai_chat_input.clear();
            self.start_ai_chat(&prompt, ctx);
        }
        egui::ScrollArea::vertical()
            .id_salt("ai_scroll")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                if self.ai_text.is_empty() {
                    ui.label("点击上方克制英雄触发 AI 分析");
                } else {
                    ui.label(&self.ai_text);
                }
            });
    }

    fn ui_match_history(&self, ui: &mut egui::Ui) {
        ui.heading(&self.match_history_name);
        ui.separator();

        if self.match_history_loading {
            ui.spinner();
            ui.label("加载中…");
            return;
        }

        if self.match_history.is_empty() {
            ui.label("暂无对局记录");
            return;
        }

        // 统计胜率
        let wins = self.match_history.iter().filter(|e| e.win).count();
        let total = self.match_history.len();
        let wr = if total > 0 { wins as f64 / total as f64 * 100.0 } else { 0.0 };
        ui.label(format!("近{total}场：{wins}胜{}负 ({wr:.0}%)", total - wins));
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("history_scroll")
            .auto_shrink(false)
            .show(ui, |ui| {
                for entry in &self.match_history {
                    let champ_name = if !entry.champion_name.is_empty() {
                        entry.champion_name.clone()
                    } else if entry.champion_id > 0 {
                        self.champ_names.get(&entry.champion_id)
                            .cloned()
                            .unwrap_or_else(|| entry.champion_key.clone())
                    } else {
                        entry.champion_key.clone()
                    };
                    let result_text = if entry.win { "胜" } else { "败" };
                    let result_color = if entry.win {
                        egui::Color32::from_rgb(60, 180, 80)
                    } else {
                        egui::Color32::from_rgb(220, 60, 60)
                    };
                    let kda = format!("{}/{}/{}", entry.kills, entry.deaths, entry.assists);
                    let kda_ratio = if entry.deaths > 0 {
                        format!("{:.1}", (entry.kills + entry.assists) as f64 / entry.deaths as f64)
                    } else {
                        "Perfect".into()
                    };
                    let duration = format!("{}:{:02}", entry.game_duration_secs / 60, entry.game_duration_secs % 60);
                    let mode = if !entry.game_type.is_empty() {
                        opgg_queue_cn(&entry.game_type)
                    } else {
                        queue_name(entry.queue_id)
                    };

                    ui.horizontal(|ui| {
                        let icon_id = if entry.champion_id > 0 {
                            Some(entry.champion_id)
                        } else {
                            self.slug_to_id.get(&entry.champion_key).copied()
                        };
                        if let Some(tex) = icon_id.and_then(|id| self.icon_textures.get(&id)) {
                            ui.image((tex.id(), egui::vec2(28.0, 28.0)));
                        }
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.colored_label(result_color, result_text);
                                ui.label(&champ_name);
                                ui.label(&kda);
                                ui.colored_label(egui::Color32::from_rgb(180, 180, 100), format!("({kda_ratio})"));
                            });
                            ui.horizontal(|ui| {
                                ui.label(mode);
                                ui.label(&duration);
                                if entry.timestamp_ms > 0 {
                                    let secs_ago = chrono::Local::now().timestamp() - entry.timestamp_ms / 1000;
                                    let ago = if secs_ago < 3600 {
                                        format!("{}分钟前", secs_ago / 60)
                                    } else if secs_ago < 86400 {
                                        format!("{}小时前", secs_ago / 3600)
                                    } else {
                                        format!("{}天前", secs_ago / 86400)
                                    };
                                    ui.label(&ago);
                                }
                            });
                        });
                    });
                    ui.separator();
                }
            });
    }
}
