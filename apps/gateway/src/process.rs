use std::{collections::HashMap, process::Stdio};

use anyhow::Context;
use orchestrator_core::{
    AuthAction, AuthStatus, EventKind, NormalizedEvent, ProviderAdapter, ProviderKind,
    RunAccumulator, RunMode, RunRequest, RunStatus, build_provider_status, normalize_auth_line,
    normalize_stderr_line, normalize_stream_line,
};
use serde_json::json;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
};
use tracing::{error, warn};

use crate::{
    db::{AuthSession, ChatSummary, RunSummary},
    state::AppState,
};

#[derive(Debug)]
pub struct CompletedCommand {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub async fn run_provider_diagnostics(
    adapter: &ProviderAdapter,
) -> orchestrator_core::ProviderDiagnostic {
    let version_output = capture_command(adapter.provider, adapter.version_command()).await;
    let version = version_output
        .as_ref()
        .ok()
        .and_then(|output| (output.exit_code == 0).then(|| first_line(&output.stdout)));

    match capture_command(adapter.provider, adapter.auth_command(AuthAction::Status)).await {
        Ok(output) => {
            adapter.parse_auth_status(version, &output.stdout, &output.stderr, output.exit_code)
        }
        Err(error) => build_provider_status(
            adapter.provider,
            adapter.program.clone(),
            version,
            if error.to_string().to_ascii_lowercase().contains("git-bash") {
                AuthStatus::MissingDependency
            } else {
                AuthStatus::Error
            },
            Some(error.to_string()),
            Vec::new(),
        ),
    }
}

pub fn spawn_run(state: AppState, chat: ChatSummary, run: RunSummary, prompt: String) {
    tokio::spawn(async move {
        if let Err(error) = run_inner(state, chat, run, prompt).await {
            error!("run task failed: {error}");
        }
    });
}

pub fn spawn_auth(
    state: AppState,
    adapter: ProviderAdapter,
    auth: AuthSession,
    action: AuthAction,
) {
    tokio::spawn(async move {
        if let Err(error) = auth_inner(state, adapter, auth, action).await {
            error!("auth task failed: {error}");
        }
    });
}

pub fn build_provider_map() -> HashMap<ProviderKind, ProviderAdapter> {
    let mut providers = HashMap::new();
    let codex_program = if cfg!(windows) { "codex.cmd" } else { "codex" };
    providers.insert(
        ProviderKind::Codex,
        ProviderAdapter::new(ProviderKind::Codex, codex_program),
    );
    providers.insert(
        ProviderKind::Claude,
        ProviderAdapter::new(ProviderKind::Claude, "claude"),
    );
    providers
}

async fn run_inner(
    state: AppState,
    chat: ChatSummary,
    run: RunSummary,
    prompt: String,
) -> anyhow::Result<()> {
    let adapter = state
        .provider(chat.provider)
        .context("missing provider adapter")?;

    let pref = {
        let prefs = state.provider_prefs.read().await;
        prefs.get(&chat.provider).cloned().unwrap_or_default()
    };

    let spec = adapter.run_command(&RunRequest {
        prompt,
        cwd: state.config.workspace_dir.clone(),
        mode: if chat.provider_session_ref.is_some() {
            RunMode::Resume
        } else {
            RunMode::New
        },
        provider_session_ref: chat.provider_session_ref.clone(),
        model: pref.model,
        effort: pref.effort,
    });

    state.db.mark_run_running(run.id).await?;
    let started = NormalizedEvent {
        event_kind: EventKind::RunStarted,
        provider: chat.provider,
        sequence: 0,
        raw: json!({ "command": spec.display() }),
        text: Some(spec.display()),
        usage: None,
        provider_session_ref: chat.provider_session_ref.clone(),
        created_at: chrono::Utc::now(),
    };
    state.db.append_run_event(run.id, &started).await?;
    state.hub.publish_run(run.id, started).await;

    let mut child = spawn_command(chat.provider, &spec)?;
    let stdout = child.stdout.take().context("missing stdout")?;
    let stderr = child.stderr.take().context("missing stderr")?;
    let (tx, mut rx) = mpsc::unbounded_channel::<(bool, String)>();
    tokio::spawn(read_lines(stdout, true, tx.clone()));
    tokio::spawn(read_lines(stderr, false, tx));

    let mut sequence = 1_i64;
    let mut acc = RunAccumulator::default();
    let mut stdout_raw = String::new();

    while let Some((is_stdout, line)) = rx.recv().await {
        if is_stdout {
            stdout_raw.push_str(&line);
            stdout_raw.push('\n');
            for event in normalize_stream_line(chat.provider, sequence, &line) {
                apply_event(&mut acc, &event);
                state.db.append_run_event(run.id, &event).await?;
                state.hub.publish_run(run.id, event).await;
            }
        } else {
            acc.stderr_text.push_str(&line);
            acc.stderr_text.push('\n');
            let event = normalize_stderr_line(chat.provider, sequence, &line);
            state.db.append_run_event(run.id, &event).await?;
            state.hub.publish_run(run.id, event).await;
        }
        sequence += 1;
    }

    let status = child.wait().await?;
    let exit_code = status.code().unwrap_or(-1);

    if let Some(provider_session_ref) = acc.provider_session_ref.as_deref() {
        state
            .db
            .update_chat_provider_session(chat.id, provider_session_ref)
            .await?;
    }

    if let Some(usage) = acc.usage.as_ref() {
        state.db.upsert_usage(run.id, chat.provider, usage).await?;
    }

    let assistant_text = acc.assistant_text.trim().to_string();
    if !assistant_text.is_empty() {
        state
            .db
            .insert_assistant_message(chat.id, run.id, &assistant_text)
            .await?;
    }

    let finished = NormalizedEvent {
        event_kind: if status.success() {
            EventKind::RunCompleted
        } else {
            EventKind::RunFailed
        },
        provider: chat.provider,
        sequence,
        raw: json!({ "exit_code": exit_code }),
        text: Some(if status.success() {
            format!("completed with exit code {exit_code}")
        } else {
            format!("failed with exit code {exit_code}")
        }),
        usage: acc.usage.clone(),
        provider_session_ref: acc.provider_session_ref.clone(),
        created_at: chrono::Utc::now(),
    };
    state.db.append_run_event(run.id, &finished).await?;
    state
        .db
        .complete_run(
            run.id,
            if status.success() {
                RunStatus::Completed
            } else {
                RunStatus::Failed
            },
            exit_code,
            &stdout_raw,
            &acc.stderr_text,
            acc.provider_session_ref.as_deref(),
        )
        .await?;
    state.hub.publish_run(run.id, finished).await;
    Ok(())
}

async fn auth_inner(
    state: AppState,
    adapter: ProviderAdapter,
    auth: AuthSession,
    action: AuthAction,
) -> anyhow::Result<()> {
    let spec = adapter.auth_command(action);
    state.db.mark_auth_running(auth.id).await?;

    let mut child = spawn_command(adapter.provider, &spec)?;
    let stdout = child.stdout.take().context("missing stdout")?;
    let stderr = child.stderr.take().context("missing stderr")?;
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(read_auth_lines(stdout, tx.clone()));
    tokio::spawn(read_auth_lines(stderr, tx));

    let mut sequence = 1_i64;
    let mut output = String::new();

    while let Some(line) = rx.recv().await {
        output.push_str(&line);
        output.push('\n');
        for event in normalize_auth_line(adapter.provider, sequence, &line) {
            state.db.append_auth_event(auth.id, &event).await?;
            state.hub.publish_auth(auth.id, event).await;
        }
        sequence += 1;
    }

    let status = child.wait().await?;
    let exit_code = status.code().unwrap_or(-1);
    let final_event = NormalizedEvent {
        event_kind: if status.success() {
            EventKind::RunCompleted
        } else {
            EventKind::RunFailed
        },
        provider: adapter.provider,
        sequence,
        raw: json!({ "exit_code": exit_code, "action": auth.action }),
        text: Some(if status.success() {
            format!("{} completed", auth.action)
        } else {
            format!("{} failed", auth.action)
        }),
        usage: None,
        provider_session_ref: None,
        created_at: chrono::Utc::now(),
    };
    state.db.append_auth_event(auth.id, &final_event).await?;
    state
        .db
        .complete_auth_session(
            auth.id,
            if status.success() {
                "completed"
            } else {
                "failed"
            },
            exit_code,
            &output,
        )
        .await?;
    state.hub.publish_auth(auth.id, final_event).await;
    Ok(())
}

async fn capture_command(
    provider: ProviderKind,
    spec: orchestrator_core::CommandSpec,
) -> anyhow::Result<CompletedCommand> {
    let output = spawn_command(provider, &spec)?.wait_with_output().await?;
    Ok(CompletedCommand {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

fn spawn_command(
    provider: ProviderKind,
    spec: &orchestrator_core::CommandSpec,
) -> anyhow::Result<Child> {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }

    if provider == ProviderKind::Claude {
        if let Some(path) = orchestrator_core::detect_windows_git_bash() {
            command.env("CLAUDE_CODE_GIT_BASH_PATH", path);
        } else if cfg!(windows) {
            warn!("Git Bash not detected for Claude Code");
        }
    }

    Ok(command.spawn()?)
}

async fn read_lines(
    reader: impl tokio::io::AsyncRead + Unpin,
    is_stdout: bool,
    tx: mpsc::UnboundedSender<(bool, String)>,
) {
    let mut lines = BufReader::new(reader).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let _ = tx.send((is_stdout, line));
            }
            Ok(None) => break,
            Err(error) => {
                let _ = tx.send((false, format!("stream read error: {error}")));
                break;
            }
        }
    }
}

async fn read_auth_lines(
    reader: impl tokio::io::AsyncRead + Unpin,
    tx: mpsc::UnboundedSender<String>,
) {
    let mut lines = BufReader::new(reader).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let _ = tx.send(line);
            }
            Ok(None) => break,
            Err(error) => {
                let _ = tx.send(format!("stream read error: {error}"));
                break;
            }
        }
    }
}

fn apply_event(acc: &mut RunAccumulator, event: &NormalizedEvent) {
    match event.event_kind {
        EventKind::AssistantDelta => {
            if let Some(text) = &event.text {
                acc.assistant_text.push_str(text);
            }
        }
        EventKind::AssistantFinal => {
            if let Some(text) = &event.text {
                if acc.assistant_text.is_empty() || text.len() >= acc.assistant_text.len() {
                    acc.assistant_text = text.clone();
                } else {
                    acc.assistant_text.push_str(text);
                }
            }
        }
        EventKind::UsageUpdated => {
            acc.usage = event.usage.clone();
        }
        EventKind::ProviderSessionBound => {
            acc.provider_session_ref = event.provider_session_ref.clone();
        }
        _ => {}
    }
}

fn first_line(body: &str) -> String {
    body.lines().next().unwrap_or_default().trim().to_string()
}
