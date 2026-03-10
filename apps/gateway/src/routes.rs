use std::{convert::Infallible, time::Duration};

use async_stream::stream;
use axum::{
    Json, Router,
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
};
use futures::StreamExt;
use orchestrator_core::{
    AuthAction, EventKind, ProviderInstallStatus, ProviderKind, RunMode, RunRequest,
};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

use crate::{
    db::{ChatMessage, ChatSummary, DailyUsage},
    error::AppError,
    process::{run_provider_diagnostics, spawn_auth, spawn_run},
    state::AppState,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/providers", get(list_providers))
        .route(
            "/providers/{provider}/preferences",
            get(get_provider_preferences).put(update_provider_preferences),
        )
        .route("/providers/{provider}/login", post(login_provider))
        .route("/providers/{provider}/logout", post(logout_provider))
        .route(
            "/providers/{provider}/auth-stream/{auth_id}",
            get(stream_auth),
        )
        .route("/chats", get(list_chats).post(create_chat))
        .route("/chats/{chat_id}", get(get_chat))
        .route(
            "/chats/{chat_id}/messages",
            get(list_messages).post(send_message),
        )
        .route("/runs/{run_id}/stream", get(stream_run))
        .route("/usage/summary", get(usage_summary))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<ProviderView>>, AppError> {
    let prefs = state.provider_prefs.read().await;
    let mut providers = Vec::new();
    for adapter in state.providers.values() {
        let diagnostic = run_provider_diagnostics(adapter).await;
        let pref = prefs.get(&diagnostic.provider).cloned().unwrap_or_default();
        providers.push(ProviderView::from_parts(diagnostic, pref));
    }
    providers.sort_by_key(|provider| provider.provider.as_str().to_string());
    Ok(Json(providers))
}

async fn get_provider_preferences(
    Path(provider): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<ProviderPreferencesResponse>, AppError> {
    let provider = parse_provider(&provider)?;
    let prefs = state.provider_prefs.read().await;
    let pref = prefs.get(&provider).cloned().unwrap_or_default();
    Ok(Json(ProviderPreferencesResponse {
        provider,
        model: pref.model,
        effort: pref.effort,
    }))
}

async fn update_provider_preferences(
    Path(provider): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<UpdateProviderPreferencesRequest>,
) -> Result<Json<ProviderPreferencesResponse>, AppError> {
    let provider = parse_provider(&provider)?;
    let normalized = crate::state::ProviderPreferences {
        model: request.model.and_then(normalize_option),
        effort: request.effort.and_then(normalize_option),
    };
    {
        let mut prefs = state.provider_prefs.write().await;
        prefs.insert(provider, normalized.clone());
    }
    Ok(Json(ProviderPreferencesResponse {
        provider,
        model: normalized.model,
        effort: normalized.effort,
    }))
}

async fn login_provider(
    Path(provider): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<AuthLaunchResponse>, AppError> {
    start_provider_action(state, parse_provider(&provider)?, AuthAction::Login).await
}

async fn logout_provider(
    Path(provider): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<AuthLaunchResponse>, AppError> {
    start_provider_action(state, parse_provider(&provider)?, AuthAction::Logout).await
}

async fn create_chat(
    State(state): State<AppState>,
    Json(request): Json<CreateChatRequest>,
) -> Result<Json<ChatSummary>, AppError> {
    let title = request
        .title
        .unwrap_or_else(|| format!("{} chat", request.provider.as_str()));
    Ok(Json(state.db.create_chat(request.provider, title).await?))
}

async fn list_chats(State(state): State<AppState>) -> Result<Json<Vec<ChatSummary>>, AppError> {
    Ok(Json(state.db.list_chats().await?))
}

async fn get_chat(
    Path(chat_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<ChatSummary>, AppError> {
    let chat = state
        .db
        .get_chat(chat_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("chat {chat_id} not found")))?;
    Ok(Json(chat))
}

async fn list_messages(
    Path(chat_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Vec<ChatMessage>>, AppError> {
    Ok(Json(state.db.list_messages(chat_id).await?))
}

async fn send_message(
    Path(chat_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<SendMessageRequest>,
) -> Result<Json<RunLaunchResponse>, AppError> {
    if request.content.trim().is_empty() {
        return Err(AppError::BadRequest(
            "message content cannot be empty".into(),
        ));
    }

    let chat = state
        .db
        .get_chat(chat_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("chat {chat_id} not found")))?;
    state
        .db
        .insert_user_message(chat_id, request.content.trim())
        .await?;

    let adapter = state.provider(chat.provider).ok_or_else(|| {
        AppError::BadRequest(format!("unknown provider {}", chat.provider.as_str()))
    })?;
    let prefs = state.provider_prefs.read().await;
    let pref = prefs.get(&chat.provider).cloned().unwrap_or_default();
    let command = adapter
        .run_command(&RunRequest {
            prompt: request.content.clone(),
            cwd: state.config.workspace_dir.clone(),
            mode: if chat.provider_session_ref.is_some() {
                RunMode::Resume
            } else {
                RunMode::New
            },
            provider_session_ref: chat.provider_session_ref.clone(),
            model: pref.model,
            effort: pref.effort,
        })
        .display();
    let run = state
        .db
        .create_run(chat_id, chat.provider, &command)
        .await?;
    state.hub.ensure_run_sender(run.id).await;
    spawn_run(state.clone(), chat, run.clone(), request.content);
    Ok(Json(RunLaunchResponse { run_id: run.id }))
}

async fn stream_run(
    Path(run_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, AppError> {
    let receiver = state.hub.subscribe_run(run_id).await;
    let stored = state.db.list_run_events(run_id).await?;
    let terminal = state.db.run_is_terminal(run_id).await?;
    Ok(stream_events(stored, receiver, terminal))
}

async fn stream_auth(
    Path((provider, auth_id)): Path<(String, Uuid)>,
    State(state): State<AppState>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, AppError> {
    let _provider = parse_provider(&provider)?;
    let receiver = state.hub.subscribe_auth(auth_id).await;
    let stored = state.db.list_auth_events(auth_id).await?;
    let terminal = state.db.auth_is_terminal(auth_id).await?;
    Ok(stream_events(stored, receiver, terminal))
}

async fn usage_summary(
    State(state): State<AppState>,
) -> Result<Json<UsageSummaryResponse>, AppError> {
    let daily = state.db.usage_daily().await?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let limits = [ProviderKind::Codex, ProviderKind::Claude]
        .into_iter()
        .map(|provider| ProviderQuota {
            provider,
            soft_limit_tokens: state.config.soft_limit_for(provider),
            used_today_tokens: daily
                .iter()
                .filter(|row| row.provider == provider && row.day == today)
                .map(|row| row.total_tokens)
                .sum(),
        })
        .collect();
    Ok(Json(UsageSummaryResponse { daily, limits }))
}

async fn start_provider_action(
    state: AppState,
    provider: ProviderKind,
    action: AuthAction,
) -> Result<Json<AuthLaunchResponse>, AppError> {
    let adapter = state.provider(provider).ok_or_else(|| {
        AppError::BadRequest(format!("provider {} unavailable", provider.as_str()))
    })?;
    let auth = state
        .db
        .create_auth_session(
            provider,
            action_label(&action),
            &adapter.auth_command(action.clone()).display(),
        )
        .await?;
    state.hub.ensure_auth_sender(auth.id).await;
    spawn_auth(state, adapter, auth.clone(), action);
    Ok(Json(AuthLaunchResponse { auth_id: auth.id }))
}

fn stream_events(
    stored: Vec<orchestrator_core::NormalizedEvent>,
    receiver: tokio::sync::broadcast::Receiver<orchestrator_core::NormalizedEvent>,
    terminal: bool,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let event_stream = stream! {
        let mut last_sequence = -1_i64;
        for event in stored {
            last_sequence = last_sequence.max(event.sequence);
            yield Ok(json_event(&event));
        }

        if terminal {
            return;
        }

        let mut receiver = BroadcastStream::new(receiver);
        while let Some(item) = receiver.next().await {
            match item {
                Ok(event) => {
                    if event.sequence <= last_sequence {
                        continue;
                    }
                    last_sequence = event.sequence;
                    let is_terminal = matches!(event.event_kind, EventKind::RunCompleted | EventKind::RunFailed);
                    yield Ok(json_event(&event));
                    if is_terminal {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    };

    Sse::new(event_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    )
}

fn action_label(action: &AuthAction) -> &'static str {
    match action {
        AuthAction::Status => "status",
        AuthAction::Login => "login",
        AuthAction::Logout => "logout",
    }
}

fn parse_provider(value: &str) -> Result<ProviderKind, AppError> {
    match value {
        "codex" => Ok(ProviderKind::Codex),
        "claude" => Ok(ProviderKind::Claude),
        other => Err(AppError::BadRequest(format!("unknown provider {other}"))),
    }
}

fn json_event(event: &orchestrator_core::NormalizedEvent) -> Event {
    Event::default()
        .json_data(event)
        .expect("normalized event should serialize")
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct CreateChatRequest {
    pub provider: ProviderKind,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct RunLaunchResponse {
    pub run_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct AuthLaunchResponse {
    pub auth_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct UsageSummaryResponse {
    pub daily: Vec<DailyUsage>,
    pub limits: Vec<ProviderQuota>,
}

#[derive(Debug, Serialize)]
pub struct ProviderQuota {
    pub provider: ProviderKind,
    pub soft_limit_tokens: Option<i64>,
    pub used_today_tokens: i64,
}

#[derive(Debug, Serialize)]
pub struct ProviderView {
    pub provider: ProviderKind,
    pub display_name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub auth_status: String,
    pub detail: Option<String>,
    pub issues: Vec<String>,
    pub selected_model: Option<String>,
    pub selected_effort: Option<String>,
}

impl ProviderView {
    fn from_parts(
        value: orchestrator_core::ProviderDiagnostic,
        prefs: crate::state::ProviderPreferences,
    ) -> Self {
        Self {
            provider: value.provider,
            display_name: value.provider.as_str().to_string(),
            installed: matches!(value.status, ProviderInstallStatus::Installed),
            version: value.version,
            auth_status: format!("{:?}", value.auth_status).to_ascii_lowercase(),
            detail: value.detail,
            issues: value.issues,
            selected_model: prefs.model,
            selected_effort: prefs.effort,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateProviderPreferencesRequest {
    pub model: Option<String>,
    pub effort: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProviderPreferencesResponse {
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub effort: Option<String>,
}

fn normalize_option(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed.to_string())
}
