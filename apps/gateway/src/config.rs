use std::{
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use orchestrator_core::ProviderKind;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    pub workspace_dir: PathBuf,
    pub frontend_dir: PathBuf,
    pub llama_cpp_model_dir: PathBuf,
    pub ollama_api_base: String,
    pub codex_daily_soft_limit: Option<i64>,
    pub claude_daily_soft_limit: Option<i64>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind_addr = env::var("BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:3000".to_string())
            .parse()?;

        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://chatbot:chatbot@localhost:5433/chatbot".to_string());

        let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_dir = env::var("WORKSPACE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| crate_dir.join("../.."));
        let frontend_dir = env::var("FRONTEND_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| crate_dir.join("../web-lab/dist"));
        let llama_cpp_model_dir = env::var("LLAMA_CPP_MODEL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| crate_dir.join("../../var/models/llama.cpp"));
        let ollama_api_base =
            env::var("OLLAMA_API_BASE").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());

        Ok(Self {
            bind_addr,
            database_url,
            workspace_dir: normalize_path(workspace_dir),
            frontend_dir: normalize_path(frontend_dir),
            llama_cpp_model_dir: normalize_path(llama_cpp_model_dir),
            ollama_api_base: ollama_api_base.trim_end_matches('/').to_string(),
            codex_daily_soft_limit: parse_optional_i64("CODEX_DAILY_SOFT_LIMIT_TOKENS")?,
            claude_daily_soft_limit: parse_optional_i64("CLAUDE_DAILY_SOFT_LIMIT_TOKENS")?,
        })
    }

    pub fn soft_limit_for(&self, provider: ProviderKind) -> Option<i64> {
        match provider {
            ProviderKind::Codex => self.codex_daily_soft_limit,
            ProviderKind::Claude => self.claude_daily_soft_limit,
            ProviderKind::Ollama | ProviderKind::LlamaCpp => None,
        }
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        Path::new(".").join(path)
    }
}

fn parse_optional_i64(key: &str) -> anyhow::Result<Option<i64>> {
    match env::var(key) {
        Ok(value) => Ok(Some(value.parse()?)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error.into()),
    }
}
