use anyhow::Context;
use chrono::{DateTime, Utc};
use orchestrator_core::{EventKind, LlmUsage, NormalizedEvent, ProviderKind, RunStatus};
use serde::Serialize;
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

fn parse_provider(value: &str) -> anyhow::Result<ProviderKind> {
    match value {
        "codex" => Ok(ProviderKind::Codex),
        "claude" => Ok(ProviderKind::Claude),
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
