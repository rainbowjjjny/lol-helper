# LoL Helper - Claude Code Project Guide

## Project Overview

A League of Legends companion desktop app that provides real-time counter-pick data from OP.GG and AI-powered lane analysis via ChatGPT during champion select. It connects to the LoL Client (LCU API) to detect game state automatically.

There are two implementations:
- **Rust (primary):** `src/` — egui-based GUI, async with tokio, built with `cargo build --release`
- **Python (legacy/prototype):** `b.py` — tkinter GUI, same feature set

## Tech Stack

- **Language:** Rust 2021 edition
- **GUI:** egui 0.31 + eframe (glow backend, persistence)
- **Async:** tokio (full features)
- **HTTP:** reqwest (JSON + rustls-tls)
- **Serialization:** serde + serde_json, toml
- **Windows APIs:** `windows` crate 0.58 (window enumeration, hotkeys, process info)
- **Other:** regex, chrono, base64, once_cell, image (PNG)

## Architecture

```
main.rs          — Entry point, tokio runtime + eframe window setup
app.rs           — Core UI state machine, message processing, rendering
lcu.rs           — LCU API poller (background task via mpsc channel)
opgg.rs          — OP.GG web scraper for counter-pick data + local cache
openai.rs        — ChatGPT API integration for lane analysis
win32.rs         — Win32 window management, hotkey listener (tilde key)
config.rs        — config.toml loader (API key, model, lockfile dir)
types.rs         — Shared data types and utility functions
```

### Data Flow

1. `lcu.rs` polls the LCU API → detects champion select → sends enemy/teammate data via `BgMsg::Lcu`
2. User selects enemy → `app.rs` queries local OP.GG cache (`opgg_data.json`)
3. User clicks counter champion → spawns `openai.rs` AI analysis → `BgMsg::AiResult`
4. User clicks teammate → fetches match history from LCU → `BgMsg::MatchHistory`
5. All background tasks communicate via `mpsc::UnboundedReceiver<BgMsg>` in `app.rs`

## Build & Run

```bat
# Uses MSVC toolchain — build.bat sets up vcvars64 environment
build.bat

# Or directly:
cargo build --release
```

Output binary: `target/release/lol-helper.exe`

## Configuration

`config.toml` (next to executable):
```toml
openai_api_key = "sk-proj-xxx"       # Required: OpenAI API key
openai_model = "gpt-5.2-chat-latest" # Optional: model name
lockfile_dir = ""                     # Optional: custom LoL lockfile directory
```

## Key Conventions

- **Language:** UI text and comments are in Chinese (Simplified)
- **OP.GG scraping:** Parses Next.js RSC (React Server Components) push data from HTML — see `parse_rsc_push_data()` in `opgg.rs`
- **LCU connection:** Reads `lockfile` for port/password, uses Basic auth over HTTPS (self-signed cert, ignored)
- **Position mapping:** LCU uses `TOP/JUNGLE/MIDDLE/BOTTOM/UTILITY`, OP.GG uses `TOP/JUNGLE/MID/ADC/SUPPORT` — see `types.rs` for conversions
- **Local cache:** `opgg_data.json` stores all champion counter data with timestamps
- **Concurrency:** Semaphore-limited to 10 concurrent OP.GG requests during full update
- **Platform:** Windows-only for Win32 features; `win32.rs` has no-op stubs for non-Windows

## Important Notes

- **Do NOT commit** `config.toml` — it contains API keys
- The `b.py` Python file is a legacy prototype with the same features; the Rust version is the active codebase
- Champion icons are downloaded from LCU at runtime and cached in memory
- The app auto-docks to the right side of the LoL client window and follows its minimize/restore state

---

# LoL Helper - 项目指南（中文版）

## 项目概述

英雄联盟桌面辅助工具，在选人阶段实时提供 OP.GG 克制数据和 ChatGPT 对线分析。通过 LCU API 自动连接客户端，检测游戏状态。

项目包含两套实现：
- **Rust（主力）：** `src/` — 基于 egui 的 GUI，tokio 异步，`cargo build --release` 构建
- **Python（旧版/原型）：** `b.py` — tkinter GUI，功能相同

## 技术栈

- **语言：** Rust 2021 edition
- **GUI：** egui 0.31 + eframe（glow 后端，支持持久化）
- **异步：** tokio（全功能）
- **HTTP：** reqwest（JSON + rustls-tls）
- **序列化：** serde + serde_json、toml
- **Windows API：** `windows` crate 0.58（窗口枚举、热键、进程信息）
- **其他：** regex、chrono、base64、once_cell、image（PNG）

## 架构

```
main.rs          — 入口，tokio 运行时 + eframe 窗口初始化
app.rs           — 核心 UI 状态机，消息处理，界面渲染
lcu.rs           — LCU API 轮询器（后台任务，通过 mpsc 通道通信）
opgg.rs          — OP.GG 网页抓取克制数据 + 本地缓存
openai.rs        — ChatGPT API 集成，对线分析
win32.rs         — Win32 窗口管理，热键监听（波浪键）
config.rs        — config.toml 配置加载（API Key、模型、lockfile 路径）
types.rs         — 共享数据类型和工具函数
```

### 数据流

1. `lcu.rs` 轮询 LCU API → 检测到选人阶段 → 通过 `BgMsg::Lcu` 发送敌方/队友数据
2. 用户选择敌方英雄 → `app.rs` 查询本地 OP.GG 缓存（`opgg_data.json`）
3. 用户点击克制英雄 → 发起 `openai.rs` AI 分析 → `BgMsg::AiResult`
4. 用户点击队友 → 从 LCU 获取战绩 → `BgMsg::MatchHistory`
5. 所有后台任务通过 `app.rs` 中的 `mpsc::UnboundedReceiver<BgMsg>` 通信

## 构建与运行

```bat
# 使用 MSVC 工具链 — build.bat 会设置 vcvars64 环境
build.bat

# 或直接：
cargo build --release
```

输出文件：`target/release/lol-helper.exe`

## 配置

`config.toml`（放在可执行文件同目录）：
```toml
openai_api_key = "sk-proj-xxx"       # 必填：OpenAI API Key
openai_model = "gpt-5.2-chat-latest" # 可选：模型名称
lockfile_dir = ""                     # 可选：手动指定 lockfile 所在目录
```

## 关键约定

- **界面语言：** UI 文本和注释均为简体中文
- **OP.GG 抓取：** 解析 Next.js RSC（React Server Components）push 数据 — 见 `opgg.rs` 中的 `parse_rsc_push_data()`
- **LCU 连接：** 读取 `lockfile` 获取端口和密码，通过 HTTPS Basic Auth 连接（自签名证书，已忽略验证）
- **位置映射：** LCU 使用 `TOP/JUNGLE/MIDDLE/BOTTOM/UTILITY`，OP.GG 使用 `TOP/JUNGLE/MID/ADC/SUPPORT` — 转换逻辑见 `types.rs`
- **本地缓存：** `opgg_data.json` 存储所有英雄克制数据及更新时间戳
- **并发控制：** 全量更新时通过信号量限制最多 10 个并发 OP.GG 请求
- **平台：** 仅 Windows（Win32 功能）；`win32.rs` 对非 Windows 平台提供空实现

## 注意事项

- **禁止提交** `config.toml` — 包含 API Key
- `b.py` 是早期 Python 原型，功能相同；Rust 版本是当前主力代码
- 英雄头像在运行时从 LCU 下载，缓存在内存中
- 应用会自动吸附到 LoL 客户端窗口右侧，并跟随其最小化/恢复状态
