CREATE TABLE workflows (
    id UUID PRIMARY KEY,
    title TEXT NOT NULL,
    objective TEXT NOT NULL,
    coordinator_provider TEXT NOT NULL,
    sensitivity TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE workflow_agents (
    id UUID PRIMARY KEY,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    role TEXT NOT NULL,
    provider TEXT NOT NULL,
    status TEXT NOT NULL,
    current_task TEXT NOT NULL,
    task_fingerprint TEXT NOT NULL,
    dependency_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    worktree_path TEXT,
    sensitivity TEXT NOT NULL,
    approval_required BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workflow_id, task_fingerprint)
);

CREATE TABLE terminal_sessions (
    id UUID PRIMARY KEY,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL UNIQUE REFERENCES workflow_agents(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    provider TEXT NOT NULL,
    status TEXT NOT NULL,
    command TEXT,
    worktree_path TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

CREATE TABLE terminal_entries (
    id UUID PRIMARY KEY,
    terminal_session_id UUID NOT NULL REFERENCES terminal_sessions(id) ON DELETE CASCADE,
    sequence BIGINT NOT NULL,
    text TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE workflow_artifacts (
    id UUID PRIMARY KEY,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    agent_id UUID REFERENCES workflow_agents(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    kind TEXT NOT NULL,
    content TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    sensitivity TEXT NOT NULL,
    reusable BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workflow_id, fingerprint)
);

CREATE TABLE workflow_approvals (
    id UUID PRIMARY KEY,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    agent_id UUID REFERENCES workflow_agents(id) ON DELETE SET NULL,
    gate_type TEXT NOT NULL,
    target_provider TEXT,
    status TEXT NOT NULL,
    reason TEXT NOT NULL,
    requested_context JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ
);

CREATE INDEX idx_workflow_agents_workflow_id ON workflow_agents(workflow_id, created_at);
CREATE INDEX idx_terminal_sessions_workflow_id ON terminal_sessions(workflow_id, created_at);
CREATE INDEX idx_terminal_entries_terminal_session_id ON terminal_entries(terminal_session_id, sequence);
CREATE INDEX idx_workflow_artifacts_workflow_id ON workflow_artifacts(workflow_id, created_at);
CREATE INDEX idx_workflow_approvals_workflow_id ON workflow_approvals(workflow_id, created_at);
