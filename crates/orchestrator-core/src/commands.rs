use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use which::which;

use crate::models::{AuthStatus, LlmUsage, ProviderKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

impl CommandSpec {
    pub fn display(&self) -> String {
        let mut parts = Vec::with_capacity(1 + self.args.len());
        parts.push(self.program.clone());
        parts.extend(self.args.clone());
        parts.join(" ")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthAction {
    Status,
    Login,
    Logout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    New,
    Resume,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderInstallStatus {
    Installed,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDiagnostic {
    pub provider: ProviderKind,
    pub status: ProviderInstallStatus,
    pub program: String,
    pub version: Option<String>,
    pub auth_status: AuthStatus,
    pub detail: Option<String>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub prompt: String,
    pub cwd: PathBuf,
    pub mode: RunMode,
    pub provider_session_ref: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunAccumulator {
    pub assistant_text: String,
    pub stderr_text: String,
    pub provider_session_ref: Option<String>,
    pub usage: Option<LlmUsage>,
}

#[derive(Debug, Clone)]
pub struct ProviderAdapter {
    pub provider: ProviderKind,
    pub program: String,
}

impl ProviderAdapter {
    pub fn new(provider: ProviderKind, program: impl Into<String>) -> Self {
        Self {
            provider,
            program: program.into(),
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self.provider {
            ProviderKind::Codex => "Codex CLI",
            ProviderKind::Claude => "Claude Code",
            ProviderKind::Ollama => "Ollama",
            ProviderKind::LlamaCpp => "llama.cpp",
        }
    }

    pub fn version_command(&self) -> CommandSpec {
        CommandSpec {
            program: self.program.clone(),
            args: vec!["--version".to_owned()],
            cwd: None,
        }
    }

    pub fn auth_command(&self, action: AuthAction) -> CommandSpec {
        let args = match (self.provider, action) {
            (ProviderKind::Codex, AuthAction::Status) => vec!["login", "status"],
            (ProviderKind::Codex, AuthAction::Login) => vec!["login", "--device-auth"],
            (ProviderKind::Codex, AuthAction::Logout) => vec!["logout"],
            (ProviderKind::Claude, AuthAction::Status) => vec!["auth", "status", "--json"],
            (ProviderKind::Claude, AuthAction::Login) => vec!["auth", "login"],
            (ProviderKind::Claude, AuthAction::Logout) => vec!["auth", "logout"],
            (ProviderKind::Ollama, _) => vec!["--version"],
            (ProviderKind::LlamaCpp, _) => vec!["--version"],
        };

        CommandSpec {
            program: self.program.clone(),
            args: args.into_iter().map(str::to_owned).collect(),
            cwd: None,
        }
    }

    pub fn run_command(&self, request: &RunRequest) -> CommandSpec {
        let mut args = match (self.provider, &request.mode) {
            (ProviderKind::Codex, RunMode::New) => vec![
                "exec".to_owned(),
                "--json".to_owned(),
                "--skip-git-repo-check".to_owned(),
            ],
            (ProviderKind::Codex, RunMode::Resume) => vec![
                "exec".to_owned(),
                "resume".to_owned(),
                request.provider_session_ref.clone().unwrap_or_default(),
                "--json".to_owned(),
                "--skip-git-repo-check".to_owned(),
            ],
            (ProviderKind::Claude, RunMode::New) => vec![
                "-p".to_owned(),
                "--output-format".to_owned(),
                "stream-json".to_owned(),
            ],
            (ProviderKind::Claude, RunMode::Resume) => vec![
                "-p".to_owned(),
                "--resume".to_owned(),
                request.provider_session_ref.clone().unwrap_or_default(),
                "--output-format".to_owned(),
                "stream-json".to_owned(),
            ],
            (ProviderKind::Ollama, _) => vec![
                "run".to_owned(),
                request
                    .model
                    .clone()
                    .unwrap_or_else(|| "llama3.1:8b".to_string()),
            ],
            (ProviderKind::LlamaCpp, _) => {
                let model = request
                    .model
                    .clone()
                    .unwrap_or_else(|| "var/models/llama.cpp/model.gguf".to_string());
                vec!["-m".to_owned(), model, "-p".to_owned()]
            }
        };

        if let Some(model) = request
            .model
            .clone()
            .filter(|value| !value.trim().is_empty())
        {
            match self.provider {
                ProviderKind::Codex | ProviderKind::Claude => {
                    args.push("--model".to_owned());
                    args.push(model);
                }
                ProviderKind::Ollama | ProviderKind::LlamaCpp => {}
            }
        }

        if self.provider == ProviderKind::Claude {
            if let Some(effort) = request
                .effort
                .clone()
                .filter(|value| !value.trim().is_empty())
            {
                args.push("--effort".to_owned());
                args.push(effort);
            }
        }

        args.push(request.prompt.clone());

        CommandSpec {
            program: self.program.clone(),
            args,
            cwd: Some(request.cwd.clone()),
        }
    }

    pub fn parse_auth_status(
        &self,
        version: Option<String>,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
    ) -> ProviderDiagnostic {
        let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
        let mut issues = Vec::new();

        let auth_status = match self.provider {
            ProviderKind::Codex => {
                if exit_code == 0 {
                    AuthStatus::Authenticated
                } else if combined.contains("login") || combined.contains("logged out") {
                    AuthStatus::LoggedOut
                } else {
                    AuthStatus::Error
                }
            }
            ProviderKind::Claude => {
                if combined.contains("git-bash") {
                    issues.push("Claude Code en Windows necesita Git Bash disponible.".to_string());
                    AuthStatus::MissingDependency
                } else if exit_code == 0 {
                    if combined.contains("authenticated")
                        || combined.contains("logged_in")
                        || combined.contains("active")
                    {
                        AuthStatus::Authenticated
                    } else {
                        AuthStatus::LoggedOut
                    }
                } else if combined.contains("login") {
                    AuthStatus::LoggedOut
                } else {
                    AuthStatus::Error
                }
            }
            ProviderKind::Ollama | ProviderKind::LlamaCpp => {
                if which(&self.program).is_ok() {
                    AuthStatus::Authenticated
                } else {
                    AuthStatus::MissingDependency
                }
            }
        };

        build_provider_status(
            self.provider,
            self.program.clone(),
            version,
            auth_status,
            if self.provider.is_local() {
                Some("Local runtime, no remote auth required.".to_string())
            } else {
                Some(stderr.trim().to_string())
                    .filter(|detail| !detail.is_empty())
                    .or_else(|| Some(stdout.trim().to_string()).filter(|detail| !detail.is_empty()))
            },
            issues,
        )
    }
}

pub fn detect_windows_git_bash() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let env_hint = std::env::var("CLAUDE_CODE_GIT_BASH_PATH")
            .ok()
            .map(PathBuf::from)
            .filter(|path| path.exists());
        if env_hint.is_some() {
            return env_hint;
        }

        let candidates = [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
        ];
        candidates
            .iter()
            .map(PathBuf::from)
            .find(|candidate| candidate.exists())
    }

    #[cfg(not(windows))]
    {
        None
    }
}

pub fn build_provider_status(
    provider: ProviderKind,
    program: impl Into<String>,
    version: Option<String>,
    auth_status: AuthStatus,
    detail: Option<String>,
    issues: Vec<String>,
) -> ProviderDiagnostic {
    let program = program.into();
    let status = if which(&program).is_ok() {
        ProviderInstallStatus::Installed
    } else {
        ProviderInstallStatus::Missing
    };

    ProviderDiagnostic {
        provider,
        status,
        program,
        version,
        auth_status,
        detail,
        issues,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::models::{AuthStatus, ProviderKind};

    use super::{AuthAction, ProviderAdapter, RunMode, RunRequest};

    #[test]
    fn builds_codex_resume_command() {
        let adapter = ProviderAdapter::new(ProviderKind::Codex, "codex.cmd");
        let spec = adapter.run_command(&RunRequest {
            prompt: "hola".into(),
            cwd: PathBuf::from("."),
            mode: RunMode::Resume,
            provider_session_ref: Some("session-123".into()),
            model: None,
            effort: None,
        });
        assert_eq!(
            spec.args,
            vec![
                "exec",
                "resume",
                "session-123",
                "--json",
                "--skip-git-repo-check",
                "hola"
            ]
        );
    }

    #[test]
    fn builds_claude_auth_status_command() {
        let adapter = ProviderAdapter::new(ProviderKind::Claude, "claude");
        let spec = adapter.auth_command(AuthAction::Status);
        assert_eq!(spec.args, vec!["auth", "status", "--json"]);
    }

    #[test]
    fn builds_claude_run_with_model_and_effort() {
        let adapter = ProviderAdapter::new(ProviderKind::Claude, "claude");
        let spec = adapter.run_command(&RunRequest {
            prompt: "hola".into(),
            cwd: PathBuf::from("."),
            mode: RunMode::New,
            provider_session_ref: None,
            model: Some("sonnet".into()),
            effort: Some("high".into()),
        });
        assert_eq!(
            spec.args,
            vec![
                "-p",
                "--output-format",
                "stream-json",
                "--model",
                "sonnet",
                "--effort",
                "high",
                "hola"
            ]
        );
    }

    #[test]
    fn codex_status_zero_means_authenticated() {
        let adapter = ProviderAdapter::new(ProviderKind::Codex, "codex.cmd");
        let diagnostic = adapter.parse_auth_status(None, "ok", "", 0);
        assert_eq!(diagnostic.auth_status, AuthStatus::Authenticated);
    }

    #[test]
    fn builds_ollama_run_with_default_model() {
        let adapter = ProviderAdapter::new(ProviderKind::Ollama, "ollama");
        let spec = adapter.run_command(&RunRequest {
            prompt: "hola".into(),
            cwd: PathBuf::from("."),
            mode: RunMode::New,
            provider_session_ref: None,
            model: None,
            effort: None,
        });
        assert_eq!(spec.args, vec!["run", "llama3.1:8b", "hola"]);
    }
}
