use anyhow::Context;
use chrono::{DateTime, Utc};
use orchestrator_core::{EventKind, LlmUsage, NormalizedEvent, ProviderKind, RunStatus};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .with_context(|| format!("failed to connect to postgres at {database_url}"))?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn create_chat(
        &self,
        provider: ProviderKind,
        title: String,
    ) -> anyhow::Result<ChatSummary> {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO chat_sessions (id, provider, title)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(id)
        .bind(provider.as_str())
        .bind(title.clone())
        .execute(&self.pool)
        .await?;

        Ok(ChatSummary {
            id,
            provider,
            title,
            provider_session_ref: None,
            created_at: Utc::now(),
            last_message_at: None,
            last_model: None,
        })
    }

    pub async fn list_chats(&self) -> anyhow::Result<Vec<ChatSummary>> {
        let rows = sqlx::query_as::<_, ChatSummaryRow>(
            r#"
            SELECT
                cs.id,
                cs.provider,
                cs.title,
                cs.provider_session_ref,
                cs.created_at,
                MAX(m.created_at) AS last_message_at,
                (
                    SELECT lu.model
                    FROM runs r
                    LEFT JOIN llm_usage lu ON lu.run_id = r.id
                    WHERE r.session_id = cs.id
                    ORDER BY COALESCE(r.finished_at, r.created_at) DESC
                    LIMIT 1
                ) AS last_model
            FROM chat_sessions cs
            LEFT JOIN messages m ON m.session_id = cs.id
            GROUP BY cs.id, cs.provider, cs.title, cs.provider_session_ref, cs.created_at
            ORDER BY COALESCE(MAX(m.created_at), cs.created_at) DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(ChatSummary::try_from).collect()
    }

    pub async fn get_chat(&self, chat_id: Uuid) -> anyhow::Result<Option<ChatSummary>> {
        let row = sqlx::query_as::<_, ChatSummaryRow>(
            r#"
            SELECT
                cs.id,
                cs.provider,
                cs.title,
                cs.provider_session_ref,
                cs.created_at,
                (
                    SELECT MAX(created_at)
                    FROM messages
                    WHERE session_id = cs.id
                ) AS last_message_at,
                (
                    SELECT lu.model
                    FROM runs r
                    LEFT JOIN llm_usage lu ON lu.run_id = r.id
                    WHERE r.session_id = cs.id
                    ORDER BY COALESCE(r.finished_at, r.created_at) DESC
                    LIMIT 1
                ) AS last_model
            FROM chat_sessions cs
            WHERE cs.id = $1
            "#,
        )
        .bind(chat_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(ChatSummary::try_from).transpose()
    }

    pub async fn insert_user_message(
        &self,
        chat_id: Uuid,
        content: &str,
    ) -> anyhow::Result<ChatMessage> {
        self.insert_message(chat_id, "user", content, None).await
    }

    pub async fn insert_assistant_message(
        &self,
        chat_id: Uuid,
        run_id: Uuid,
        content: &str,
    ) -> anyhow::Result<ChatMessage> {
        self.insert_message(chat_id, "assistant", content, Some(run_id))
            .await
    }

    pub async fn list_messages(&self, chat_id: Uuid) -> anyhow::Result<Vec<ChatMessage>> {
        let rows = sqlx::query_as::<_, ChatMessageRow>(
            r#"
            SELECT
                m.id,
                m.session_id,
                m.role,
                m.content,
                m.created_at,
                m.source_run_id,
                lu.model,
                CAST(lu.input_tokens AS BIGINT) AS input_tokens,
                CAST(lu.output_tokens AS BIGINT) AS output_tokens,
                CAST(lu.total_tokens AS BIGINT) AS total_tokens,
                CAST(lu.estimated_cost_usd AS DOUBLE PRECISION) AS estimated_cost_usd
            FROM messages m
            LEFT JOIN llm_usage lu ON lu.run_id = m.source_run_id
            WHERE m.session_id = $1
            ORDER BY m.created_at ASC
            "#,
        )
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn create_run(
        &self,
        chat_id: Uuid,
        provider: ProviderKind,
        command: &str,
    ) -> anyhow::Result<RunSummary> {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO runs (id, session_id, provider, command, status)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(id)
        .bind(chat_id)
        .bind(provider.as_str())
        .bind(command)
        .bind("pending")
        .execute(&self.pool)
        .await?;

        Ok(RunSummary {
            id,
            session_id: chat_id,
            provider,
            status: RunStatus::Pending,
            command: command.to_string(),
            exit_code: None,
            provider_session_ref: None,
            created_at: Utc::now(),
            finished_at: None,
        })
    }

    pub async fn mark_run_running(&self, run_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("UPDATE runs SET status = 'running' WHERE id = $1")
            .bind(run_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn complete_run(
        &self,
        run_id: Uuid,
        status: RunStatus,
        exit_code: i32,
        stdout_final: &str,
        stderr_final: &str,
        provider_session_ref: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE runs
            SET status = $2,
                exit_code = $3,
                stdout_final = $4,
                stderr_final = $5,
                provider_session_ref = $6,
                finished_at = now()
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .bind(run_status_as_str(status))
        .bind(exit_code)
        .bind(stdout_final)
        .bind(stderr_final)
        .bind(provider_session_ref)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn append_run_event(
        &self,
        run_id: Uuid,
        event: &NormalizedEvent,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO run_events (
                id, run_id, provider, sequence, event_kind, raw_event, text, usage, provider_session_ref, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(run_id)
        .bind(event.provider.as_str())
        .bind(event.sequence)
        .bind(event_kind_as_str(event.event_kind))
        .bind(event.raw.clone())
        .bind(event.text.clone())
        .bind(event.usage.clone().map(serde_json::to_value).transpose()?)
        .bind(event.provider_session_ref.clone())
        .bind(event.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_run_events(&self, run_id: Uuid) -> anyhow::Result<Vec<NormalizedEvent>> {
        let rows = sqlx::query_as::<_, EventRow>(
            r#"
            SELECT provider, sequence, event_kind, raw_event, text, usage, provider_session_ref, created_at
            FROM run_events
            WHERE run_id = $1
            ORDER BY sequence ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(EventRow::try_into).collect()
    }

    pub async fn upsert_usage(
        &self,
        run_id: Uuid,
        provider: ProviderKind,
        usage: &LlmUsage,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO llm_usage (
                id, run_id, provider, model, input_tokens, output_tokens, total_tokens, estimated_cost_usd
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (run_id) DO UPDATE
            SET model = EXCLUDED.model,
                input_tokens = EXCLUDED.input_tokens,
                output_tokens = EXCLUDED.output_tokens,
                total_tokens = EXCLUDED.total_tokens,
                estimated_cost_usd = EXCLUDED.estimated_cost_usd
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(run_id)
        .bind(provider.as_str())
        .bind(usage.model.clone())
        .bind(usage.input_tokens)
        .bind(usage.output_tokens)
        .bind(usage.total_tokens)
        .bind(usage.estimated_cost_usd)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_chat_provider_session(
        &self,
        chat_id: Uuid,
        provider_session_ref: &str,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE chat_sessions SET provider_session_ref = $2 WHERE id = $1")
            .bind(chat_id)
            .bind(provider_session_ref)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_auth_session(
        &self,
        provider: ProviderKind,
        action: &str,
        command: &str,
    ) -> anyhow::Result<AuthSession> {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO provider_auth_sessions (id, provider, action, command, status)
            VALUES ($1, $2, $3, $4, 'pending')
            "#,
        )
        .bind(id)
        .bind(provider.as_str())
        .bind(action)
        .bind(command)
        .execute(&self.pool)
        .await?;

        Ok(AuthSession {
            id,
            provider,
            action: action.to_string(),
            status: "pending".to_string(),
            command: command.to_string(),
            exit_code: None,
            created_at: Utc::now(),
            finished_at: None,
            last_output: None,
        })
    }

    pub async fn mark_auth_running(&self, auth_id: Uuid) -> anyhow::Result<()> {
        sqlx::query("UPDATE provider_auth_sessions SET status = 'running' WHERE id = $1")
            .bind(auth_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn append_auth_event(
        &self,
        auth_id: Uuid,
        event: &NormalizedEvent,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO provider_auth_events (
                id, auth_session_id, provider, sequence, event_kind, raw_event, text, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(auth_id)
        .bind(event.provider.as_str())
        .bind(event.sequence)
        .bind(event_kind_as_str(event.event_kind))
        .bind(event.raw.clone())
        .bind(event.text.clone())
        .bind(event.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_auth_events(&self, auth_id: Uuid) -> anyhow::Result<Vec<NormalizedEvent>> {
        let rows = sqlx::query_as::<_, EventRow>(
            r#"
            SELECT provider, sequence, event_kind, raw_event, text, usage, provider_session_ref, created_at
            FROM provider_auth_events
            WHERE auth_session_id = $1
            ORDER BY sequence ASC
            "#,
        )
        .bind(auth_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(EventRow::try_into).collect()
    }

    pub async fn complete_auth_session(
        &self,
        auth_id: Uuid,
        status: &str,
        exit_code: i32,
        last_output: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE provider_auth_sessions
            SET status = $2,
                exit_code = $3,
                last_output = $4,
                finished_at = now()
            WHERE id = $1
            "#,
        )
        .bind(auth_id)
        .bind(status)
        .bind(exit_code)
        .bind(last_output)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn run_is_terminal(&self, run_id: Uuid) -> anyhow::Result<bool> {
        let status =
            sqlx::query_scalar::<_, Option<String>>("SELECT status FROM runs WHERE id = $1")
                .bind(run_id)
                .fetch_optional(&self.pool)
                .await?
                .flatten();
        Ok(matches!(
            status.as_deref(),
            Some("completed" | "failed" | "cancelled")
        ))
    }

    pub async fn auth_is_terminal(&self, auth_id: Uuid) -> anyhow::Result<bool> {
        let status = sqlx::query_scalar::<_, Option<String>>(
            "SELECT status FROM provider_auth_sessions WHERE id = $1",
        )
        .bind(auth_id)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        Ok(matches!(status.as_deref(), Some("completed" | "failed")))
    }

    pub async fn usage_daily(&self) -> anyhow::Result<Vec<DailyUsage>> {
        let rows = sqlx::query_as::<_, DailyUsageRow>(
            r#"
            SELECT
                provider,
                TO_CHAR(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD') AS day,
                COALESCE(SUM(CAST(input_tokens AS BIGINT)), 0)::BIGINT AS input_tokens,
                COALESCE(SUM(CAST(output_tokens AS BIGINT)), 0)::BIGINT AS output_tokens,
                COALESCE(SUM(CAST(total_tokens AS BIGINT)), 0)::BIGINT AS total_tokens
            FROM llm_usage
            GROUP BY provider, day
            ORDER BY day DESC, provider ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(DailyUsage::try_from).collect()
    }

    pub async fn ensure_default_workflow_templates(&self) -> anyhow::Result<()> {
        for template in [
            WorkflowTemplate {
                id: Uuid::new_v4(),
                template_key: "micro".to_string(),
                name: "Micro".to_string(),
                description: "Single-track implementation with a lightweight QA pass.".to_string(),
                phases: vec![
                    "planning".to_string(),
                    "execution".to_string(),
                    "qa_loop".to_string(),
                ],
                default_agent_roles: vec![
                    "planner".to_string(),
                    "coder".to_string(),
                    "evidence_collector".to_string(),
                ],
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            WorkflowTemplate {
                id: Uuid::new_v4(),
                template_key: "sprint".to_string(),
                name: "Sprint".to_string(),
                description: "Short multi-agent sprint with research, implementation, and QA."
                    .to_string(),
                phases: vec![
                    "planning".to_string(),
                    "research".to_string(),
                    "execution".to_string(),
                    "qa_loop".to_string(),
                    "release_decision".to_string(),
                ],
                default_agent_roles: vec![
                    "planner".to_string(),
                    "researcher".to_string(),
                    "coder".to_string(),
                    "evidence_collector".to_string(),
                    "reality_checker".to_string(),
                ],
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            WorkflowTemplate {
                id: Uuid::new_v4(),
                template_key: "engineering_pipeline".to_string(),
                name: "Engineering Pipeline".to_string(),
                description:
                    "Structured engineering flow with handoffs, QA retries, and release gate."
                        .to_string(),
                phases: vec![
                    "planning".to_string(),
                    "architecture".to_string(),
                    "execution".to_string(),
                    "qa_loop".to_string(),
                    "hardening".to_string(),
                    "release_decision".to_string(),
                ],
                default_agent_roles: vec![
                    "planner".to_string(),
                    "researcher".to_string(),
                    "coder".to_string(),
                    "evidence_collector".to_string(),
                    "reality_checker".to_string(),
                ],
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        ] {
            self.upsert_workflow_template(&template).await?;
        }
        Ok(())
    }

    pub async fn create_workflow(
        &self,
        title: String,
        objective: String,
        coordinator_provider: ProviderKind,
        sensitivity: &str,
        template_key: &str,
    ) -> anyhow::Result<WorkflowSummary> {
        self.ensure_default_workflow_templates().await?;
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO workflows (
                id, title, objective, coordinator_provider, sensitivity, status, template_key,
                phase, phase_gate_status, attempt_counter, next_action
            )
            VALUES ($1, $2, $3, $4, $5, 'planned', $6, 'planning', 'open', 0, 'Initialize workflow')
            "#,
        )
        .bind(id)
        .bind(title.clone())
        .bind(objective.clone())
        .bind(coordinator_provider.as_str())
        .bind(sensitivity)
        .bind(template_key)
        .execute(&self.pool)
        .await?;

        Ok(WorkflowSummary {
            id,
            title,
            objective,
            coordinator_provider,
            sensitivity: sensitivity.to_string(),
            status: "planned".to_string(),
            template_key: template_key.to_string(),
            phase: "planning".to_string(),
            phase_gate_status: "open".to_string(),
            current_task_id: None,
            attempt_counter: 0,
            next_action: Some("Initialize workflow".to_string()),
            blocked_reason: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    }

    pub async fn list_workflows(&self) -> anyhow::Result<Vec<WorkflowSummary>> {
        self.ensure_default_workflow_templates().await?;
        let rows = sqlx::query_as::<_, WorkflowRow>(
            r#"
            SELECT
                id, title, objective, coordinator_provider, sensitivity, status, template_key,
                phase, phase_gate_status, current_task_id, attempt_counter, next_action,
                blocked_reason, created_at, updated_at
            FROM workflows
            ORDER BY updated_at DESC, created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(WorkflowSummary::try_from).collect()
    }

    pub async fn get_workflow(&self, workflow_id: Uuid) -> anyhow::Result<Option<WorkflowSummary>> {
        let row = sqlx::query_as::<_, WorkflowRow>(
            r#"
            SELECT
                id, title, objective, coordinator_provider, sensitivity, status, template_key,
                phase, phase_gate_status, current_task_id, attempt_counter, next_action,
                blocked_reason, created_at, updated_at
            FROM workflows
            WHERE id = $1
            "#,
        )
        .bind(workflow_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(WorkflowSummary::try_from).transpose()
    }

    pub async fn set_workflow_status(&self, workflow_id: Uuid, status: &str) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE workflows
            SET status = $2,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(workflow_id)
        .bind(status)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_workflow_runtime(
        &self,
        workflow_id: Uuid,
        status: Option<&str>,
        phase: Option<&str>,
        phase_gate_status: Option<&str>,
        next_action: Option<Option<&str>>,
        blocked_reason: Option<Option<&str>>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE workflows
            SET status = COALESCE($2, status),
                phase = COALESCE($3, phase),
                phase_gate_status = COALESCE($4, phase_gate_status),
                next_action = CASE WHEN $5::TEXT IS NULL THEN next_action ELSE $5 END,
                blocked_reason = CASE WHEN $6::TEXT IS NULL THEN blocked_reason ELSE $6 END,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(workflow_id)
        .bind(status)
        .bind(phase)
        .bind(phase_gate_status)
        .bind(next_action.flatten())
        .bind(blocked_reason.flatten())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn increment_workflow_attempts(&self, workflow_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE workflows
            SET attempt_counter = attempt_counter + 1,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(workflow_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_workflow_agent(
        &self,
        workflow_id: Uuid,
        name: &str,
        role: &str,
        provider: ProviderKind,
        current_task: &str,
        task_fingerprint: &str,
        dependency_ids: &[Uuid],
        worktree_path: Option<&str>,
        sensitivity: &str,
        approval_required: bool,
    ) -> anyhow::Result<WorkflowAgent> {
        let id = Uuid::new_v4();
        let dependency_ids = serde_json::to_value(dependency_ids)?;
        sqlx::query(
            r#"
            INSERT INTO workflow_agents (
                id, workflow_id, name, role, provider, status, current_task, task_fingerprint,
                dependency_ids, worktree_path, sensitivity, approval_required
            )
            VALUES ($1, $2, $3, $4, $5, 'pending', $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(id)
        .bind(workflow_id)
        .bind(name)
        .bind(role)
        .bind(provider.as_str())
        .bind(current_task)
        .bind(task_fingerprint)
        .bind(dependency_ids)
        .bind(worktree_path)
        .bind(sensitivity)
        .bind(approval_required)
        .execute(&self.pool)
        .await?;

        self.get_agent(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("agent {id} was not created"))
    }

    pub async fn get_agent(&self, agent_id: Uuid) -> anyhow::Result<Option<WorkflowAgent>> {
        let row = sqlx::query_as::<_, WorkflowAgentRow>(
            r#"
            SELECT
                id, workflow_id, name, role, provider, status, current_task, task_fingerprint,
                dependency_ids, worktree_path, sensitivity, approval_required, created_at, updated_at
            FROM workflow_agents
            WHERE id = $1
            "#,
        )
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(WorkflowAgent::try_from).transpose()
    }

    pub async fn list_workflow_agents(
        &self,
        workflow_id: Uuid,
    ) -> anyhow::Result<Vec<WorkflowAgent>> {
        let rows = sqlx::query_as::<_, WorkflowAgentRow>(
            r#"
            SELECT
                id, workflow_id, name, role, provider, status, current_task, task_fingerprint,
                dependency_ids, worktree_path, sensitivity, approval_required, created_at, updated_at
            FROM workflow_agents
            WHERE workflow_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(workflow_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(WorkflowAgent::try_from).collect()
    }

    pub async fn set_agent_status(
        &self,
        agent_id: Uuid,
        status: &str,
        current_task: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE workflow_agents
            SET status = $2,
                current_task = COALESCE($3, current_task),
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(agent_id)
        .bind(status)
        .bind(current_task)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn claim_pending_agent(&self, agent_id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE workflow_agents
            SET status = 'running',
                updated_at = now()
            WHERE id = $1
              AND status = 'pending'
            "#,
        )
        .bind(agent_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_agent_provider(
        &self,
        agent_id: Uuid,
        provider: ProviderKind,
        approval_required: bool,
        status: &str,
        current_task: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE workflow_agents
            SET provider = $2,
                approval_required = $3,
                status = $4,
                current_task = COALESCE($5, current_task),
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(agent_id)
        .bind(provider.as_str())
        .bind(approval_required)
        .bind(status)
        .bind(current_task)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_terminal_session(
        &self,
        workflow_id: Uuid,
        agent_id: Uuid,
        title: &str,
        provider: ProviderKind,
        worktree_path: Option<&str>,
    ) -> anyhow::Result<TerminalSession> {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO terminal_sessions (
                id, workflow_id, agent_id, title, provider, status, worktree_path
            )
            VALUES ($1, $2, $3, $4, $5, 'pending', $6)
            "#,
        )
        .bind(id)
        .bind(workflow_id)
        .bind(agent_id)
        .bind(title)
        .bind(provider.as_str())
        .bind(worktree_path)
        .execute(&self.pool)
        .await?;

        self.get_terminal_session(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("terminal {id} was not created"))
    }

    pub async fn get_terminal_session(
        &self,
        terminal_id: Uuid,
    ) -> anyhow::Result<Option<TerminalSession>> {
        let row = sqlx::query_as::<_, TerminalSessionRow>(
            r#"
            SELECT
                id, workflow_id, agent_id, title, provider, status, command, worktree_path,
                created_at, updated_at, finished_at
            FROM terminal_sessions
            WHERE id = $1
            "#,
        )
        .bind(terminal_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(TerminalSession::try_from).transpose()
    }

    pub async fn list_terminal_sessions(
        &self,
        workflow_id: Uuid,
    ) -> anyhow::Result<Vec<TerminalSession>> {
        let rows = sqlx::query_as::<_, TerminalSessionRow>(
            r#"
            SELECT
                id, workflow_id, agent_id, title, provider, status, command, worktree_path,
                created_at, updated_at, finished_at
            FROM terminal_sessions
            WHERE workflow_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(workflow_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(TerminalSession::try_from).collect()
    }

    pub async fn mark_terminal_running(
        &self,
        terminal_id: Uuid,
        command: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE terminal_sessions
            SET status = 'running',
                command = $2,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(terminal_id)
        .bind(command)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn complete_terminal_session(
        &self,
        terminal_id: Uuid,
        status: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE terminal_sessions
            SET status = $2,
                updated_at = now(),
                finished_at = now()
            WHERE id = $1
            "#,
        )
        .bind(terminal_id)
        .bind(status)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_terminal_provider(
        &self,
        terminal_id: Uuid,
        provider: ProviderKind,
        status: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE terminal_sessions
            SET provider = $2,
                status = $3,
                command = NULL,
                updated_at = now(),
                finished_at = NULL
            WHERE id = $1
            "#,
        )
        .bind(terminal_id)
        .bind(provider.as_str())
        .bind(status)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn append_terminal_output(
        &self,
        terminal_id: Uuid,
        sequence: i64,
        text: &str,
    ) -> anyhow::Result<TerminalOutput> {
        let output = TerminalOutput {
            terminal_session_id: terminal_id,
            sequence,
            text: text.to_string(),
            created_at: Utc::now(),
        };
        sqlx::query(
            r#"
            INSERT INTO terminal_entries (id, terminal_session_id, sequence, text, created_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(terminal_id)
        .bind(sequence)
        .bind(text)
        .bind(output.created_at)
        .execute(&self.pool)
        .await?;
        Ok(output)
    }

    pub async fn list_terminal_entries(
        &self,
        terminal_id: Uuid,
    ) -> anyhow::Result<Vec<TerminalOutput>> {
        let rows = sqlx::query_as::<_, TerminalEntryRow>(
            r#"
            SELECT terminal_session_id, sequence, text, created_at
            FROM terminal_entries
            WHERE terminal_session_id = $1
            ORDER BY sequence ASC
            "#,
        )
        .bind(terminal_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn create_workflow_artifact(
        &self,
        workflow_id: Uuid,
        agent_id: Option<Uuid>,
        title: &str,
        kind: &str,
        content: &str,
        fingerprint: &str,
        sensitivity: &str,
    ) -> anyhow::Result<WorkflowArtifact> {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO workflow_artifacts (
                id, workflow_id, agent_id, title, kind, content, fingerprint, sensitivity, reusable
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true)
            ON CONFLICT (workflow_id, fingerprint) DO UPDATE
            SET title = EXCLUDED.title,
                kind = EXCLUDED.kind,
                content = EXCLUDED.content,
                sensitivity = EXCLUDED.sensitivity
            "#,
        )
        .bind(id)
        .bind(workflow_id)
        .bind(agent_id)
        .bind(title)
        .bind(kind)
        .bind(content)
        .bind(fingerprint)
        .bind(sensitivity)
        .execute(&self.pool)
        .await?;

        let rows = self.list_workflow_artifacts(workflow_id).await?;
        rows.into_iter()
            .find(|artifact| artifact.fingerprint == fingerprint)
            .ok_or_else(|| anyhow::anyhow!("artifact {fingerprint} was not created"))
    }

    pub async fn list_workflow_artifacts(
        &self,
        workflow_id: Uuid,
    ) -> anyhow::Result<Vec<WorkflowArtifact>> {
        let rows = sqlx::query_as::<_, WorkflowArtifactRow>(
            r#"
            SELECT id, workflow_id, agent_id, title, kind, content, fingerprint, sensitivity, reusable, created_at
            FROM workflow_artifacts
            WHERE workflow_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(workflow_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn create_approval_gate(
        &self,
        workflow_id: Uuid,
        agent_id: Option<Uuid>,
        gate_type: &str,
        target_provider: Option<ProviderKind>,
        reason: &str,
        requested_context: serde_json::Value,
    ) -> anyhow::Result<ApprovalGate> {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO workflow_approvals (
                id, workflow_id, agent_id, gate_type, target_provider, status, reason, requested_context
            )
            VALUES ($1, $2, $3, $4, $5, 'pending', $6, $7)
            "#,
        )
        .bind(id)
        .bind(workflow_id)
        .bind(agent_id)
        .bind(gate_type)
        .bind(target_provider.map(ProviderKind::as_str))
        .bind(reason)
        .bind(requested_context)
        .execute(&self.pool)
        .await?;

        let rows = self.list_workflow_approvals(workflow_id).await?;
        rows.into_iter()
            .find(|approval| approval.id == id)
            .ok_or_else(|| anyhow::anyhow!("approval gate {id} was not created"))
    }

    pub async fn list_workflow_approvals(
        &self,
        workflow_id: Uuid,
    ) -> anyhow::Result<Vec<ApprovalGate>> {
        let rows = sqlx::query_as::<_, ApprovalGateRow>(
            r#"
            SELECT
                id, workflow_id, agent_id, gate_type, target_provider, status, reason,
                requested_context, created_at, resolved_at
            FROM workflow_approvals
            WHERE workflow_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(workflow_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(ApprovalGate::try_from).collect()
    }

    pub async fn update_approval_status(
        &self,
        gate_id: Uuid,
        status: &str,
    ) -> anyhow::Result<Option<ApprovalGate>> {
        let workflow_id = sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT workflow_id FROM workflow_approvals WHERE id = $1",
        )
        .bind(gate_id)
        .fetch_optional(&self.pool)
        .await?
        .flatten();

        let Some(workflow_id) = workflow_id else {
            return Ok(None);
        };

        sqlx::query(
            r#"
            UPDATE workflow_approvals
            SET status = $2,
                resolved_at = CASE WHEN $2 = 'pending' THEN NULL ELSE now() END
            WHERE id = $1
            "#,
        )
        .bind(gate_id)
        .bind(status)
        .execute(&self.pool)
        .await?;

        let rows = self.list_workflow_approvals(workflow_id).await?;
        Ok(rows.into_iter().find(|approval| approval.id == gate_id))
    }

    pub async fn get_workflow_detail(
        &self,
        workflow_id: Uuid,
    ) -> anyhow::Result<Option<WorkflowDetail>> {
        let Some(workflow) = self.get_workflow(workflow_id).await? else {
            return Ok(None);
        };

        Ok(Some(WorkflowDetail {
            workflow,
            agents: self.list_workflow_agents(workflow_id).await?,
            terminals: self.list_terminal_sessions(workflow_id).await?,
            approvals: self.list_workflow_approvals(workflow_id).await?,
            artifacts: self.list_workflow_artifacts(workflow_id).await?,
            handoffs: self.list_workflow_handoffs(workflow_id).await?,
            qa_verdicts: self.list_workflow_qa_verdicts(workflow_id).await?,
            release_verdicts: self.list_workflow_release_verdicts(workflow_id).await?,
            evidence: self.list_workflow_evidence_records(workflow_id).await?,
            snapshots: self.list_workflow_snapshots(workflow_id).await?,
        }))
    }

    pub async fn list_workflow_templates(&self) -> anyhow::Result<Vec<WorkflowTemplate>> {
        self.ensure_default_workflow_templates().await?;
        let rows = sqlx::query_as::<_, WorkflowTemplateRow>(
            r#"
            SELECT
                id, template_key, name, description, phases, default_agent_roles, created_at, updated_at
            FROM workflow_templates
            ORDER BY name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(WorkflowTemplate::try_from).collect()
    }

    pub async fn create_workflow_handoff(
        &self,
        workflow_id: Uuid,
        from_agent_id: Option<Uuid>,
        to_agent_id: Option<Uuid>,
        phase: &str,
        handoff_type: &str,
        task_ref: &str,
        priority: &str,
        context_summary: &str,
        relevant_artifact_ids: &[Uuid],
        dependencies: &[String],
        constraints: &[String],
        deliverable_request: &str,
        acceptance_criteria: &[String],
        evidence_required: &[String],
        status: &str,
    ) -> anyhow::Result<WorkflowHandoff> {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO workflow_handoffs (
                id, workflow_id, from_agent_id, to_agent_id, phase, handoff_type, task_ref,
                priority, context_summary, relevant_artifact_ids, dependencies, constraints,
                deliverable_request, acceptance_criteria, evidence_required, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            "#,
        )
        .bind(id)
        .bind(workflow_id)
        .bind(from_agent_id)
        .bind(to_agent_id)
        .bind(phase)
        .bind(handoff_type)
        .bind(task_ref)
        .bind(priority)
        .bind(context_summary)
        .bind(serde_json::to_value(relevant_artifact_ids)?)
        .bind(serde_json::to_value(dependencies)?)
        .bind(serde_json::to_value(constraints)?)
        .bind(deliverable_request)
        .bind(serde_json::to_value(acceptance_criteria)?)
        .bind(serde_json::to_value(evidence_required)?)
        .bind(status)
        .execute(&self.pool)
        .await?;

        let handoffs = self.list_workflow_handoffs(workflow_id).await?;
        handoffs
            .into_iter()
            .find(|handoff| handoff.id == id)
            .ok_or_else(|| anyhow::anyhow!("handoff {id} was not created"))
    }

    pub async fn list_workflow_handoffs(
        &self,
        workflow_id: Uuid,
    ) -> anyhow::Result<Vec<WorkflowHandoff>> {
        let rows = sqlx::query_as::<_, WorkflowHandoffRow>(
            r#"
            SELECT
                id, workflow_id, from_agent_id, to_agent_id, phase, handoff_type, task_ref, priority,
                context_summary, relevant_artifact_ids, dependencies, constraints, deliverable_request,
                acceptance_criteria, evidence_required, status, created_at, resolved_at
            FROM workflow_handoffs
            WHERE workflow_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(workflow_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(WorkflowHandoff::try_from).collect()
    }

    pub async fn resolve_handoff(&self, handoff_id: Uuid, status: &str) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE workflow_handoffs
            SET status = $2,
                resolved_at = CASE WHEN $2 = 'open' THEN NULL ELSE now() END
            WHERE id = $1
            "#,
        )
        .bind(handoff_id)
        .bind(status)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_qa_verdict(
        &self,
        workflow_id: Uuid,
        agent_id: Option<Uuid>,
        phase: &str,
        verdict: &str,
        summary: &str,
        findings: &[String],
        evidence_artifact_ids: &[Uuid],
        attempt_number: i32,
    ) -> anyhow::Result<WorkflowQaVerdict> {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO workflow_qa_verdicts (
                id, workflow_id, agent_id, phase, verdict, summary, findings,
                evidence_artifact_ids, attempt_number
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(id)
        .bind(workflow_id)
        .bind(agent_id)
        .bind(phase)
        .bind(verdict)
        .bind(summary)
        .bind(serde_json::to_value(findings)?)
        .bind(serde_json::to_value(evidence_artifact_ids)?)
        .bind(attempt_number)
        .execute(&self.pool)
        .await?;

        let verdicts = self.list_workflow_qa_verdicts(workflow_id).await?;
        verdicts
            .into_iter()
            .find(|record| record.id == id)
            .ok_or_else(|| anyhow::anyhow!("qa verdict {id} was not created"))
    }

    pub async fn list_workflow_qa_verdicts(
        &self,
        workflow_id: Uuid,
    ) -> anyhow::Result<Vec<WorkflowQaVerdict>> {
        let rows = sqlx::query_as::<_, WorkflowQaVerdictRow>(
            r#"
            SELECT
                id, workflow_id, agent_id, phase, verdict, summary, findings,
                evidence_artifact_ids, attempt_number, created_at
            FROM workflow_qa_verdicts
            WHERE workflow_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(workflow_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(WorkflowQaVerdict::try_from).collect()
    }

    pub async fn create_release_verdict(
        &self,
        workflow_id: Uuid,
        agent_id: Option<Uuid>,
        phase: &str,
        verdict: &str,
        summary: &str,
        findings: &[String],
        evidence_artifact_ids: &[Uuid],
    ) -> anyhow::Result<WorkflowReleaseVerdict> {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO workflow_release_verdicts (
                id, workflow_id, agent_id, phase, verdict, summary, findings, evidence_artifact_ids
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(id)
        .bind(workflow_id)
        .bind(agent_id)
        .bind(phase)
        .bind(verdict)
        .bind(summary)
        .bind(serde_json::to_value(findings)?)
        .bind(serde_json::to_value(evidence_artifact_ids)?)
        .execute(&self.pool)
        .await?;

        let verdicts = self.list_workflow_release_verdicts(workflow_id).await?;
        verdicts
            .into_iter()
            .find(|record| record.id == id)
            .ok_or_else(|| anyhow::anyhow!("release verdict {id} was not created"))
    }

    pub async fn list_workflow_release_verdicts(
        &self,
        workflow_id: Uuid,
    ) -> anyhow::Result<Vec<WorkflowReleaseVerdict>> {
        let rows = sqlx::query_as::<_, WorkflowReleaseVerdictRow>(
            r#"
            SELECT
                id, workflow_id, agent_id, phase, verdict, summary, findings,
                evidence_artifact_ids, created_at
            FROM workflow_release_verdicts
            WHERE workflow_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(workflow_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(WorkflowReleaseVerdict::try_from)
            .collect()
    }

    pub async fn append_workflow_evidence(
        &self,
        workflow_id: Uuid,
        actor_type: &str,
        actor_id: Option<Uuid>,
        event_type: &str,
        payload: serde_json::Value,
    ) -> anyhow::Result<WorkflowEvidenceRecord> {
        let id = Uuid::new_v4();
        let prev_hash = sqlx::query_scalar::<_, Option<String>>(
            r#"
            SELECT record_hash
            FROM workflow_evidence_records
            WHERE workflow_id = $1
            ORDER BY created_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(workflow_id)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        let created_at = Utc::now();
        let record_hash = hash_evidence_record(
            &prev_hash, actor_type, actor_id, event_type, &payload, created_at,
        );

        sqlx::query(
            r#"
            INSERT INTO workflow_evidence_records (
                id, workflow_id, actor_type, actor_id, event_type, payload, prev_hash, record_hash, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(id)
        .bind(workflow_id)
        .bind(actor_type)
        .bind(actor_id)
        .bind(event_type)
        .bind(&payload)
        .bind(prev_hash.clone())
        .bind(record_hash.clone())
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(WorkflowEvidenceRecord {
            id,
            workflow_id,
            actor_type: actor_type.to_string(),
            actor_id,
            event_type: event_type.to_string(),
            payload,
            prev_hash,
            record_hash,
            created_at,
        })
    }

    pub async fn list_workflow_evidence_records(
        &self,
        workflow_id: Uuid,
    ) -> anyhow::Result<Vec<WorkflowEvidenceRecord>> {
        let rows = sqlx::query_as::<_, WorkflowEvidenceRecordRow>(
            r#"
            SELECT id, workflow_id, actor_type, actor_id, event_type, payload, prev_hash, record_hash, created_at
            FROM workflow_evidence_records
            WHERE workflow_id = $1
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(workflow_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn create_workflow_snapshot(
        &self,
        workflow_id: Uuid,
        agent_id: Option<Uuid>,
        snapshot_type: &str,
        label: &str,
        payload: serde_json::Value,
        rollback_target: bool,
    ) -> anyhow::Result<WorkflowSnapshot> {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO workflow_snapshots (
                id, workflow_id, agent_id, snapshot_type, label, payload, rollback_target
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(id)
        .bind(workflow_id)
        .bind(agent_id)
        .bind(snapshot_type)
        .bind(label)
        .bind(payload)
        .bind(rollback_target)
        .execute(&self.pool)
        .await?;

        let snapshots = self.list_workflow_snapshots(workflow_id).await?;
        snapshots
            .into_iter()
            .find(|snapshot| snapshot.id == id)
            .ok_or_else(|| anyhow::anyhow!("snapshot {id} was not created"))
    }

    pub async fn list_workflow_snapshots(
        &self,
        workflow_id: Uuid,
    ) -> anyhow::Result<Vec<WorkflowSnapshot>> {
        let rows = sqlx::query_as::<_, WorkflowSnapshotRow>(
            r#"
            SELECT id, workflow_id, agent_id, snapshot_type, label, payload, rollback_target, created_at
            FROM workflow_snapshots
            WHERE workflow_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(workflow_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn get_workflow_snapshot(
        &self,
        snapshot_id: Uuid,
    ) -> anyhow::Result<Option<WorkflowSnapshot>> {
        let row = sqlx::query_as::<_, WorkflowSnapshotRow>(
            r#"
            SELECT id, workflow_id, agent_id, snapshot_type, label, payload, rollback_target, created_at
            FROM workflow_snapshots
            WHERE id = $1
            "#,
        )
        .bind(snapshot_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    pub async fn restore_workflow_snapshot(
        &self,
        snapshot_id: Uuid,
    ) -> anyhow::Result<Option<WorkflowDetail>> {
        let Some(snapshot) = self.get_workflow_snapshot(snapshot_id).await? else {
            return Ok(None);
        };
        let payload: WorkflowSnapshotPayload = serde_json::from_value(snapshot.payload.clone())
            .context("invalid workflow snapshot payload")?;

        sqlx::query(
            r#"
            UPDATE workflows
            SET status = $2,
                phase = $3,
                phase_gate_status = $4,
                current_task_id = $5,
                attempt_counter = $6,
                next_action = $7,
                blocked_reason = $8,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(payload.workflow.id)
        .bind(payload.workflow.status)
        .bind(payload.workflow.phase)
        .bind(payload.workflow.phase_gate_status)
        .bind(payload.workflow.current_task_id)
        .bind(payload.workflow.attempt_counter)
        .bind(payload.workflow.next_action)
        .bind(payload.workflow.blocked_reason)
        .execute(&self.pool)
        .await?;

        for agent in payload.agents {
            sqlx::query(
                r#"
                UPDATE workflow_agents
                SET provider = $2,
                    status = $3,
                    current_task = $4,
                    task_fingerprint = $5,
                    worktree_path = $6,
                    sensitivity = $7,
                    approval_required = $8,
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(agent.id)
            .bind(agent.provider.as_str())
            .bind(agent.status)
            .bind(agent.current_task)
            .bind(agent.task_fingerprint)
            .bind(agent.worktree_path)
            .bind(agent.sensitivity)
            .bind(agent.approval_required)
            .execute(&self.pool)
            .await?;
        }

        for terminal in payload.terminals {
            sqlx::query(
                r#"
                UPDATE terminal_sessions
                SET provider = $2,
                    status = $3,
                    command = $4,
                    worktree_path = $5,
                    updated_at = now(),
                    finished_at = CASE WHEN $3 IN ('completed', 'failed', 'blocked') THEN now() ELSE NULL END
                WHERE id = $1
                "#,
            )
            .bind(terminal.id)
            .bind(terminal.provider.as_str())
            .bind(terminal.status)
            .bind(terminal.command)
            .bind(terminal.worktree_path)
            .execute(&self.pool)
            .await?;
        }

        Ok(self.get_workflow_detail(snapshot.workflow_id).await?)
    }

    pub async fn list_mcp_servers(&self) -> anyhow::Result<Vec<McpServer>> {
        let rows = sqlx::query_as::<_, McpServerRow>(
            r#"
            SELECT id, name, command, args, local_only, enabled, allowed_providers, created_at, updated_at
            FROM mcp_servers
            ORDER BY enabled DESC, name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(McpServer::try_from).collect()
    }

    pub async fn list_enabled_mcp_servers(&self) -> anyhow::Result<Vec<McpServer>> {
        let rows = sqlx::query_as::<_, McpServerRow>(
            r#"
            SELECT id, name, command, args, local_only, enabled, allowed_providers, created_at, updated_at
            FROM mcp_servers
            WHERE enabled = true
            ORDER BY name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(McpServer::try_from).collect()
    }

    pub async fn upsert_mcp_server(
        &self,
        name: &str,
        command: &str,
        args: &[String],
        local_only: bool,
        enabled: bool,
        allowed_providers: &[ProviderKind],
    ) -> anyhow::Result<McpServer> {
        let id = Uuid::new_v4();
        let args = serde_json::to_value(args)?;
        let allowed = serde_json::to_value(
            allowed_providers
                .iter()
                .map(|provider| provider.as_str())
                .collect::<Vec<_>>(),
        )?;
        sqlx::query(
            r#"
            INSERT INTO mcp_servers (id, name, command, args, local_only, enabled, allowed_providers)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (name) DO UPDATE
            SET command = EXCLUDED.command,
                args = EXCLUDED.args,
                local_only = EXCLUDED.local_only,
                enabled = EXCLUDED.enabled,
                allowed_providers = EXCLUDED.allowed_providers,
                updated_at = now()
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(command)
        .bind(args)
        .bind(local_only)
        .bind(enabled)
        .bind(allowed)
        .execute(&self.pool)
        .await?;

        let servers = self.list_mcp_servers().await?;
        servers
            .into_iter()
            .find(|server| server.name == name)
            .ok_or_else(|| anyhow::anyhow!("mcp server {name} was not stored"))
    }

    pub async fn set_mcp_server_enabled(
        &self,
        server_id: Uuid,
        enabled: bool,
    ) -> anyhow::Result<Option<McpServer>> {
        sqlx::query(
            r#"
            UPDATE mcp_servers
            SET enabled = $2,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(server_id)
        .bind(enabled)
        .execute(&self.pool)
        .await?;

        let servers = self.list_mcp_servers().await?;
        Ok(servers.into_iter().find(|server| server.id == server_id))
    }

    pub async fn list_llama_cpp_models(&self) -> anyhow::Result<Vec<LlamaCppModel>> {
        let rows = sqlx::query_as::<_, LlamaCppModelRow>(
            r#"
            SELECT id, alias, file_path, context_window, quantization, enabled, created_at, updated_at
            FROM llama_cpp_models
            ORDER BY enabled DESC, alias ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn upsert_llama_cpp_model(
        &self,
        alias: &str,
        file_path: &str,
        context_window: Option<i32>,
        quantization: Option<&str>,
        enabled: bool,
    ) -> anyhow::Result<LlamaCppModel> {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO llama_cpp_models (id, alias, file_path, context_window, quantization, enabled)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (alias) DO UPDATE
            SET file_path = EXCLUDED.file_path,
                context_window = EXCLUDED.context_window,
                quantization = EXCLUDED.quantization,
                enabled = EXCLUDED.enabled,
                updated_at = now()
            "#,
        )
        .bind(id)
        .bind(alias)
        .bind(file_path)
        .bind(context_window)
        .bind(quantization)
        .bind(enabled)
        .execute(&self.pool)
        .await?;

        let models = self.list_llama_cpp_models().await?;
        models
            .into_iter()
            .find(|model| model.alias == alias)
            .ok_or_else(|| anyhow::anyhow!("llama.cpp model {alias} was not stored"))
    }

    pub async fn set_llama_cpp_model_enabled(
        &self,
        model_id: Uuid,
        enabled: bool,
    ) -> anyhow::Result<Option<LlamaCppModel>> {
        sqlx::query(
            r#"
            UPDATE llama_cpp_models
            SET enabled = $2,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(model_id)
        .bind(enabled)
        .execute(&self.pool)
        .await?;

        let models = self.list_llama_cpp_models().await?;
        Ok(models.into_iter().find(|model| model.id == model_id))
    }

    async fn upsert_workflow_template(&self, template: &WorkflowTemplate) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO workflow_templates (
                id, template_key, name, description, phases, default_agent_roles
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (template_key) DO UPDATE
            SET name = EXCLUDED.name,
                description = EXCLUDED.description,
                phases = EXCLUDED.phases,
                default_agent_roles = EXCLUDED.default_agent_roles,
                updated_at = now()
            "#,
        )
        .bind(template.id)
        .bind(&template.template_key)
        .bind(&template.name)
        .bind(&template.description)
        .bind(serde_json::to_value(&template.phases)?)
        .bind(serde_json::to_value(&template.default_agent_roles)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_message(
        &self,
        chat_id: Uuid,
        role: &str,
        content: &str,
        source_run_id: Option<Uuid>,
    ) -> anyhow::Result<ChatMessage> {
        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO messages (id, session_id, role, content, source_run_id)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(id)
        .bind(chat_id)
        .bind(role)
        .bind(content)
        .bind(source_run_id)
        .execute(&self.pool)
        .await?;

        Ok(ChatMessage {
            id,
            session_id: chat_id,
            role: role.to_string(),
            content: content.to_string(),
            created_at: Utc::now(),
            source_run_id,
            usage: None,
        })
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct ChatSummary {
    pub id: Uuid,
    pub provider: ProviderKind,
    pub title: String,
    pub provider_session_ref: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub last_model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub session_id: Uuid,
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub source_run_id: Option<Uuid>,
    pub usage: Option<LlmUsage>,
}

#[derive(Debug, Serialize, Clone)]
pub struct RunSummary {
    pub id: Uuid,
    pub session_id: Uuid,
    pub provider: ProviderKind,
    pub status: RunStatus,
    pub command: String,
    pub exit_code: Option<i32>,
    pub provider_session_ref: Option<String>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AuthSession {
    pub id: Uuid,
    pub provider: ProviderKind,
    pub action: String,
    pub status: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub last_output: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DailyUsage {
    pub provider: ProviderKind,
    pub day: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowSummary {
    pub id: Uuid,
    pub title: String,
    pub objective: String,
    pub coordinator_provider: ProviderKind,
    pub sensitivity: String,
    pub status: String,
    pub template_key: String,
    pub phase: String,
    pub phase_gate_status: String,
    pub current_task_id: Option<Uuid>,
    pub attempt_counter: i32,
    pub next_action: Option<String>,
    pub blocked_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowAgent {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub name: String,
    pub role: String,
    pub provider: ProviderKind,
    pub status: String,
    pub current_task: String,
    pub task_fingerprint: String,
    pub dependency_ids: Vec<Uuid>,
    pub worktree_path: Option<String>,
    pub sensitivity: String,
    pub approval_required: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TerminalSession {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub agent_id: Uuid,
    pub title: String,
    pub provider: ProviderKind,
    pub status: String,
    pub command: Option<String>,
    pub worktree_path: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TerminalOutput {
    pub terminal_session_id: Uuid,
    pub sequence: i64,
    pub text: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowArtifact {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub title: String,
    pub kind: String,
    pub content: String,
    pub fingerprint: String,
    pub sensitivity: String,
    pub reusable: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApprovalGate {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub gate_type: String,
    pub target_provider: Option<ProviderKind>,
    pub status: String,
    pub reason: String,
    pub requested_context: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowDetail {
    pub workflow: WorkflowSummary,
    pub agents: Vec<WorkflowAgent>,
    pub terminals: Vec<TerminalSession>,
    pub approvals: Vec<ApprovalGate>,
    pub artifacts: Vec<WorkflowArtifact>,
    pub handoffs: Vec<WorkflowHandoff>,
    pub qa_verdicts: Vec<WorkflowQaVerdict>,
    pub release_verdicts: Vec<WorkflowReleaseVerdict>,
    pub evidence: Vec<WorkflowEvidenceRecord>,
    pub snapshots: Vec<WorkflowSnapshot>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowTemplate {
    pub id: Uuid,
    pub template_key: String,
    pub name: String,
    pub description: String,
    pub phases: Vec<String>,
    pub default_agent_roles: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowHandoff {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub from_agent_id: Option<Uuid>,
    pub to_agent_id: Option<Uuid>,
    pub phase: String,
    pub handoff_type: String,
    pub task_ref: String,
    pub priority: String,
    pub context_summary: String,
    pub relevant_artifact_ids: Vec<Uuid>,
    pub dependencies: Vec<String>,
    pub constraints: Vec<String>,
    pub deliverable_request: String,
    pub acceptance_criteria: Vec<String>,
    pub evidence_required: Vec<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowQaVerdict {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub phase: String,
    pub verdict: String,
    pub summary: String,
    pub findings: Vec<String>,
    pub evidence_artifact_ids: Vec<Uuid>,
    pub attempt_number: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowReleaseVerdict {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub phase: String,
    pub verdict: String,
    pub summary: String,
    pub findings: Vec<String>,
    pub evidence_artifact_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowEvidenceRecord {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub actor_type: String,
    pub actor_id: Option<Uuid>,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub prev_hash: Option<String>,
    pub record_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkflowSnapshot {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub snapshot_type: String,
    pub label: String,
    pub payload: serde_json::Value,
    pub rollback_target: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Clone)]
pub struct McpServer {
    pub id: Uuid,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub local_only: bool,
    pub enabled: bool,
    pub allowed_providers: Vec<ProviderKind>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Clone)]
pub struct LlamaCppModel {
    pub id: Uuid,
    pub alias: String,
    pub file_path: String,
    pub context_window: Option<i32>,
    pub quantization: Option<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct ChatSummaryRow {
    id: Uuid,
    provider: String,
    title: String,
    provider_session_ref: Option<String>,
    created_at: DateTime<Utc>,
    last_message_at: Option<DateTime<Utc>>,
    last_model: Option<String>,
}

#[derive(FromRow)]
struct ChatMessageRow {
    id: Uuid,
    session_id: Uuid,
    role: String,
    content: String,
    created_at: DateTime<Utc>,
    source_run_id: Option<Uuid>,
    model: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    estimated_cost_usd: Option<f64>,
}

#[derive(FromRow)]
struct EventRow {
    provider: String,
    sequence: i64,
    event_kind: String,
    raw_event: serde_json::Value,
    text: Option<String>,
    usage: Option<serde_json::Value>,
    provider_session_ref: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct DailyUsageRow {
    provider: String,
    day: String,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
}

#[derive(FromRow)]
struct WorkflowRow {
    id: Uuid,
    title: String,
    objective: String,
    coordinator_provider: String,
    sensitivity: String,
    status: String,
    template_key: String,
    phase: String,
    phase_gate_status: String,
    current_task_id: Option<Uuid>,
    attempt_counter: i32,
    next_action: Option<String>,
    blocked_reason: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct WorkflowAgentRow {
    id: Uuid,
    workflow_id: Uuid,
    name: String,
    role: String,
    provider: String,
    status: String,
    current_task: String,
    task_fingerprint: String,
    dependency_ids: serde_json::Value,
    worktree_path: Option<String>,
    sensitivity: String,
    approval_required: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct TerminalSessionRow {
    id: Uuid,
    workflow_id: Uuid,
    agent_id: Uuid,
    title: String,
    provider: String,
    status: String,
    command: Option<String>,
    worktree_path: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

#[derive(FromRow)]
struct TerminalEntryRow {
    terminal_session_id: Uuid,
    sequence: i64,
    text: String,
    created_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct WorkflowArtifactRow {
    id: Uuid,
    workflow_id: Uuid,
    agent_id: Option<Uuid>,
    title: String,
    kind: String,
    content: String,
    fingerprint: String,
    sensitivity: String,
    reusable: bool,
    created_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct ApprovalGateRow {
    id: Uuid,
    workflow_id: Uuid,
    agent_id: Option<Uuid>,
    gate_type: String,
    target_provider: Option<String>,
    status: String,
    reason: String,
    requested_context: serde_json::Value,
    created_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
}

#[derive(FromRow)]
struct WorkflowTemplateRow {
    id: Uuid,
    template_key: String,
    name: String,
    description: String,
    phases: serde_json::Value,
    default_agent_roles: serde_json::Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct WorkflowHandoffRow {
    id: Uuid,
    workflow_id: Uuid,
    from_agent_id: Option<Uuid>,
    to_agent_id: Option<Uuid>,
    phase: String,
    handoff_type: String,
    task_ref: String,
    priority: String,
    context_summary: String,
    relevant_artifact_ids: serde_json::Value,
    dependencies: serde_json::Value,
    constraints: serde_json::Value,
    deliverable_request: String,
    acceptance_criteria: serde_json::Value,
    evidence_required: serde_json::Value,
    status: String,
    created_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
}

#[derive(FromRow)]
struct WorkflowQaVerdictRow {
    id: Uuid,
    workflow_id: Uuid,
    agent_id: Option<Uuid>,
    phase: String,
    verdict: String,
    summary: String,
    findings: serde_json::Value,
    evidence_artifact_ids: serde_json::Value,
    attempt_number: i32,
    created_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct WorkflowReleaseVerdictRow {
    id: Uuid,
    workflow_id: Uuid,
    agent_id: Option<Uuid>,
    phase: String,
    verdict: String,
    summary: String,
    findings: serde_json::Value,
    evidence_artifact_ids: serde_json::Value,
    created_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct WorkflowEvidenceRecordRow {
    id: Uuid,
    workflow_id: Uuid,
    actor_type: String,
    actor_id: Option<Uuid>,
    event_type: String,
    payload: serde_json::Value,
    prev_hash: Option<String>,
    record_hash: String,
    created_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct WorkflowSnapshotRow {
    id: Uuid,
    workflow_id: Uuid,
    agent_id: Option<Uuid>,
    snapshot_type: String,
    label: String,
    payload: serde_json::Value,
    rollback_target: bool,
    created_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct McpServerRow {
    id: Uuid,
    name: String,
    command: String,
    args: serde_json::Value,
    local_only: bool,
    enabled: bool,
    allowed_providers: serde_json::Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct LlamaCppModelRow {
    id: Uuid,
    alias: String,
    file_path: String,
    context_window: Option<i32>,
    quantization: Option<String>,
    enabled: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ChatSummaryRow> for ChatSummary {
    type Error = anyhow::Error;

    fn try_from(value: ChatSummaryRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            provider: parse_provider(&value.provider)?,
            title: value.title,
            provider_session_ref: value.provider_session_ref,
            created_at: value.created_at,
            last_message_at: value.last_message_at,
            last_model: value.last_model,
        })
    }
}

impl From<ChatMessageRow> for ChatMessage {
    fn from(value: ChatMessageRow) -> Self {
        let usage_present = value.model.is_some()
            || value.input_tokens.is_some()
            || value.output_tokens.is_some()
            || value.total_tokens.is_some()
            || value.estimated_cost_usd.is_some();
        Self {
            id: value.id,
            session_id: value.session_id,
            role: value.role,
            content: value.content,
            created_at: value.created_at,
            source_run_id: value.source_run_id,
            usage: usage_present.then_some(LlmUsage {
                model: value.model,
                input_tokens: value.input_tokens,
                output_tokens: value.output_tokens,
                total_tokens: value.total_tokens,
                estimated_cost_usd: value.estimated_cost_usd,
            }),
        }
    }
}

impl TryFrom<EventRow> for NormalizedEvent {
    type Error = anyhow::Error;

    fn try_from(value: EventRow) -> Result<Self, Self::Error> {
        Ok(Self {
            event_kind: parse_event_kind(&value.event_kind)?,
            provider: parse_provider(&value.provider)?,
            sequence: value.sequence,
            raw: value.raw_event,
            text: value.text,
            usage: value
                .usage
                .map(serde_json::from_value)
                .transpose()
                .context("invalid llm usage json")?,
            provider_session_ref: value.provider_session_ref,
            created_at: value.created_at,
        })
    }
}

impl TryFrom<DailyUsageRow> for DailyUsage {
    type Error = anyhow::Error;

    fn try_from(value: DailyUsageRow) -> Result<Self, Self::Error> {
        Ok(Self {
            provider: parse_provider(&value.provider)?,
            day: value.day,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            total_tokens: value.total_tokens,
        })
    }
}

impl TryFrom<WorkflowRow> for WorkflowSummary {
    type Error = anyhow::Error;

    fn try_from(value: WorkflowRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            title: value.title,
            objective: value.objective,
            coordinator_provider: parse_provider(&value.coordinator_provider)?,
            sensitivity: value.sensitivity,
            status: value.status,
            template_key: value.template_key,
            phase: value.phase,
            phase_gate_status: value.phase_gate_status,
            current_task_id: value.current_task_id,
            attempt_counter: value.attempt_counter,
            next_action: value.next_action,
            blocked_reason: value.blocked_reason,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

impl TryFrom<WorkflowAgentRow> for WorkflowAgent {
    type Error = anyhow::Error;

    fn try_from(value: WorkflowAgentRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            workflow_id: value.workflow_id,
            name: value.name,
            role: value.role,
            provider: parse_provider(&value.provider)?,
            status: value.status,
            current_task: value.current_task,
            task_fingerprint: value.task_fingerprint,
            dependency_ids: serde_json::from_value(value.dependency_ids)
                .context("invalid workflow dependency ids")?,
            worktree_path: value.worktree_path,
            sensitivity: value.sensitivity,
            approval_required: value.approval_required,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

impl TryFrom<TerminalSessionRow> for TerminalSession {
    type Error = anyhow::Error;

    fn try_from(value: TerminalSessionRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            workflow_id: value.workflow_id,
            agent_id: value.agent_id,
            title: value.title,
            provider: parse_provider(&value.provider)?,
            status: value.status,
            command: value.command,
            worktree_path: value.worktree_path,
            created_at: value.created_at,
            updated_at: value.updated_at,
            finished_at: value.finished_at,
        })
    }
}

impl From<TerminalEntryRow> for TerminalOutput {
    fn from(value: TerminalEntryRow) -> Self {
        Self {
            terminal_session_id: value.terminal_session_id,
            sequence: value.sequence,
            text: value.text,
            created_at: value.created_at,
        }
    }
}

impl From<WorkflowArtifactRow> for WorkflowArtifact {
    fn from(value: WorkflowArtifactRow) -> Self {
        Self {
            id: value.id,
            workflow_id: value.workflow_id,
            agent_id: value.agent_id,
            title: value.title,
            kind: value.kind,
            content: value.content,
            fingerprint: value.fingerprint,
            sensitivity: value.sensitivity,
            reusable: value.reusable,
            created_at: value.created_at,
        }
    }
}

impl TryFrom<ApprovalGateRow> for ApprovalGate {
    type Error = anyhow::Error;

    fn try_from(value: ApprovalGateRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            workflow_id: value.workflow_id,
            agent_id: value.agent_id,
            gate_type: value.gate_type,
            target_provider: value
                .target_provider
                .as_deref()
                .map(parse_provider)
                .transpose()?,
            status: value.status,
            reason: value.reason,
            requested_context: value.requested_context,
            created_at: value.created_at,
            resolved_at: value.resolved_at,
        })
    }
}

impl TryFrom<WorkflowTemplateRow> for WorkflowTemplate {
    type Error = anyhow::Error;

    fn try_from(value: WorkflowTemplateRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            template_key: value.template_key,
            name: value.name,
            description: value.description,
            phases: serde_json::from_value(value.phases).context("invalid template phases")?,
            default_agent_roles: serde_json::from_value(value.default_agent_roles)
                .context("invalid template default_agent_roles")?,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

impl TryFrom<WorkflowHandoffRow> for WorkflowHandoff {
    type Error = anyhow::Error;

    fn try_from(value: WorkflowHandoffRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            workflow_id: value.workflow_id,
            from_agent_id: value.from_agent_id,
            to_agent_id: value.to_agent_id,
            phase: value.phase,
            handoff_type: value.handoff_type,
            task_ref: value.task_ref,
            priority: value.priority,
            context_summary: value.context_summary,
            relevant_artifact_ids: serde_json::from_value(value.relevant_artifact_ids)
                .context("invalid handoff artifact ids")?,
            dependencies: serde_json::from_value(value.dependencies)
                .context("invalid handoff dependencies")?,
            constraints: serde_json::from_value(value.constraints)
                .context("invalid handoff constraints")?,
            deliverable_request: value.deliverable_request,
            acceptance_criteria: serde_json::from_value(value.acceptance_criteria)
                .context("invalid handoff acceptance_criteria")?,
            evidence_required: serde_json::from_value(value.evidence_required)
                .context("invalid handoff evidence_required")?,
            status: value.status,
            created_at: value.created_at,
            resolved_at: value.resolved_at,
        })
    }
}

impl TryFrom<WorkflowQaVerdictRow> for WorkflowQaVerdict {
    type Error = anyhow::Error;

    fn try_from(value: WorkflowQaVerdictRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            workflow_id: value.workflow_id,
            agent_id: value.agent_id,
            phase: value.phase,
            verdict: value.verdict,
            summary: value.summary,
            findings: serde_json::from_value(value.findings).context("invalid qa findings")?,
            evidence_artifact_ids: serde_json::from_value(value.evidence_artifact_ids)
                .context("invalid qa evidence artifact ids")?,
            attempt_number: value.attempt_number,
            created_at: value.created_at,
        })
    }
}

impl TryFrom<WorkflowReleaseVerdictRow> for WorkflowReleaseVerdict {
    type Error = anyhow::Error;

    fn try_from(value: WorkflowReleaseVerdictRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            workflow_id: value.workflow_id,
            agent_id: value.agent_id,
            phase: value.phase,
            verdict: value.verdict,
            summary: value.summary,
            findings: serde_json::from_value(value.findings).context("invalid release findings")?,
            evidence_artifact_ids: serde_json::from_value(value.evidence_artifact_ids)
                .context("invalid release evidence artifact ids")?,
            created_at: value.created_at,
        })
    }
}

impl From<WorkflowEvidenceRecordRow> for WorkflowEvidenceRecord {
    fn from(value: WorkflowEvidenceRecordRow) -> Self {
        Self {
            id: value.id,
            workflow_id: value.workflow_id,
            actor_type: value.actor_type,
            actor_id: value.actor_id,
            event_type: value.event_type,
            payload: value.payload,
            prev_hash: value.prev_hash,
            record_hash: value.record_hash,
            created_at: value.created_at,
        }
    }
}

impl From<WorkflowSnapshotRow> for WorkflowSnapshot {
    fn from(value: WorkflowSnapshotRow) -> Self {
        Self {
            id: value.id,
            workflow_id: value.workflow_id,
            agent_id: value.agent_id,
            snapshot_type: value.snapshot_type,
            label: value.label,
            payload: value.payload,
            rollback_target: value.rollback_target,
            created_at: value.created_at,
        }
    }
}

impl TryFrom<McpServerRow> for McpServer {
    type Error = anyhow::Error;

    fn try_from(value: McpServerRow) -> Result<Self, Self::Error> {
        let allowed = serde_json::from_value::<Vec<String>>(value.allowed_providers)
            .context("invalid mcp allowed_providers")?
            .into_iter()
            .map(|provider| parse_provider(&provider))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            id: value.id,
            name: value.name,
            command: value.command,
            args: serde_json::from_value(value.args).context("invalid mcp args")?,
            local_only: value.local_only,
            enabled: value.enabled,
            allowed_providers: allowed,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

impl From<LlamaCppModelRow> for LlamaCppModel {
    fn from(value: LlamaCppModelRow) -> Self {
        Self {
            id: value.id,
            alias: value.alias,
            file_path: value.file_path,
            context_window: value.context_window,
            quantization: value.quantization,
            enabled: value.enabled,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

fn parse_provider(value: &str) -> anyhow::Result<ProviderKind> {
    match value {
        "codex" => Ok(ProviderKind::Codex),
        "claude" => Ok(ProviderKind::Claude),
        "ollama" => Ok(ProviderKind::Ollama),
        "llama_cpp" => Ok(ProviderKind::LlamaCpp),
        other => anyhow::bail!("unknown provider {other}"),
    }
}

fn parse_event_kind(value: &str) -> anyhow::Result<EventKind> {
    match value {
        "run_started" => Ok(EventKind::RunStarted),
        "assistant_delta" => Ok(EventKind::AssistantDelta),
        "assistant_final" => Ok(EventKind::AssistantFinal),
        "usage_updated" => Ok(EventKind::UsageUpdated),
        "provider_session_bound" => Ok(EventKind::ProviderSessionBound),
        "auth_output" => Ok(EventKind::AuthOutput),
        "auth_url" => Ok(EventKind::AuthUrl),
        "stderr" => Ok(EventKind::StdErr),
        "run_completed" => Ok(EventKind::RunCompleted),
        "run_failed" => Ok(EventKind::RunFailed),
        other => anyhow::bail!("unknown event kind {other}"),
    }
}

fn event_kind_as_str(value: EventKind) -> &'static str {
    match value {
        EventKind::RunStarted => "run_started",
        EventKind::AssistantDelta => "assistant_delta",
        EventKind::AssistantFinal => "assistant_final",
        EventKind::UsageUpdated => "usage_updated",
        EventKind::ProviderSessionBound => "provider_session_bound",
        EventKind::AuthOutput => "auth_output",
        EventKind::AuthUrl => "auth_url",
        EventKind::StdErr => "stderr",
        EventKind::RunCompleted => "run_completed",
        EventKind::RunFailed => "run_failed",
    }
}

fn run_status_as_str(value: RunStatus) -> &'static str {
    match value {
        RunStatus::Pending => "pending",
        RunStatus::Running => "running",
        RunStatus::Completed => "completed",
        RunStatus::Failed => "failed",
        RunStatus::Cancelled => "cancelled",
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkflowSnapshotPayload {
    workflow: WorkflowSummary,
    agents: Vec<WorkflowAgent>,
    terminals: Vec<TerminalSession>,
}

fn hash_evidence_record(
    prev_hash: &Option<String>,
    actor_type: &str,
    actor_id: Option<Uuid>,
    event_type: &str,
    payload: &serde_json::Value,
    created_at: DateTime<Utc>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_deref().unwrap_or(""));
    hasher.update(actor_type);
    hasher.update(actor_id.map(|id| id.to_string()).unwrap_or_default());
    hasher.update(event_type);
    hasher.update(payload.to_string());
    hasher.update(created_at.to_rfc3339());
    format!("{:x}", hasher.finalize())
}
