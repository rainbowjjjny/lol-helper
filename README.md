# LoL Helper

英雄联盟桌面辅助工具，在选人阶段实时提供 **OP.GG 克制数据**和 **ChatGPT 对线分析**。通过 LCU API 自动连接客户端，无需手动操作。

![Rust](https://img.shields.io/badge/Rust-2021-orange)
![Platform](https://img.shields.io/badge/Platform-Windows-blue)

## 功能

- **自动连接客户端** — 通过 LCU API 检测选人阶段，自动识别敌方英雄和队友
- **OP.GG 克制数据** — 本地缓存全英雄克制胜率和场次数据，支持全量更新
- **手动选位** — 克制数据支持手动切换位置（上/打野/中/下/辅），适应 Flex 英雄
- **AI 对线分析** — 点击克制英雄，调用 ChatGPT 生成针对性对线建议
- **全局玩家信息** — 展示当局全部 10 名玩家的段位信息（单双排）
- **OP.GG 战绩查询** — 点击任意玩家查看近期对局记录、胜率、KDA
- **窗口吸附** — 自动吸附到客户端窗口右侧，跟随最小化/恢复
- **收藏英雄** — 常用克制英雄置顶显示

## 截图

> 启动程序后自动连接客户端，进入选人阶段即可使用。

## 快速开始

### 1. 配置

在可执行文件同目录下创建 `config.toml`：

```toml
openai_api_key = "sk-proj-xxx"       # 必填：OpenAI API Key
openai_model = "gpt-4o"              # 可选：模型名称
lockfile_dir = ""                     # 可选：手动指定 LoL 客户端 lockfile 所在目录
```

### 2. 运行

直接运行 `lol-helper.exe`，程序会自动查找 LoL 客户端并连接。

## 从源码构建

### 前置要求

- Rust 工具链（MSVC）
- Windows 10/11

### 构建

```bash
cargo build --release
```

输出文件：`target/release/lol-helper.exe`

也可以使用 `build.bat`，会自动设置 MSVC 环境变量。

## 技术架构

```
main.rs    — 入口：tokio 运行时 + eframe 窗口
app.rs     — UI 状态机、消息处理、界面渲染
lcu.rs     — LCU API 轮询（后台任务，自动检测选人阶段）
opgg.rs    — OP.GG 克制数据抓取 + 本地缓存
openai.rs  — ChatGPT API 对线分析
win32.rs   — Win32 窗口管理（吸附、最小化跟随）
config.rs  — 配置文件加载
types.rs   — 共享类型定义
```

### 数据流

1. `lcu.rs` 轮询 LCU API → 检测选人阶段 → 发送敌方/队友/对手数据
2. 用户选择敌方英雄 → 查询本地 OP.GG 缓存，展示克制数据
3. 用户点击克制英雄 → 调用 ChatGPT 生成对线分析
4. 用户点击任意玩家 → 从 OP.GG 获取近期战绩

### 技术栈

| 组件 | 依赖 |
|------|------|
| GUI | egui 0.31 + eframe |
| 异步 | tokio |
| HTTP | reqwest (rustls-tls) |
| 序列化 | serde + serde_json |
| Windows API | windows 0.58 |

## 注意事项

- **请勿提交 `config.toml`** — 包含 API Key
- 英雄头像在运行时从 LCU 下载，缓存在内存中
- OP.GG 数据本地缓存为 `opgg_data.json`，首次使用需点击「全量更新」
- 仅支持 Windows 平台

## License

MIT
