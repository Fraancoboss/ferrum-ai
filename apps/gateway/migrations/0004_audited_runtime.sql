ALTER TABLE workflows
    ADD COLUMN IF NOT EXISTS template_key TEXT NOT NULL DEFAULT 'engineering_pipeline',
    ADD COLUMN IF NOT EXISTS phase TEXT NOT NULL DEFAULT 'planning',
    ADD COLUMN IF NOT EXISTS phase_gate_status TEXT NOT NULL DEFAULT 'open',
    ADD COLUMN IF NOT EXISTS current_task_id UUID,
    ADD COLUMN IF NOT EXISTS attempt_counter INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS next_action TEXT,
    ADD COLUMN IF NOT EXISTS blocked_reason TEXT;

CREATE TABLE workflow_templates (
    id UUID PRIMARY KEY,
    template_key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    phases JSONB NOT NULL DEFAULT '[]'::jsonb,
    default_agent_roles JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE workflow_handoffs (
    id UUID PRIMARY KEY,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    from_agent_id UUID REFERENCES workflow_agents(id) ON DELETE SET NULL,
    to_agent_id UUID REFERENCES workflow_agents(id) ON DELETE SET NULL,
    phase TEXT NOT NULL,
    handoff_type TEXT NOT NULL,
    task_ref TEXT NOT NULL,
    priority TEXT NOT NULL,
    context_summary TEXT NOT NULL,
    relevant_artifact_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    dependencies JSONB NOT NULL DEFAULT '[]'::jsonb,
    constraints JSONB NOT NULL DEFAULT '[]'::jsonb,
    deliverable_request TEXT NOT NULL,
    acceptance_criteria JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_required JSONB NOT NULL DEFAULT '[]'::jsonb,
    status TEXT NOT NULL DEFAULT 'open',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ
);

CREATE TABLE workflow_qa_verdicts (
    id UUID PRIMARY KEY,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    agent_id UUID REFERENCES workflow_agents(id) ON DELETE SET NULL,
    phase TEXT NOT NULL,
    verdict TEXT NOT NULL,
    summary TEXT NOT NULL,
    findings JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_artifact_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    attempt_number INT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE workflow_release_verdicts (
    id UUID PRIMARY KEY,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    agent_id UUID REFERENCES workflow_agents(id) ON DELETE SET NULL,
    phase TEXT NOT NULL,
    verdict TEXT NOT NULL,
    summary TEXT NOT NULL,
    findings JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_artifact_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE workflow_evidence_records (
    id UUID PRIMARY KEY,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    actor_type TEXT NOT NULL,
    actor_id UUID,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    prev_hash TEXT,
    record_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE workflow_snapshots (
    id UUID PRIMARY KEY,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    agent_id UUID REFERENCES workflow_agents(id) ON DELETE SET NULL,
    snapshot_type TEXT NOT NULL,
    label TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    rollback_target BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_workflow_templates_key ON workflow_templates(template_key);
CREATE INDEX idx_workflow_handoffs_workflow_id ON workflow_handoffs(workflow_id, created_at);
CREATE INDEX idx_workflow_qa_verdicts_workflow_id ON workflow_qa_verdicts(workflow_id, created_at);
CREATE INDEX idx_workflow_release_verdicts_workflow_id ON workflow_release_verdicts(workflow_id, created_at);
CREATE INDEX idx_workflow_evidence_records_workflow_id ON workflow_evidence_records(workflow_id, created_at);
CREATE INDEX idx_workflow_snapshots_workflow_id ON workflow_snapshots(workflow_id, created_at);
