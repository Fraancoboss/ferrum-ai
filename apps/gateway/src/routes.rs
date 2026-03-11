use std::{convert::Infallible, time::Duration};

use async_stream::stream;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
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
    agent_mode::{initialize_workflow, spawn_workflow},
    db::{ChatMessage, ChatSummary, DailyUsage},
    error::AppError,
    local_models::{
        BrowserHardwareSnapshotInput, CatalogQuery, GgufImportResponse, ImportGgufRequest,
        InstallCatalogModelRequest, LocalModelCatalogItem, LocalModelInventoryView,
        ProvidersGovernanceView, ProvidersHardwareView, catalog_view, current_hardware_view,
        import_gguf_model, persist_browser_snapshot, providers_governance_view,
        start_ollama_catalog_install,
    },
    process::{run_provider_diagnostics, spawn_auth, spawn_run},
    state::AppState,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/providers", get(list_providers))
        .route("/providers/hardware", get(get_providers_hardware))
        .route("/providers/governance", get(get_providers_governance))
        .route(
            "/providers/hardware/browser-snapshot",
            post(save_browser_hardware_snapshot),
        )
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
        .route("/workflow-templates", get(list_workflow_templates))
        .route("/workflows", get(list_workflows).post(create_workflow))
        .route("/workflows/{workflow_id}", get(get_workflow))
        .route(
            "/workflows/{workflow_id}/handoffs",
            get(list_workflow_handoffs).post(create_workflow_handoff),
        )
        .route(
            "/workflows/{workflow_id}/evidence",
            get(list_workflow_evidence),
        )
        .route(
            "/workflows/{workflow_id}/qa-status",
            get(get_workflow_qa_status),
        )
        .route(
            "/workflows/{workflow_id}/snapshots",
            get(list_workflow_snapshots).post(create_workflow_snapshot),
        )
        .route("/workflows/{workflow_id}/rollback", post(rollback_workflow))
        .route("/workflows/{workflow_id}/start", post(start_workflow))
        .route("/approvals/{approval_id}", post(update_approval))
        .route("/agents/{agent_id}/provider", post(update_agent_provider))
        .route("/agents/{agent_id}/retry", post(retry_agent))
        .route("/agents/{agent_id}/escalate", post(escalate_agent))
        .route(
            "/mcp/servers",
            get(list_mcp_servers).post(upsert_mcp_server),
        )
        .route("/mcp/servers/{server_id}", post(set_mcp_server_enabled))
        .route(
            "/llama-cpp/models",
            get(list_llama_cpp_models).post(upsert_llama_cpp_model),
        )
        .route(
            "/llama-cpp/models/{model_id}",
            post(set_llama_cpp_model_enabled),
        )
        .route("/local-models/catalog", get(list_local_model_catalog))
        .route("/local-models/installed", get(list_local_models_installed))
        .route("/local-models/install-jobs", get(list_local_model_install_jobs))
        .route(
            "/local-models/ollama/install",
            post(install_ollama_catalog_model),
        )
        .route("/local-models/gguf/import", post(import_local_gguf))
        .route("/skills", get(list_skills).post(create_skill))
        .route("/skills/{skill_id}", get(get_skill))
        .route("/skills/{skill_id}/versions", get(list_skill_versions).post(create_skill_version))
        .route(
            "/skills/{skill_id}/assignments",
            get(list_skill_assignments).post(create_skill_assignment),
        )
        .route("/skill-assignments/targets", get(skill_assignment_targets))
        .route("/skill-assignments/{assignment_id}", post(delete_skill_assignment))
        .route(
            "/skill-versions/{version_id}/submit-review",
            post(submit_skill_version_for_review),
        )
        .route("/skill-versions/{version_id}/approve", post(approve_skill_version))
        .route("/skill-versions/{version_id}/publish", post(publish_skill_version))
        .route("/terminals/{terminal_id}/stream", get(stream_terminal))
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
    let llama_models = state.db.list_llama_cpp_models().await?;
    let mut providers = Vec::new();
    for adapter in state.providers.values() {
        let diagnostic = run_provider_diagnostics(adapter).await;
        let pref = prefs.get(&diagnostic.provider).cloned().unwrap_or_default();
        let mut view = ProviderView::from_parts(diagnostic, pref);
        if view.provider == ProviderKind::LlamaCpp
            && !llama_models.iter().any(|model| model.enabled)
        {
            view.issues
                .push("Register at least one enabled GGUF model for llama.cpp.".to_string());
            if view.detail.is_none() {
                view.detail = Some("No enabled GGUF model configured.".to_string());
            }
        }
        providers.push(view);
    }
    providers.sort_by_key(|provider| provider.provider.as_str().to_string());
    Ok(Json(providers))
}

async fn get_providers_hardware(
    State(state): State<AppState>,
) -> Result<Json<ProvidersHardwareView>, AppError> {
    Ok(Json(current_hardware_view(&state.db).await?))
}

async fn get_providers_governance(
    State(state): State<AppState>,
) -> Result<Json<ProvidersGovernanceView>, AppError> {
    Ok(Json(providers_governance_view(&state).await?))
}

async fn save_browser_hardware_snapshot(
    State(state): State<AppState>,
    Json(request): Json<BrowserHardwareSnapshotInput>,
) -> Result<Json<crate::local_models::DeviceHardwareProfile>, AppError> {
    Ok(Json(persist_browser_snapshot(&state.db, request).await?))
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
    let limits = [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Ollama,
        ProviderKind::LlamaCpp,
    ]
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

async fn list_workflows(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::WorkflowSummary>>, AppError> {
    Ok(Json(state.db.list_workflows().await?))
}

async fn list_workflow_templates(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::WorkflowTemplate>>, AppError> {
    Ok(Json(state.db.list_workflow_templates().await?))
}

async fn create_workflow(
    State(state): State<AppState>,
    Json(request): Json<CreateWorkflowRequest>,
) -> Result<Json<crate::db::WorkflowDetail>, AppError> {
    if request.objective.trim().is_empty() {
        return Err(AppError::BadRequest(
            "workflow objective cannot be empty".into(),
        ));
    }

    let workflow = state
        .db
        .create_workflow(
            request
                .title
                .clone()
                .unwrap_or_else(|| "Agent workflow".to_string()),
            request.objective.trim().to_string(),
            request.coordinator_provider.unwrap_or(ProviderKind::Ollama),
            request.sensitivity.as_deref().unwrap_or("internal"),
            request
                .template_key
                .as_deref()
                .unwrap_or("engineering_pipeline"),
        )
        .await?;

    let detail = initialize_workflow(&state, &workflow).await?;
    if request.auto_start.unwrap_or(true) {
        spawn_workflow(state.clone(), workflow.id);
    }
    Ok(Json(detail))
}

async fn get_workflow(
    Path(workflow_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<crate::db::WorkflowDetail>, AppError> {
    let workflow = state
        .db
        .get_workflow_detail(workflow_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {workflow_id} not found")))?;
    Ok(Json(workflow))
}

async fn list_workflow_handoffs(
    Path(workflow_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::WorkflowHandoff>>, AppError> {
    state
        .db
        .get_workflow(workflow_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {workflow_id} not found")))?;
    Ok(Json(state.db.list_workflow_handoffs(workflow_id).await?))
}

async fn create_workflow_handoff(
    Path(workflow_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<CreateWorkflowHandoffRequest>,
) -> Result<Json<crate::db::WorkflowHandoff>, AppError> {
    state
        .db
        .get_workflow(workflow_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {workflow_id} not found")))?;
    let handoff = state
        .db
        .create_workflow_handoff(
            workflow_id,
            request.from_agent_id,
            request.to_agent_id,
            &request.phase,
            &request.handoff_type,
            &request.task_ref,
            request.priority.as_deref().unwrap_or("normal"),
            &request.context_summary,
            &request.relevant_artifact_ids.unwrap_or_default(),
            &request.dependencies.unwrap_or_default(),
            &request.constraints.unwrap_or_default(),
            &request.deliverable_request,
            &request.acceptance_criteria.unwrap_or_default(),
            &request.evidence_required.unwrap_or_default(),
            request.status.as_deref().unwrap_or("open"),
        )
        .await?;
    state
        .db
        .append_workflow_evidence(
            workflow_id,
            "human",
            None,
            "handoff_created",
            serde_json::json!({
                "handoff_id": handoff.id,
                "type": handoff.handoff_type,
                "phase": handoff.phase,
            }),
        )
        .await?;
    Ok(Json(handoff))
}

async fn list_workflow_evidence(
    Path(workflow_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::WorkflowEvidenceRecord>>, AppError> {
    state
        .db
        .get_workflow(workflow_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {workflow_id} not found")))?;
    Ok(Json(
        state.db.list_workflow_evidence_records(workflow_id).await?,
    ))
}

async fn get_workflow_qa_status(
    Path(workflow_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<WorkflowQaStatusResponse>, AppError> {
    state
        .db
        .get_workflow(workflow_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {workflow_id} not found")))?;
    Ok(Json(WorkflowQaStatusResponse {
        qa_verdicts: state.db.list_workflow_qa_verdicts(workflow_id).await?,
        release_verdicts: state.db.list_workflow_release_verdicts(workflow_id).await?,
    }))
}

async fn list_workflow_snapshots(
    Path(workflow_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::WorkflowSnapshot>>, AppError> {
    state
        .db
        .get_workflow(workflow_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {workflow_id} not found")))?;
    Ok(Json(state.db.list_workflow_snapshots(workflow_id).await?))
}

async fn create_workflow_snapshot(
    Path(workflow_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<CreateWorkflowSnapshotRequest>,
) -> Result<Json<crate::db::WorkflowSnapshot>, AppError> {
    let detail = state
        .db
        .get_workflow_detail(workflow_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {workflow_id} not found")))?;
    let snapshot = state
        .db
        .create_workflow_snapshot(
            workflow_id,
            request.agent_id,
            request.snapshot_type.as_deref().unwrap_or("checkpoint"),
            request.label.as_deref().unwrap_or("Manual checkpoint"),
            serde_json::json!({
                "workflow": detail.workflow,
                "agents": detail.agents,
                "terminals": detail.terminals,
            }),
            request.rollback_target.unwrap_or(true),
        )
        .await?;
    state
        .db
        .append_workflow_evidence(
            workflow_id,
            "human",
            request.agent_id,
            "snapshot_created",
            serde_json::json!({
                "snapshot_id": snapshot.id,
                "label": snapshot.label,
            }),
        )
        .await?;
    Ok(Json(snapshot))
}

async fn rollback_workflow(
    Path(workflow_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<RollbackWorkflowRequest>,
) -> Result<Json<crate::db::WorkflowDetail>, AppError> {
    let detail = state
        .db
        .restore_workflow_snapshot(request.snapshot_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("snapshot {} not found", request.snapshot_id)))?;
    if detail.workflow.id != workflow_id {
        return Err(AppError::BadRequest(
            "snapshot does not belong to this workflow".into(),
        ));
    }
    state
        .db
        .append_workflow_evidence(
            workflow_id,
            "human",
            None,
            "workflow_rolled_back",
            serde_json::json!({
                "snapshot_id": request.snapshot_id,
            }),
        )
        .await?;
    Ok(Json(detail))
}

async fn start_workflow(
    Path(workflow_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<crate::db::WorkflowDetail>, AppError> {
    state
        .db
        .get_workflow_detail(workflow_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {workflow_id} not found")))?;
    spawn_workflow(state.clone(), workflow_id);
    let detail = state
        .db
        .get_workflow_detail(workflow_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {workflow_id} not found")))?;
    Ok(Json(detail))
}

async fn update_approval(
    Path(approval_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<ApprovalDecisionRequest>,
) -> Result<Json<crate::db::ApprovalGate>, AppError> {
    let gate = state
        .db
        .update_approval_status(
            approval_id,
            if request.approved {
                "approved"
            } else {
                "rejected"
            },
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("approval {approval_id} not found")))?;

    if let Some(agent_id) = gate.agent_id {
        state
            .db
            .set_agent_status(
                agent_id,
                if request.approved {
                    "pending"
                } else {
                    "blocked"
                },
                Some(if request.approved {
                    "Approval granted"
                } else {
                    "Approval rejected"
                }),
            )
            .await?;
    }

    if request.approved {
        state
            .db
            .append_workflow_evidence(
                gate.workflow_id,
                "human",
                gate.agent_id,
                "approval_granted",
                serde_json::json!({
                    "approval_id": gate.id,
                    "provider": gate.target_provider.map(|provider| provider.as_str()),
                }),
            )
            .await?;
        spawn_workflow(state.clone(), gate.workflow_id);
    } else {
        state
            .db
            .set_workflow_status(gate.workflow_id, "attention")
            .await?;
        state
            .db
            .append_workflow_evidence(
                gate.workflow_id,
                "human",
                gate.agent_id,
                "approval_rejected",
                serde_json::json!({
                    "approval_id": gate.id,
                    "provider": gate.target_provider.map(|provider| provider.as_str()),
                }),
            )
            .await?;
    }

    Ok(Json(gate))
}

async fn update_agent_provider(
    Path(agent_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<UpdateAgentProviderRequest>,
) -> Result<Json<crate::db::WorkflowDetail>, AppError> {
    let agent = state
        .db
        .get_agent(agent_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("agent {agent_id} not found")))?;
    if agent.status == "running" {
        return Err(AppError::BadRequest(
            "cannot change provider while the agent is running".into(),
        ));
    }

    let detail = state
        .db
        .get_workflow_detail(agent.workflow_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {} not found", agent.workflow_id)))?;
    let workflow = &detail.workflow;
    let needs_approval = workflow.sensitivity != "public" && !request.provider.is_local();
    let next_status = if needs_approval { "gated" } else { "pending" };

    state
        .db
        .update_agent_provider(
            agent_id,
            request.provider,
            needs_approval,
            next_status,
            Some("Provider reassigned"),
        )
        .await?;

    if let Some(terminal) = detail
        .terminals
        .iter()
        .find(|terminal| terminal.agent_id == agent_id)
    {
        state
            .db
            .update_terminal_provider(terminal.id, request.provider, next_status)
            .await?;
    }

    if needs_approval {
        let refreshed_for_approval = state
            .db
            .get_workflow_detail(workflow.id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("workflow {} not found", workflow.id)))?;
        let external_context_skills = refreshed_for_approval
            .resolved_skills
            .iter()
            .filter(|skill| skill.agent_id == agent_id && skill.applies_to_external_context)
            .map(|skill| {
                serde_json::json!({
                    "skill_id": skill.skill_id,
                    "skill_version_id": skill.skill_version_id,
                    "name": skill.skill_name,
                    "version": skill.skill_version,
                    "skill_type": skill.skill_type,
                    "source_target_type": skill.source_target_type,
                    "source_target_key": skill.source_target_key,
                    "provider_exposure": skill.provider_exposure,
                })
            })
            .collect::<Vec<_>>();
        state
            .db
            .create_approval_gate(
                workflow.id,
                Some(agent_id),
                "provider_egress",
                Some(request.provider),
                "Provider reassignment requires approval before external execution.",
                serde_json::json!({
                    "agent_id": agent_id,
                    "role": agent.role,
                    "provider": request.provider.as_str(),
                    "workflow_sensitivity": workflow.sensitivity,
                    "external_context_skills": external_context_skills,
                }),
            )
            .await?;
        state
            .db
            .set_workflow_status(workflow.id, "awaiting_approval")
            .await?;
    } else {
        state
            .db
            .append_workflow_evidence(
                workflow.id,
                "human",
                Some(agent_id),
                "agent_provider_reassigned",
                serde_json::json!({
                    "agent_id": agent_id,
                    "provider": request.provider.as_str(),
                }),
            )
            .await?;
        spawn_workflow(state.clone(), workflow.id);
    }

    let refreshed = state
        .db
        .get_workflow_detail(workflow.id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {} not found", workflow.id)))?;
    Ok(Json(refreshed))
}

async fn retry_agent(
    Path(agent_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<crate::db::WorkflowDetail>, AppError> {
    let agent = state
        .db
        .get_agent(agent_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("agent {agent_id} not found")))?;
    if agent.status == "running" {
        return Err(AppError::BadRequest("cannot retry a running agent".into()));
    }
    state
        .db
        .set_agent_status(agent_id, "pending", Some("Retry requested by operator"))
        .await?;
    if let Some(detail) = state.db.get_workflow_detail(agent.workflow_id).await? {
        if let Some(terminal) = detail
            .terminals
            .iter()
            .find(|terminal| terminal.agent_id == agent_id)
        {
            state
                .db
                .update_terminal_provider(terminal.id, agent.provider, "pending")
                .await?;
        }
    }
    state
        .db
        .append_workflow_evidence(
            agent.workflow_id,
            "human",
            Some(agent_id),
            "agent_retry_requested",
            serde_json::json!({
                "agent_id": agent_id,
                "role": agent.role,
            }),
        )
        .await?;
    spawn_workflow(state.clone(), agent.workflow_id);
    let detail = state
        .db
        .get_workflow_detail(agent.workflow_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {} not found", agent.workflow_id)))?;
    Ok(Json(detail))
}

async fn escalate_agent(
    Path(agent_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<EscalateAgentRequest>,
) -> Result<Json<crate::db::WorkflowDetail>, AppError> {
    let agent = state
        .db
        .get_agent(agent_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("agent {agent_id} not found")))?;
    state
        .db
        .create_workflow_handoff(
            agent.workflow_id,
            Some(agent_id),
            None,
            "qa_loop",
            "escalation",
            "manual escalation",
            "high",
            request.reason.as_deref().unwrap_or("Operator escalation"),
            &[],
            &[],
            &[],
            "Review and unblock the workflow.",
            &["Inspect evidence".to_string()],
            &["Inspect latest handoffs and verdicts".to_string()],
            "open",
        )
        .await?;
    state
        .db
        .update_workflow_runtime(
            agent.workflow_id,
            Some("attention"),
            None,
            Some("blocked"),
            Some(Some("Manual escalation raised")),
            Some(Some(
                request.reason.as_deref().unwrap_or("Operator escalation"),
            )),
        )
        .await?;
    state
        .db
        .append_workflow_evidence(
            agent.workflow_id,
            "human",
            Some(agent_id),
            "agent_escalated",
            serde_json::json!({
                "agent_id": agent_id,
                "reason": request.reason,
            }),
        )
        .await?;
    let detail = state
        .db
        .get_workflow_detail(agent.workflow_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("workflow {} not found", agent.workflow_id)))?;
    Ok(Json(detail))
}

async fn stream_terminal(
    Path(terminal_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, AppError> {
    let receiver = state.hub.subscribe_terminal(terminal_id).await;
    let stored = state.db.list_terminal_entries(terminal_id).await?;
    Ok(stream_terminal_entries(stored, receiver))
}

async fn list_mcp_servers(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::McpServer>>, AppError> {
    Ok(Json(state.db.list_mcp_servers().await?))
}

async fn upsert_mcp_server(
    State(state): State<AppState>,
    Json(request): Json<UpsertMcpServerRequest>,
) -> Result<Json<crate::db::McpServer>, AppError> {
    if request.name.trim().is_empty() || request.command.trim().is_empty() {
        return Err(AppError::BadRequest(
            "mcp name and command are required".into(),
        ));
    }

    let providers = request
        .allowed_providers
        .unwrap_or_else(|| vec![ProviderKind::Ollama, ProviderKind::LlamaCpp]);
    let server = state
        .db
        .upsert_mcp_server(
            request.name.trim(),
            request.command.trim(),
            &request.args.unwrap_or_default(),
            request.local_only.unwrap_or(true),
            request.enabled.unwrap_or(true),
            &providers,
        )
        .await?;
    Ok(Json(server))
}

async fn set_mcp_server_enabled(
    Path(server_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<EnabledRequest>,
) -> Result<Json<crate::db::McpServer>, AppError> {
    let server = state
        .db
        .set_mcp_server_enabled(server_id, request.enabled)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("mcp server {server_id} not found")))?;
    Ok(Json(server))
}

async fn list_llama_cpp_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::LlamaCppModel>>, AppError> {
    Ok(Json(state.db.list_llama_cpp_models().await?))
}

async fn upsert_llama_cpp_model(
    State(state): State<AppState>,
    Json(request): Json<UpsertLlamaCppModelRequest>,
) -> Result<Json<crate::db::LlamaCppModel>, AppError> {
    if request.alias.trim().is_empty() || request.file_path.trim().is_empty() {
        return Err(AppError::BadRequest(
            "llama.cpp alias and file_path are required".into(),
        ));
    }

    let file_path = resolve_model_path(&state, request.file_path.trim());
    let exists = tokio::fs::try_exists(&file_path)
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    if !exists {
        return Err(AppError::BadRequest(format!(
            "llama.cpp model file not found at {}",
            file_path.display()
        )));
    }

    let model = state
        .db
        .upsert_llama_cpp_model(
            request.alias.trim(),
            &file_path.to_string_lossy(),
            request.context_window,
            request.quantization.as_deref(),
            request.enabled.unwrap_or(true),
        )
        .await?;
    Ok(Json(model))
}

async fn set_llama_cpp_model_enabled(
    Path(model_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<EnabledRequest>,
) -> Result<Json<crate::db::LlamaCppModel>, AppError> {
    let model = state
        .db
        .set_llama_cpp_model_enabled(model_id, request.enabled)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("llama.cpp model {model_id} not found")))?;
    Ok(Json(model))
}

async fn list_local_model_catalog(
    Query(query): Query<CatalogQuery>,
    State(state): State<AppState>,
) -> Result<Json<Vec<LocalModelCatalogItem>>, AppError> {
    Ok(Json(catalog_view(&state.db, &state, &query).await?))
}

async fn list_local_models_installed(
    State(state): State<AppState>,
) -> Result<Json<LocalModelInventoryView>, AppError> {
    Ok(Json(crate::local_models::local_inventory(&state).await?))
}

async fn list_local_model_install_jobs(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::ModelInstallJob>>, AppError> {
    Ok(Json(state.db.list_model_install_jobs(25).await?))
}

async fn install_ollama_catalog_model(
    State(state): State<AppState>,
    Json(request): Json<InstallCatalogModelRequest>,
) -> Result<Json<crate::db::ModelInstallJob>, AppError> {
    if request.catalog_key.trim().is_empty() {
        return Err(AppError::BadRequest("catalog_key is required".into()));
    }
    Ok(Json(start_ollama_catalog_install(state, request).await.map_err(
        |error| AppError::BadRequest(error.to_string()),
    )?))
}

async fn import_local_gguf(
    State(state): State<AppState>,
    Json(request): Json<ImportGgufRequest>,
) -> Result<Json<GgufImportResponse>, AppError> {
    Ok(Json(
        import_gguf_model(&state, request)
            .await
            .map_err(|error| AppError::BadRequest(error.to_string()))?,
    ))
}

async fn list_skills(
    Query(query): Query<ListSkillsQuery>,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::SkillSummary>>, AppError> {
    let filters = crate::db::SkillListFilters {
        tenant_key: query.tenant_key.unwrap_or_else(|| "default".to_string()),
        skill_type: normalize_option_ref(query.skill_type),
        status: normalize_option_ref(query.status),
        tag: normalize_option_ref(query.tag),
        owner: normalize_option_ref(query.owner),
        sensitivity: normalize_option_ref(query.sensitivity),
    };
    Ok(Json(state.db.list_skills(&filters).await?))
}

async fn create_skill(
    State(state): State<AppState>,
    Json(request): Json<CreateSkillRequest>,
) -> Result<Json<crate::db::SkillDetail>, AppError> {
    if request.slug.trim().is_empty()
        || request.name.trim().is_empty()
        || request.description.trim().is_empty()
        || request.initial_version.summary.trim().is_empty()
    {
        return Err(AppError::BadRequest(
            "slug, name, description, and initial summary are required".into(),
        ));
    }

    let skill_type = validate_skill_type(request.skill_type.trim())?;
    let allowed_sensitivity_levels =
        validate_sensitivity_levels(request.allowed_sensitivity_levels.unwrap_or_else(|| {
            vec!["internal".to_string()]
        }))?;
    let owner = request
        .owner
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("local-operator")
        .to_string();

    let input = crate::db::CreateSkillInput {
        tenant_key: request
            .tenant_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("default")
            .to_string(),
        slug: request.slug.trim().to_string(),
        name: request.name.trim().to_string(),
        skill_type: skill_type.to_string(),
        description: request.description.trim().to_string(),
        owner: owner.clone(),
        visibility: request
            .visibility
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("private")
            .to_string(),
        tags: normalize_string_list(request.tags.unwrap_or_default()),
        allowed_sensitivity_levels,
        provider_exposure: request
            .provider_exposure
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(validate_provider_exposure)
            .transpose()?
            .unwrap_or_else(|| default_provider_exposure(skill_type))
            .to_string(),
        source_kind: request
            .source_kind
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("manual")
            .to_string(),
        initial_version: to_skill_version_input(request.initial_version, &owner)?,
    };

    let detail = state
        .db
        .create_skill(input)
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(Json(detail))
}

async fn get_skill(
    Path(skill_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<crate::db::SkillDetail>, AppError> {
    let detail = state
        .db
        .get_skill_detail(skill_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("skill {skill_id} not found")))?;
    Ok(Json(detail))
}

async fn list_skill_versions(
    Path(skill_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::SkillVersion>>, AppError> {
    Ok(Json(state.db.list_skill_versions(skill_id).await?))
}

async fn list_skill_assignments(
    Path(skill_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::db::SkillAssignment>>, AppError> {
    Ok(Json(state.db.list_skill_assignments(skill_id).await?))
}

async fn create_skill_version(
    Path(skill_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<CreateSkillVersionRequest>,
) -> Result<Json<crate::db::SkillDetail>, AppError> {
    if request.summary.trim().is_empty() {
        return Err(AppError::BadRequest("summary is required".into()));
    }
    let input = to_skill_version_input(request, "local-operator")?;
    let detail = state
        .db
        .create_skill_version(skill_id, input)
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(Json(detail))
}

async fn create_skill_assignment(
    Path(skill_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<CreateSkillAssignmentRequest>,
) -> Result<Json<crate::db::SkillDetail>, AppError> {
    let target_type = validate_assignment_target_type(request.target_type.trim())?;
    let target_key = request.target_key.trim();
    if target_key.is_empty() {
        return Err(AppError::BadRequest("assignment target_key cannot be empty".into()));
    }
    let targets = state.db.skill_assignment_targets().await?;
    let valid_target = match target_type {
        "workflow_template" => targets.workflow_templates.iter().any(|item| item == target_key),
        "agent_role" => targets.agent_roles.iter().any(|item| item == target_key),
        "provider" => targets.providers.iter().any(|item| item == target_key),
        _ => false,
    };
    if !valid_target {
        return Err(AppError::BadRequest(format!(
            "assignment target_key {target_key} is not valid for {target_type}"
        )));
    }

    let detail = state
        .db
        .create_skill_assignment(
            skill_id,
            crate::db::CreateSkillAssignmentInput {
                skill_version_id: request.skill_version_id,
                target_type: target_type.to_string(),
                target_key: target_key.to_string(),
            },
        )
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(Json(detail))
}

async fn delete_skill_assignment(
    Path(assignment_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<crate::db::SkillDetail>, AppError> {
    let detail = state
        .db
        .delete_skill_assignment(assignment_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("skill assignment {assignment_id} not found")))?;
    Ok(Json(detail))
}

async fn skill_assignment_targets(
    State(state): State<AppState>,
) -> Result<Json<crate::db::SkillAssignmentTargets>, AppError> {
    Ok(Json(state.db.skill_assignment_targets().await?))
}

async fn submit_skill_version_for_review(
    Path(version_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<SkillActionRequest>,
) -> Result<Json<crate::db::SkillDetail>, AppError> {
    let detail = state
        .db
        .submit_skill_version_for_review(
            version_id,
            request.actor.as_deref().unwrap_or("local-operator"),
            request.comment.as_deref(),
        )
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(Json(detail))
}

async fn approve_skill_version(
    Path(version_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<SkillActionRequest>,
) -> Result<Json<crate::db::SkillDetail>, AppError> {
    let detail = state
        .db
        .approve_skill_version(
            version_id,
            request.actor.as_deref().unwrap_or("local-operator"),
            request.comment.as_deref(),
        )
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(Json(detail))
}

async fn publish_skill_version(
    Path(version_id): Path<Uuid>,
    State(state): State<AppState>,
    Json(request): Json<SkillActionRequest>,
) -> Result<Json<crate::db::SkillDetail>, AppError> {
    let detail = state
        .db
        .publish_skill_version(
            version_id,
            request.actor.as_deref().unwrap_or("local-operator"),
            request.comment.as_deref(),
        )
        .await
        .map_err(|error| AppError::BadRequest(error.to_string()))?;
    Ok(Json(detail))
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

fn stream_terminal_entries(
    stored: Vec<crate::db::TerminalOutput>,
    receiver: tokio::sync::broadcast::Receiver<crate::db::TerminalOutput>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let event_stream = stream! {
        let mut last_sequence = -1_i64;
        for entry in stored {
            last_sequence = last_sequence.max(entry.sequence);
            yield Ok(Event::default().json_data(&entry).expect("terminal output should serialize"));
        }

        let mut receiver = BroadcastStream::new(receiver);
        while let Some(item) = receiver.next().await {
            match item {
                Ok(entry) => {
                    if entry.sequence <= last_sequence {
                        continue;
                    }
                    last_sequence = entry.sequence;
                    yield Ok(Event::default().json_data(&entry).expect("terminal output should serialize"));
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
        "ollama" => Ok(ProviderKind::Ollama),
        "llama_cpp" => Ok(ProviderKind::LlamaCpp),
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
    pub auth_required: bool,
    pub data_boundary: String,
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
            display_name: value.provider.display_name().to_string(),
            installed: matches!(value.status, ProviderInstallStatus::Installed),
            version: value.version,
            auth_status: format!("{:?}", value.auth_status).to_ascii_lowercase(),
            auth_required: value.provider.requires_auth(),
            data_boundary: if value.provider.is_local() {
                "local_only".to_string()
            } else {
                "external".to_string()
            },
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

#[derive(Debug, Deserialize)]
pub struct CreateWorkflowRequest {
    pub title: Option<String>,
    pub objective: String,
    pub sensitivity: Option<String>,
    pub coordinator_provider: Option<ProviderKind>,
    pub template_key: Option<String>,
    pub auto_start: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkflowHandoffRequest {
    pub from_agent_id: Option<Uuid>,
    pub to_agent_id: Option<Uuid>,
    pub phase: String,
    pub handoff_type: String,
    pub task_ref: String,
    pub priority: Option<String>,
    pub context_summary: String,
    pub relevant_artifact_ids: Option<Vec<Uuid>>,
    pub dependencies: Option<Vec<String>>,
    pub constraints: Option<Vec<String>>,
    pub deliverable_request: String,
    pub acceptance_criteria: Option<Vec<String>>,
    pub evidence_required: Option<Vec<String>>,
    pub status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WorkflowQaStatusResponse {
    pub qa_verdicts: Vec<crate::db::WorkflowQaVerdict>,
    pub release_verdicts: Vec<crate::db::WorkflowReleaseVerdict>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkflowSnapshotRequest {
    pub agent_id: Option<Uuid>,
    pub snapshot_type: Option<String>,
    pub label: Option<String>,
    pub rollback_target: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct RollbackWorkflowRequest {
    pub snapshot_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct ApprovalDecisionRequest {
    pub approved: bool,
}

fn normalize_option(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed.to_string())
}

fn normalize_option_ref(value: Option<String>) -> Option<String> {
    value.and_then(normalize_option)
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut normalized = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn validate_skill_type(value: &str) -> Result<&str, AppError> {
    match value {
        "library" | "agent-context" | "cli" | "provider" | "policy" => Ok(value),
        other => Err(AppError::BadRequest(format!("invalid skill_type {other}"))),
    }
}

fn validate_provider_exposure(value: &str) -> Result<&str, AppError> {
    match value {
        "local_only" | "agent_context_only" | "provider_allowed" => Ok(value),
        other => Err(AppError::BadRequest(format!(
            "invalid provider_exposure {other}"
        ))),
    }
}

fn validate_assignment_target_type(value: &str) -> Result<&str, AppError> {
    match value {
        "workflow_template" | "agent_role" | "provider" => Ok(value),
        other => Err(AppError::BadRequest(format!(
            "invalid assignment target_type {other}"
        ))),
    }
}

fn default_provider_exposure(skill_type: &str) -> &'static str {
    match skill_type {
        "agent-context" => "agent_context_only",
        "provider" => "provider_allowed",
        _ => "local_only",
    }
}

fn validate_sensitivity_levels(values: Vec<String>) -> Result<Vec<String>, AppError> {
    let normalized = normalize_string_list(values);
    if normalized.is_empty() {
        return Err(AppError::BadRequest(
            "allowed_sensitivity_levels cannot be empty".into(),
        ));
    }

    for value in &normalized {
        match value.as_str() {
            "public" | "internal" | "sensitive" => {}
            other => {
                return Err(AppError::BadRequest(format!(
                    "invalid allowed_sensitivity_levels value {other}"
                )));
            }
        }
    }

    Ok(normalized)
}

fn to_skill_version_input(
    request: CreateSkillVersionRequest,
    default_actor: &str,
) -> Result<crate::db::CreateSkillVersionInput, AppError> {
    if request.summary.trim().is_empty() {
        return Err(AppError::BadRequest("summary is required".into()));
    }

    let created_by = request
        .created_by
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_actor)
        .to_string();

    Ok(crate::db::CreateSkillVersionInput {
        summary: request.summary.trim().to_string(),
        body: request.body,
        examples: normalize_string_list(request.examples.unwrap_or_default()),
        constraints: normalize_string_list(request.constraints.unwrap_or_default()),
        review_notes: request.review_notes.and_then(normalize_option),
        created_by,
        source_ref: request.source_ref.and_then(normalize_option),
        dataset_pack_key: request.dataset_pack_key.and_then(normalize_option),
    })
}

fn resolve_model_path(state: &AppState, input: &str) -> std::path::PathBuf {
    let candidate = std::path::PathBuf::from(input);
    if candidate.is_absolute() {
        candidate
    } else {
        state.config.llama_cpp_model_dir.join(candidate)
    }
}

#[derive(Debug, Deserialize)]
pub struct UpsertMcpServerRequest {
    pub name: String,
    pub command: String,
    pub args: Option<Vec<String>>,
    pub local_only: Option<bool>,
    pub enabled: Option<bool>,
    pub allowed_providers: Option<Vec<ProviderKind>>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertLlamaCppModelRequest {
    pub alias: String,
    pub file_path: String,
    pub context_window: Option<i32>,
    pub quantization: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct EnabledRequest {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAgentProviderRequest {
    pub provider: ProviderKind,
}

#[derive(Debug, Deserialize)]
pub struct EscalateAgentRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListSkillsQuery {
    pub tenant_key: Option<String>,
    pub skill_type: Option<String>,
    pub status: Option<String>,
    pub tag: Option<String>,
    pub owner: Option<String>,
    pub sensitivity: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSkillVersionRequest {
    pub summary: String,
    pub body: serde_json::Value,
    pub examples: Option<Vec<String>>,
    pub constraints: Option<Vec<String>>,
    pub review_notes: Option<String>,
    pub created_by: Option<String>,
    pub source_ref: Option<String>,
    pub dataset_pack_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSkillRequest {
    pub tenant_key: Option<String>,
    pub slug: String,
    pub name: String,
    pub skill_type: String,
    pub description: String,
    pub owner: Option<String>,
    pub visibility: Option<String>,
    pub tags: Option<Vec<String>>,
    pub allowed_sensitivity_levels: Option<Vec<String>>,
    pub provider_exposure: Option<String>,
    pub source_kind: Option<String>,
    pub initial_version: CreateSkillVersionRequest,
}

#[derive(Debug, Deserialize)]
pub struct CreateSkillAssignmentRequest {
    pub skill_version_id: Uuid,
    pub target_type: String,
    pub target_key: String,
}

#[derive(Debug, Deserialize)]
pub struct SkillActionRequest {
    pub actor: Option<String>,
    pub comment: Option<String>,
}
