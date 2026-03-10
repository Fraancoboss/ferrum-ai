use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use anyhow::Context;
use orchestrator_core::{
    EventKind, NormalizedEvent, ProviderKind, RunAccumulator, RunMode, RunRequest,
    normalize_stream_line,
};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde_json::json;
use tokio::{process::Command, sync::mpsc};
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
    db::{TerminalSession, WorkflowAgent, WorkflowDetail, WorkflowSummary},
    process::build_provider_map,
    state::AppState,
};

#[derive(Debug, Clone)]
pub struct AgentTemplate {
    pub name: String,
    pub role: String,
    pub provider: ProviderKind,
    pub current_task: String,
    pub phase: String,
    pub sensitivity: String,
    pub approval_required: bool,
    pub dependency_roles: Vec<&'static str>,
    pub worktree_path: String,
}

pub async fn initialize_workflow(
    state: &AppState,
    workflow: &WorkflowSummary,
) -> anyhow::Result<WorkflowDetail> {
    let root = workflow_root(&state.config.workspace_dir, workflow.id);
    tokio::fs::create_dir_all(&root).await?;
    state
        .db
        .append_workflow_evidence(
            workflow.id,
            "system",
            None,
            "workflow_initialized",
            json!({
                "workflow_id": workflow.id,
                "template_key": workflow.template_key,
                "objective": workflow.objective,
            }),
        )
        .await?;

    let mut role_ids = std::collections::HashMap::new();
    let mut created = Vec::new();
    for template in default_templates(workflow, &root).await? {
        let dependency_ids = template
            .dependency_roles
            .iter()
            .filter_map(|role| role_ids.get(*role).copied())
            .collect::<Vec<_>>();
        let agent = state
            .db
            .create_workflow_agent(
                workflow.id,
                &template.name,
                &template.role,
                template.provider,
                &template.current_task,
                &format!(
                    "{}:{}:{}",
                    template.role,
                    template.provider.as_str(),
                    template.current_task
                ),
                &dependency_ids,
                Some(&template.worktree_path),
                &template.sensitivity,
                template.approval_required,
            )
            .await?;
        state
            .db
            .append_workflow_evidence(
                workflow.id,
                "system",
                Some(agent.id),
                "agent_registered",
                json!({
                    "agent_id": agent.id,
                    "role": template.role,
                    "provider": template.provider.as_str(),
                    "phase": template.phase,
                }),
            )
            .await?;
        role_ids.insert(template.role.clone(), agent.id);
        let terminal = state
            .db
            .create_terminal_session(
                workflow.id,
                agent.id,
                &template.name,
                template.provider,
                Some(&template.worktree_path),
            )
            .await?;
        state.hub.ensure_terminal_sender(terminal.id).await;
        created.push((agent, terminal, template));
    }

    for (agent, _, template) in &created {
        let dependency_ids = template
            .dependency_roles
            .iter()
            .filter_map(|role| role_ids.get(*role).copied())
            .collect::<Vec<_>>();
        if dependency_ids.is_empty() {
            state
                .db
                .create_workflow_handoff(
                    workflow.id,
                    None,
                    Some(agent.id),
                    &template.phase,
                    "phase_gate",
                    &template.current_task,
                    "high",
                    "Initial phase entry point.",
                    &[],
                    &[],
                    &[],
                    &template.current_task,
                    &default_acceptance_criteria(&template.role),
                    &default_evidence_requirements(&template.role),
                    "open",
                )
                .await?;
        } else {
            for dependency_id in dependency_ids {
                state
                    .db
                    .create_workflow_handoff(
                        workflow.id,
                        Some(dependency_id),
                        Some(agent.id),
                        &template.phase,
                        "standard",
                        &template.current_task,
                        "normal",
                        &format!("Dependent execution for role {}", template.role),
                        &[],
                        &template
                            .dependency_roles
                            .iter()
                            .map(|role| role.to_string())
                            .collect::<Vec<_>>(),
                        &[],
                        &template.current_task,
                        &default_acceptance_criteria(&template.role),
                        &default_evidence_requirements(&template.role),
                        "open",
                    )
                    .await?;
            }
        }
    }

    for (agent, _, template) in &created {
        if template.approval_required {
            state
                .db
                .set_agent_status(agent.id, "gated", Some("Waiting for approval"))
                .await?;
            state
                .db
                .create_approval_gate(
                    workflow.id,
                    Some(agent.id),
                    "provider_egress",
                    Some(template.provider),
                    "External provider requested for a non-public workflow. Approval required before data leaves the host.",
                    json!({
                        "role": template.role,
                        "provider": template.provider.as_str(),
                        "sensitivity": workflow.sensitivity,
                    }),
                )
                .await?;
        }
    }

    state
        .db
        .update_workflow_runtime(
            workflow.id,
            Some(
                if created
                    .iter()
                    .any(|(_, _, template)| template.approval_required)
                {
                    "awaiting_approval"
                } else {
                    "planned"
                },
            ),
            Some("planning"),
            Some("open"),
            Some(Some("Waiting for initial plan execution")),
            Some(None),
        )
        .await?;

    if let Some(detail) = state.db.get_workflow_detail(workflow.id).await? {
        state
            .db
            .create_workflow_snapshot(
                workflow.id,
                None,
                "checkpoint",
                "Workflow initialized",
                json!({
                    "workflow": detail.workflow,
                    "agents": detail.agents,
                    "terminals": detail.terminals,
                }),
                true,
            )
            .await?;
    }

    state
        .db
        .get_workflow_detail(workflow.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("workflow {} was not initialized", workflow.id))
}

pub fn spawn_workflow(state: AppState, workflow_id: Uuid) {
    tokio::spawn(async move {
        if let Err(error) = run_scheduler(state, workflow_id).await {
            error!("workflow scheduler failed: {error}");
        }
    });
}

pub async fn run_scheduler(state: AppState, workflow_id: Uuid) -> anyhow::Result<()> {
    reconcile_workflow_status(&state, workflow_id).await?;
    let Some(detail) = state.db.get_workflow_detail(workflow_id).await? else {
        return Ok(());
    };

    let completed: std::collections::HashSet<Uuid> = detail
        .agents
        .iter()
        .filter(|agent| agent.status == "completed")
        .map(|agent| agent.id)
        .collect();

    let pending_approvals: std::collections::HashSet<Uuid> = detail
        .approvals
        .iter()
        .filter(|approval| approval.status == "pending")
        .filter_map(|approval| approval.agent_id)
        .collect();

    for agent in detail
        .agents
        .iter()
        .filter(|agent| agent.status == "pending")
    {
        if pending_approvals.contains(&agent.id) {
            continue;
        }
        if !agent
            .dependency_ids
            .iter()
            .all(|dependency| completed.contains(dependency))
        {
            continue;
        }
        if !state.db.claim_pending_agent(agent.id).await? {
            continue;
        }
        state
            .db
            .append_workflow_evidence(
                workflow_id,
                "system",
                Some(agent.id),
                "agent_claimed",
                json!({
                    "agent_id": agent.id,
                    "role": agent.role,
                    "phase": phase_for_role(&agent.role),
                }),
            )
            .await?;
        let terminal = detail
            .terminals
            .iter()
            .find(|terminal| terminal.agent_id == agent.id)
            .cloned()
            .context("missing terminal for workflow agent")?;
        spawn_agent(
            state.clone(),
            detail.workflow.clone(),
            agent.clone(),
            terminal,
        );
    }

    reconcile_workflow_status(&state, workflow_id).await?;
    Ok(())
}

pub fn spawn_agent(
    state: AppState,
    workflow: WorkflowSummary,
    agent: WorkflowAgent,
    terminal: TerminalSession,
) {
    tokio::spawn(async move {
        if let Err(error) = run_agent(state, workflow, agent, terminal).await {
            error!("agent run failed: {error}");
        }
    });
}

async fn run_agent(
    state: AppState,
    workflow: WorkflowSummary,
    agent: WorkflowAgent,
    terminal: TerminalSession,
) -> anyhow::Result<()> {
    let detail = state
        .db
        .get_workflow_detail(workflow.id)
        .await?
        .context("workflow disappeared before agent run")?;
    let allowed_mcp = state
        .db
        .list_enabled_mcp_servers()
        .await?
        .into_iter()
        .filter(|server| server.local_only)
        .filter(|server| server.allowed_providers.contains(&agent.provider))
        .map(|server| {
            format!(
                "{} => {} {}",
                server.name,
                server.command,
                server.args.join(" ")
            )
        })
        .collect::<Vec<_>>();
    let adapter = state
        .provider(agent.provider)
        .or_else(|| build_provider_map().get(&agent.provider).cloned())
        .context("missing provider adapter")?;
    let prefs = {
        let guard = state.provider_prefs.read().await;
        guard.get(&agent.provider).cloned().unwrap_or_default()
    };

    let prompt = build_agent_prompt(&workflow, &agent, &detail, &allowed_mcp);
    let cwd = agent
        .worktree_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| state.config.workspace_dir.clone());
    let spec = adapter.run_command(&RunRequest {
        prompt,
        cwd,
        mode: RunMode::New,
        provider_session_ref: None,
        model: prefs.model,
        effort: prefs.effort,
    });

    state
        .db
        .mark_terminal_running(terminal.id, &spec.display())
        .await?;
    state
        .db
        .update_workflow_runtime(
            workflow.id,
            Some("running"),
            Some(&phase_for_role(&agent.role)),
            Some("open"),
            Some(Some(agent.current_task.as_str())),
            Some(None),
        )
        .await?;
    state
        .db
        .append_terminal_output(terminal.id, 0, &format!("$ {}", spec.display()))
        .await?;
    state
        .db
        .append_workflow_evidence(
            workflow.id,
            "agent",
            Some(agent.id),
            "terminal_started",
            json!({
                "terminal_id": terminal.id,
                "provider": agent.provider.as_str(),
                "command": spec.display(),
            }),
        )
        .await?;

    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 120,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut command = CommandBuilder::new(&spec.program);
    for arg in &spec.args {
        command.arg(arg);
    }
    if let Some(cwd) = &spec.cwd {
        command.cwd(cwd);
    }
    if agent.provider == ProviderKind::Claude {
        if let Some(path) = orchestrator_core::detect_windows_git_bash() {
            command.env("CLAUDE_CODE_GIT_BASH_PATH", path);
        } else if cfg!(windows) {
            warn!("Git Bash not detected for Claude Code");
        }
    }

    let mut child = pair.slave.spawn_command(command)?;
    let reader = pair.master.try_clone_reader()?;
    drop(pair.slave);

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        let mut buffered = BufReader::new(reader);
        loop {
            let mut line = String::new();
            match buffered.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                    if tx.send(trimmed).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = tx.send(format!("stream read error: {error}"));
                    break;
                }
            }
        }
    });

    let mut sequence = 1_i64;
    let mut acc = RunAccumulator::default();
    let mut transcript = String::new();
    while let Some(line) = rx.recv().await {
        transcript.push_str(&line);
        transcript.push('\n');
        let output = state
            .db
            .append_terminal_output(terminal.id, sequence, &line)
            .await?;
        state.hub.publish_terminal(terminal.id, output).await;
        for event in normalize_stream_line(agent.provider, sequence, &line) {
            apply_event(&mut acc, &event);
        }
        sequence += 1;
    }

    let status = tokio::task::spawn_blocking(move || child.wait()).await??;
    let final_status = if status.success() {
        "completed"
    } else {
        "failed"
    };

    state
        .db
        .set_agent_status(
            agent.id,
            final_status,
            Some(if status.success() {
                "Task completed"
            } else {
                "Task failed"
            }),
        )
        .await?;
    state
        .db
        .complete_terminal_session(terminal.id, final_status)
        .await?;

    let artifact_body = if !acc.assistant_text.trim().is_empty() {
        acc.assistant_text.trim().to_string()
    } else {
        transcript.trim().to_string()
    };
    if !artifact_body.is_empty() {
        let artifact = state
            .db
            .create_workflow_artifact(
                workflow.id,
                Some(agent.id),
                &format!("{} output", agent.name),
                "agent_report",
                &artifact_body,
                &format!("{}:{}:{}", workflow.id, agent.role, artifact_body.len()),
                &agent.sensitivity,
            )
            .await?;
        state
            .db
            .append_workflow_evidence(
                workflow.id,
                "agent",
                Some(agent.id),
                "artifact_created",
                json!({
                    "artifact_id": artifact.id,
                    "kind": artifact.kind,
                    "title": artifact.title,
                }),
            )
            .await?;
        record_agent_outcome(&state, &workflow, &agent, &artifact.id, status.success()).await?;
    } else {
        record_agent_outcome(&state, &workflow, &agent, &Uuid::nil(), status.success()).await?;
    }

    state
        .db
        .append_workflow_evidence(
            workflow.id,
            "agent",
            Some(agent.id),
            "terminal_completed",
            json!({
                "terminal_id": terminal.id,
                "status": final_status,
                "success": status.success(),
            }),
        )
        .await?;

    if let Some(detail) = state.db.get_workflow_detail(workflow.id).await? {
        state
            .db
            .create_workflow_snapshot(
                workflow.id,
                Some(agent.id),
                "checkpoint",
                &format!("{} {}", agent.role, final_status),
                json!({
                    "workflow": detail.workflow,
                    "agents": detail.agents,
                    "terminals": detail.terminals,
                }),
                true,
            )
            .await?;
    }

    reconcile_workflow_status(&state, workflow.id).await?;
    run_scheduler(state, workflow.id).await?;
    Ok(())
}

async fn reconcile_workflow_status(state: &AppState, workflow_id: Uuid) -> anyhow::Result<()> {
    let Some(detail) = state.db.get_workflow_detail(workflow_id).await? else {
        return Ok(());
    };

    let latest_release = detail.release_verdicts.first();
    let latest_qa = detail.qa_verdicts.first();
    let (status, phase_gate_status, blocked_reason) = if detail
        .handoffs
        .iter()
        .any(|handoff| handoff.handoff_type == "escalation" && handoff.status == "open")
    {
        (
            "attention",
            "blocked",
            Some("Escalation requires operator review"),
        )
    } else if matches!(
        latest_release.map(|value| value.verdict.as_str()),
        Some("pass")
    ) {
        ("completed", "passed", None)
    } else if detail
        .approvals
        .iter()
        .any(|approval| approval.status == "pending")
    {
        (
            if detail.agents.iter().any(|agent| agent.status == "running") {
                "running"
            } else {
                "awaiting_approval"
            },
            "pending",
            None,
        )
    } else if matches!(latest_qa.map(|value| value.verdict.as_str()), Some("fail"))
        || detail.agents.iter().any(|agent| agent.status == "failed")
    {
        ("attention", "blocked", Some("QA loop requires fixes"))
    } else if detail.agents.iter().any(|agent| agent.status == "running") {
        ("running", "open", None)
    } else if detail
        .agents
        .iter()
        .all(|agent| agent.status == "completed")
    {
        ("completed", "passed", None)
    } else {
        ("planned", "open", None)
    };

    let next_action = detail
        .agents
        .iter()
        .find(|agent| agent.status == "pending")
        .map(|agent| agent.current_task.as_str());
    let phase = current_phase(&detail);
    state
        .db
        .update_workflow_runtime(
            workflow_id,
            Some(status),
            Some(&phase),
            Some(phase_gate_status),
            Some(next_action),
            Some(blocked_reason),
        )
        .await?;
    Ok(())
}

async fn default_templates(
    workflow: &WorkflowSummary,
    root: &Path,
) -> anyhow::Result<Vec<AgentTemplate>> {
    let planner_dir = ensure_agent_workspace(root, "planner").await?;
    let coder_dir = ensure_agent_workspace(root, "coder").await?;
    let researcher_dir = ensure_agent_workspace(root, "researcher").await?;
    let qa_dir = ensure_agent_workspace(root, "evidence_collector").await?;
    let release_dir = ensure_agent_workspace(root, "reality_checker").await?;

    let gated_external = workflow.sensitivity != "public";

    let mut templates = vec![
        AgentTemplate {
            name: "Planner".to_string(),
            role: "planner".to_string(),
            provider: ProviderKind::Ollama,
            current_task: format!(
                "Break the objective into distinct execution tracks: {}",
                workflow.objective
            ),
            phase: "planning".to_string(),
            sensitivity: workflow.sensitivity.clone(),
            approval_required: false,
            dependency_roles: Vec::new(),
            worktree_path: planner_dir,
        },
        AgentTemplate {
            name: "Coder".to_string(),
            role: "coder".to_string(),
            provider: ProviderKind::Claude,
            current_task: format!(
                "Produce the implementation path and edits for: {}",
                workflow.objective
            ),
            phase: "execution".to_string(),
            sensitivity: workflow.sensitivity.clone(),
            approval_required: gated_external,
            dependency_roles: vec!["planner"],
            worktree_path: coder_dir,
        },
        AgentTemplate {
            name: "Evidence Collector".to_string(),
            role: "evidence_collector".to_string(),
            provider: ProviderKind::Ollama,
            current_task: format!(
                "Validate the implementation against acceptance criteria for: {}",
                workflow.objective
            ),
            phase: "qa_loop".to_string(),
            sensitivity: workflow.sensitivity.clone(),
            approval_required: false,
            dependency_roles: vec!["coder"],
            worktree_path: qa_dir,
        },
    ];

    if workflow.template_key != "micro" {
        templates.insert(
            1,
            AgentTemplate {
                name: "Researcher".to_string(),
                role: "researcher".to_string(),
                provider: ProviderKind::Codex,
                current_task: format!(
                    "Investigate the codebase, constraints, and external unknowns relevant to: {}",
                    workflow.objective
                ),
                phase: "architecture".to_string(),
                sensitivity: workflow.sensitivity.clone(),
                approval_required: gated_external,
                dependency_roles: vec!["planner"],
                worktree_path: researcher_dir,
            },
        );
    }

    if workflow.template_key != "micro" {
        templates.push(AgentTemplate {
            name: "Reality Checker".to_string(),
            role: "reality_checker".to_string(),
            provider: ProviderKind::Ollama,
            current_task: format!(
                "Decide if the workflow is ready for release: {}",
                workflow.objective
            ),
            phase: "release_decision".to_string(),
            sensitivity: workflow.sensitivity.clone(),
            approval_required: false,
            dependency_roles: vec!["evidence_collector"],
            worktree_path: release_dir,
        });
    }

    Ok(templates)
}

async fn ensure_agent_workspace(root: &Path, role: &str) -> anyhow::Result<String> {
    let dir = root.join(role);
    if tokio::fs::try_exists(&dir).await? && dir.join(".git").exists() {
        return Ok(dir.to_string_lossy().to_string());
    }

    if let Some(parent) = dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    if let Some(git_root) = discover_git_root(root).await? {
        if ensure_git_worktree(&git_root, &dir).await.is_ok() {
            return Ok(dir.to_string_lossy().to_string());
        }
    }

    tokio::fs::create_dir_all(&dir).await?;
    Ok(dir.to_string_lossy().to_string())
}

fn workflow_root(base: &Path, workflow_id: Uuid) -> PathBuf {
    let repo_name = base
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workspace");
    base.parent()
        .unwrap_or(base)
        .join(".ferrum-worktrees")
        .join(repo_name)
        .join(workflow_id.to_string())
}

fn build_agent_prompt(
    workflow: &WorkflowSummary,
    agent: &WorkflowAgent,
    detail: &WorkflowDetail,
    allowed_mcp: &[String],
) -> String {
    let dependency_context = detail
        .artifacts
        .iter()
        .filter(|artifact| {
            artifact
                .agent_id
                .is_some_and(|id| agent.dependency_ids.contains(&id))
        })
        .map(|artifact| format!("## {}\n{}", artifact.title, artifact.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    let role_instructions = match agent.role.as_str() {
        "planner" => {
            "Produce a crisp execution plan with non-overlapping tracks. Focus on decomposition, data boundaries, and a handoff-friendly checklist."
        }
        "researcher" => {
            "Investigate only the questions assigned to you. Return facts, constraints, and risks. Do not re-plan the whole job."
        }
        "coder" => {
            "Focus on implementation strategy and concrete code-level work. Avoid repeating research unless it changes the implementation."
        }
        "evidence_collector" => {
            "Validate the current implementation using concrete evidence. Return pass or fail with exact findings and no hand-waving."
        }
        "reality_checker" => {
            "Act as the release authority. Decide whether the workflow is actually ready or still needs work."
        }
        _ => "Complete the assigned task and return a concise report.",
    };

    let sensitivity_instructions = if workflow.sensitivity == "public" {
        "The workflow is public. External providers are allowed."
    } else {
        "The workflow is not public. Do not expose secrets or sensitive data. If context is missing, continue with local-only reasoning and explicitly flag blocked areas."
    };
    let tool_registry = if allowed_mcp.is_empty() {
        "No MCP servers are allowlisted for this provider in this workflow.".to_string()
    } else {
        format!(
            "Allowlisted local MCP servers:\n- {}",
            allowed_mcp.join("\n- ")
        )
    };

    if dependency_context.is_empty() {
        format!(
            "You are the {role} agent in a coordinated workflow.\n\nObjective:\n{objective}\n\nCurrent task:\n{task}\n\nTool policy:\n{tool_registry}\n\nRules:\n- {role_instructions}\n- {sensitivity_instructions}\n- Use only the listed local MCP servers when you need tools.\n- Produce a concise, implementation-ready report.\n",
            role = agent.role,
            objective = workflow.objective,
            task = agent.current_task,
            tool_registry = tool_registry,
            role_instructions = role_instructions,
            sensitivity_instructions = sensitivity_instructions,
        )
    } else {
        format!(
            "You are the {role} agent in a coordinated workflow.\n\nObjective:\n{objective}\n\nCurrent task:\n{task}\n\nDependency context:\n{dependency_context}\n\nTool policy:\n{tool_registry}\n\nRules:\n- {role_instructions}\n- {sensitivity_instructions}\n- Reuse the dependency context instead of repeating work.\n- Use only the listed local MCP servers when you need tools.\n- Produce a concise, implementation-ready report.\n",
            role = agent.role,
            objective = workflow.objective,
            task = agent.current_task,
            dependency_context = dependency_context,
            tool_registry = tool_registry,
            role_instructions = role_instructions,
            sensitivity_instructions = sensitivity_instructions,
        )
    }
}

fn default_acceptance_criteria(role: &str) -> Vec<String> {
    match role {
        "planner" => vec![
            "Break the work into non-overlapping tracks".to_string(),
            "Flag data-boundary and dependency risks".to_string(),
        ],
        "researcher" => vec![
            "Capture factual constraints grounded in the codebase".to_string(),
            "List external unknowns separately from confirmed facts".to_string(),
        ],
        "coder" => vec![
            "Describe concrete implementation steps".to_string(),
            "Show how the edits satisfy the objective".to_string(),
        ],
        "evidence_collector" => vec![
            "Return an explicit PASS or FAIL".to_string(),
            "Attach exact issues when failing".to_string(),
        ],
        "reality_checker" => vec![
            "Issue a release verdict".to_string(),
            "Block release when evidence is incomplete".to_string(),
        ],
        _ => vec!["Return a clear deliverable".to_string()],
    }
}

fn default_evidence_requirements(role: &str) -> Vec<String> {
    match role {
        "evidence_collector" | "reality_checker" => vec![
            "Reference artifacts by id or title".to_string(),
            "State exact acceptance criteria coverage".to_string(),
        ],
        _ => vec!["Produce a reusable artifact".to_string()],
    }
}

fn phase_for_role(role: &str) -> String {
    match role {
        "planner" => "planning",
        "researcher" => "architecture",
        "coder" => "execution",
        "evidence_collector" => "qa_loop",
        "reality_checker" => "release_decision",
        _ => "execution",
    }
    .to_string()
}

fn current_phase(detail: &WorkflowDetail) -> String {
    detail
        .agents
        .iter()
        .find(|agent| agent.status == "running")
        .or_else(|| detail.agents.iter().find(|agent| agent.status == "pending"))
        .map(|agent| phase_for_role(&agent.role))
        .unwrap_or_else(|| "release_decision".to_string())
}

async fn record_agent_outcome(
    state: &AppState,
    workflow: &WorkflowSummary,
    agent: &WorkflowAgent,
    artifact_id: &Uuid,
    success: bool,
) -> anyhow::Result<()> {
    let phase = phase_for_role(&agent.role);

    let detail = state
        .db
        .get_workflow_detail(workflow.id)
        .await?
        .context("workflow disappeared while recording agent outcome")?;

    for handoff in detail
        .handoffs
        .iter()
        .filter(|handoff| handoff.to_agent_id == Some(agent.id) && handoff.status == "open")
    {
        state.db.resolve_handoff(handoff.id, "consumed").await?;
    }

    match agent.role.as_str() {
        "evidence_collector" => {
            state.db.increment_workflow_attempts(workflow.id).await?;
            let refreshed = state
                .db
                .get_workflow(workflow.id)
                .await?
                .context("workflow missing after attempt increment")?;
            let attempt = refreshed.attempt_counter;
            let verdict = if success { "pass" } else { "fail" };
            let findings = if success {
                Vec::new()
            } else {
                vec!["Evidence collector could not validate the implementation".to_string()]
            };
            state
                .db
                .create_qa_verdict(
                    workflow.id,
                    Some(agent.id),
                    &phase,
                    verdict,
                    if success {
                        "Evidence collected and acceptance criteria satisfied."
                    } else {
                        "Implementation did not satisfy the evidence gate."
                    },
                    &findings,
                    if *artifact_id == Uuid::nil() {
                        &[]
                    } else {
                        std::slice::from_ref(artifact_id)
                    },
                    attempt,
                )
                .await?;
            state
                .db
                .append_workflow_evidence(
                    workflow.id,
                    "agent",
                    Some(agent.id),
                    "qa_verdict_recorded",
                    json!({
                        "verdict": verdict,
                        "attempt": attempt,
                        "phase": phase,
                    }),
                )
                .await?;

            if success {
                if let Some(reality_checker) = detail
                    .agents
                    .iter()
                    .find(|candidate| candidate.role == "reality_checker")
                {
                    state
                        .db
                        .create_workflow_handoff(
                            workflow.id,
                            Some(agent.id),
                            Some(reality_checker.id),
                            "release_decision",
                            "qa_pass",
                            "release review",
                            "high",
                            "QA passed. Release decision required.",
                            if *artifact_id == Uuid::nil() {
                                &[]
                            } else {
                                std::slice::from_ref(artifact_id)
                            },
                            &["qa evidence".to_string()],
                            &[],
                            "Review the evidence and decide whether the workflow is ready.",
                            &["Issue a release verdict".to_string()],
                            &["Reference the QA artifact".to_string()],
                            "open",
                        )
                        .await?;
                }
            } else if attempt >= 3 {
                state
                    .db
                    .create_workflow_handoff(
                        workflow.id,
                        Some(agent.id),
                        None,
                        "qa_loop",
                        "escalation",
                        "qa escalation",
                        "high",
                        "QA failed three times. Operator escalation required.",
                        if *artifact_id == Uuid::nil() {
                            &[]
                        } else {
                            std::slice::from_ref(artifact_id)
                        },
                        &[],
                        &[],
                        "Review the repeated QA failure and decide the next action.",
                        &["Inspect failure evidence".to_string()],
                        &["Check latest QA verdict".to_string()],
                        "open",
                    )
                    .await?;
            } else if let Some(coder) = detail
                .agents
                .iter()
                .find(|candidate| candidate.role == "coder")
            {
                state
                    .db
                    .set_agent_status(
                        coder.id,
                        "pending",
                        Some("Address QA findings and retry the implementation"),
                    )
                    .await?;
                state
                    .db
                    .set_agent_status(
                        agent.id,
                        "pending",
                        Some("Re-run QA after implementation fixes"),
                    )
                    .await?;
                state
                    .db
                    .create_workflow_handoff(
                        workflow.id,
                        Some(agent.id),
                        Some(coder.id),
                        "qa_loop",
                        "qa_fail",
                        "qa retry",
                        "high",
                        "QA failed. The coder must address the exact findings and re-submit.",
                        if *artifact_id == Uuid::nil() {
                            &[]
                        } else {
                            std::slice::from_ref(artifact_id)
                        },
                        &["qa verdict".to_string()],
                        &[],
                        "Fix the implementation to satisfy the failed evidence gate.",
                        &["Resolve the listed findings".to_string()],
                        &["Respond with updated implementation artifact".to_string()],
                        "open",
                    )
                    .await?;
            }
        }
        "reality_checker" => {
            let qa_ok = detail
                .qa_verdicts
                .first()
                .is_some_and(|verdict| verdict.verdict == "pass");
            let verdict = if success && qa_ok {
                "pass"
            } else {
                "needs_work"
            };
            let findings = if verdict == "pass" {
                Vec::new()
            } else {
                vec!["Release gate blocked because QA evidence is incomplete or the final review failed.".to_string()]
            };
            state
                .db
                .create_release_verdict(
                    workflow.id,
                    Some(agent.id),
                    &phase,
                    verdict,
                    if verdict == "pass" {
                        "Reality checker approved the workflow for release."
                    } else {
                        "Reality checker requires more work before release."
                    },
                    &findings,
                    if *artifact_id == Uuid::nil() {
                        &[]
                    } else {
                        std::slice::from_ref(artifact_id)
                    },
                )
                .await?;
            state
                .db
                .append_workflow_evidence(
                    workflow.id,
                    "agent",
                    Some(agent.id),
                    "release_verdict_recorded",
                    json!({
                        "verdict": verdict,
                        "phase": phase,
                    }),
                )
                .await?;
        }
        _ => {
            for downstream in detail
                .agents
                .iter()
                .filter(|candidate| candidate.dependency_ids.contains(&agent.id))
            {
                state
                    .db
                    .create_workflow_handoff(
                        workflow.id,
                        Some(agent.id),
                        Some(downstream.id),
                        &phase_for_role(&downstream.role),
                        "standard",
                        &downstream.current_task,
                        "normal",
                        &format!("{} completed and handed off downstream work.", agent.role),
                        if *artifact_id == Uuid::nil() {
                            &[]
                        } else {
                            std::slice::from_ref(artifact_id)
                        },
                        &[],
                        &[],
                        &downstream.current_task,
                        &default_acceptance_criteria(&downstream.role),
                        &default_evidence_requirements(&downstream.role),
                        "open",
                    )
                    .await?;
            }
        }
    }

    Ok(())
}

async fn discover_git_root(base: &Path) -> anyhow::Result<Option<PathBuf>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(base)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .await;

    let Ok(output) = output else {
        return Ok(None);
    };
    if !output.status.success() {
        return Ok(None);
    }

    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(root)))
    }
}

async fn ensure_git_worktree(git_root: &Path, dir: &Path) -> anyhow::Result<()> {
    if tokio::fs::try_exists(dir).await? && dir.join(".git").exists() {
        return Ok(());
    }

    if let Some(parent) = dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let status = Command::new("git")
        .arg("-C")
        .arg(git_root)
        .args(["worktree", "add", "--detach"])
        .arg(dir)
        .arg("HEAD")
        .status()
        .await
        .context("failed to create git worktree")?;

    anyhow::ensure!(status.success(), "git worktree add failed");
    Ok(())
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
