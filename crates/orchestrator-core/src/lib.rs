mod commands;
mod models;
mod parser;

pub use commands::{
    AuthAction, CommandSpec, ProviderAdapter, ProviderDiagnostic, ProviderInstallStatus,
    RunAccumulator, RunMode, RunRequest, build_provider_status, detect_windows_git_bash,
};
pub use models::{
    AuthStatus, EventKind, LlmUsage, NormalizedEvent, ProviderKind, ProviderSessionRef, RunStatus,
};
pub use parser::{
    extract_assistant_text, extract_provider_session_ref, extract_usage, normalize_auth_line,
    normalize_stderr_line, normalize_stream_line,
};
