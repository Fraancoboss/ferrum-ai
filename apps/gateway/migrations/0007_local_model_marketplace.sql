CREATE TABLE IF NOT EXISTS hardware_profiles (
    id UUID PRIMARY KEY,
    profile_kind TEXT NOT NULL,
    source_key TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (profile_kind, source_key)
);

CREATE INDEX IF NOT EXISTS idx_hardware_profiles_kind_updated
    ON hardware_profiles (profile_kind, updated_at DESC);

CREATE TABLE IF NOT EXISTS model_install_jobs (
    id UUID PRIMARY KEY,
    actor_name TEXT NOT NULL,
    source_app TEXT NOT NULL,
    source_channel TEXT NOT NULL,
    runtime_target TEXT NOT NULL,
    catalog_key TEXT,
    source_ref TEXT,
    checksum_expected TEXT,
    checksum_actual TEXT,
    destination_ref TEXT,
    status TEXT NOT NULL,
    progress_percent INTEGER NOT NULL DEFAULT 0,
    detail TEXT,
    error_text TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_model_install_jobs_runtime_created
    ON model_install_jobs (runtime_target, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_model_install_jobs_status_updated
    ON model_install_jobs (status, updated_at DESC);

CREATE TABLE IF NOT EXISTS model_usage_attribution (
    id UUID PRIMARY KEY,
    actor_name TEXT NOT NULL,
    source_app TEXT NOT NULL,
    source_channel TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens BIGINT,
    output_tokens BIGINT,
    total_tokens BIGINT,
    workflow_id UUID,
    chat_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_model_usage_attribution_model_created
    ON model_usage_attribution (provider, model, created_at DESC);
