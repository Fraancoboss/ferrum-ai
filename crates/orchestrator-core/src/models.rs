use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Codex,
    Claude,
    Ollama,
    LlamaCpp,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Ollama => "ollama",
            Self::LlamaCpp => "llama_cpp",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Ollama => "Ollama",
            Self::LlamaCpp => "llama.cpp",
        }
    }

    pub fn is_local(self) -> bool {
        matches!(self, Self::Ollama | Self::LlamaCpp)
    }

    pub fn requires_auth(self) -> bool {
        matches!(self, Self::Codex | Self::Claude)
    }

    pub fn supports_resume(self) -> bool {
        matches!(self, Self::Codex | Self::Claude)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthStatus {
    Unknown,
    Authenticated,
    LoggedOut,
    MissingDependency,
    Error,
    InProgress,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    RunStarted,
    AssistantDelta,
    AssistantFinal,
    UsageUpdated,
    ProviderSessionBound,
    AuthOutput,
    AuthUrl,
    StdErr,
    RunCompleted,
    RunFailed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ProviderSessionRef {
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LlmUsage {
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedEvent {
    pub event_kind: EventKind,
    pub provider: ProviderKind,
    pub sequence: i64,
    pub raw: Value,
    pub text: Option<String>,
    pub usage: Option<LlmUsage>,
    pub provider_session_ref: Option<String>,
    pub created_at: DateTime<Utc>,
}
