use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub openai_api_key: String,
    #[serde(default = "default_model")]
    pub openai_model: String,
    #[serde(default)]
    pub lockfile_dir: String,
    #[serde(default = "default_region")]
    pub region: String,
}

fn default_model() -> String {
    "gpt-5.2-chat-latest".to_string()
}

fn default_region() -> String {
    "jp".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            openai_api_key: String::new(),
            openai_model: default_model(),
            lockfile_dir: String::new(),
            region: default_region(),
        }
    }
}

/// 获取 exe 同目录下的配置文件路径
fn config_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.parent().unwrap_or(std::path::Path::new(".")).join("config.toml")
}

pub fn load_config() -> AppConfig {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!("config.toml 解析失败: {e}");
            AppConfig::default()
        }),
        Err(_) => {
            eprintln!("未找到 config.toml ({})，使用默认配置", path.display());
            AppConfig::default()
        }
    }
}
