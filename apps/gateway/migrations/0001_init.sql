CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE chat_sessions (
    id UUID PRIMARY KEY,
    provider TEXT NOT NULL,
    title TEXT NOT NULL,
    provider_session_ref TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE runs (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    command TEXT NOT NULL,
    status TEXT NOT NULL,
    exit_code INT,
    stdout_final TEXT,
    stderr_final TEXT,
    provider_session_ref TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

CREATE TABLE messages (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    source_run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE run_events (
    id UUID PRIMARY KEY,
    run_id UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    event_kind TEXT NOT NULL,
    raw_event JSONB NOT NULL,
    text TEXT,
    usage JSONB,
    provider_session_ref TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE llm_usage (
    id UUID PRIMARY KEY,
    run_id UUID NOT NULL UNIQUE REFERENCES runs(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    model TEXT,
    input_tokens BIGINT,
    output_tokens BIGINT,
    total_tokens BIGINT,
    estimated_cost_usd DOUBLE PRECISION,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE provider_auth_sessions (
    id UUID PRIMARY KEY,
    provider TEXT NOT NULL,
    action TEXT NOT NULL,
    command TEXT NOT NULL,
    status TEXT NOT NULL,
    exit_code INT,
    last_output TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

CREATE TABLE provider_auth_events (
    id UUID PRIMARY KEY,
    auth_session_id UUID NOT NULL REFERENCES provider_auth_sessions(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    event_kind TEXT NOT NULL,
    raw_event JSONB NOT NULL,
    text TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE documents (
    id UUID PRIMARY KEY,
    session_id UUID REFERENCES chat_sessions(id) ON DELETE SET NULL,
    source_run_id UUID REFERENCES runs(id) ON DELETE SET NULL,
    content TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE chunks (
    id UUID PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    chunk_index INT NOT NULL,
    content TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    embedding vector(1536),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_messages_session_id ON messages(session_id, created_at);
CREATE INDEX idx_runs_session_id ON runs(session_id, created_at);
CREATE INDEX idx_run_events_run_id ON run_events(run_id, sequence);
CREATE INDEX idx_auth_events_auth_session_id ON provider_auth_events(auth_session_id, sequence);
CREATE INDEX idx_llm_usage_created_at ON llm_usage(created_at);
CREATE INDEX idx_documents_session_id ON documents(session_id);
CREATE INDEX idx_chunks_document_id ON chunks(document_id, chunk_index);

