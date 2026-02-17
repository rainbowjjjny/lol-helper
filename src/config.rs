use serde::Deserialize;
use std::path::PathBuf;

/// 单个 AI 引擎配置
#[derive(Debug, Deserialize, Clone)]
pub struct AiEngine {
    pub name: String,
    #[serde(default = "default_api_url")]
    pub api_url: String,
    pub api_key: String,
    /// 兼容旧配置：单个模型
    #[serde(default)]
    pub model: String,
    /// 多模型列表
    #[serde(default)]
    pub models: Vec<String>,
}

impl AiEngine {
    /// 获取该引擎的模型列表（兼容 model / models 两种写法）
    pub fn get_models(&self) -> Vec<String> {
        if !self.models.is_empty() {
            return self.models.clone();
        }
        if !self.model.is_empty() {
            return vec![self.model.clone()];
        }
        vec![]
    }
}

fn default_api_url() -> String {
    "https://api.openai.com/v1/chat/completions".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    /// 兼容旧配置
    #[serde(default)]
    pub openai_api_key: String,
    #[serde(default = "default_model")]
    pub openai_model: String,
    #[serde(default)]
    pub lockfile_dir: String,
    #[serde(default = "default_region")]
    pub region: String,
    /// 多 AI 引擎列表
    #[serde(default)]
    pub ai_engines: Vec<AiEngine>,
}

fn default_model() -> String {
    "gpt-4o".to_string()
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
            ai_engines: vec![],
        }
    }
}

impl AppConfig {
    /// 获取最终的 AI 引擎列表（兼容旧配置）
    pub fn get_engines(&self) -> Vec<AiEngine> {
        if !self.ai_engines.is_empty() {
            return self.ai_engines.clone();
        }
        // 兼容旧的 openai_api_key / openai_model 配置
        if !self.openai_api_key.is_empty() && self.openai_api_key != "sk-proj-xxx" {
            vec![AiEngine {
                name: format!("OpenAI ({})", self.openai_model),
                api_url: default_api_url(),
                api_key: self.openai_api_key.clone(),
                model: self.openai_model.clone(),
                models: vec![],
            }]
        } else {
            vec![]
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
