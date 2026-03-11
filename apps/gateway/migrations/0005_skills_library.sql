CREATE TABLE skills (
    id UUID PRIMARY KEY,
    tenant_key TEXT NOT NULL DEFAULT 'default',
    slug TEXT NOT NULL,
    name TEXT NOT NULL,
    skill_type TEXT NOT NULL,
    description TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    owner TEXT NOT NULL,
    visibility TEXT NOT NULL DEFAULT 'private',
    tags JSONB NOT NULL DEFAULT '[]'::jsonb,
    allowed_sensitivity_levels JSONB NOT NULL DEFAULT '["internal"]'::jsonb,
    provider_exposure TEXT NOT NULL,
    source_kind TEXT NOT NULL DEFAULT 'manual',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_skills_skill_type CHECK (
        skill_type IN ('library', 'agent-context', 'cli', 'provider', 'policy')
    ),
    CONSTRAINT chk_skills_status CHECK (
        status IN ('active', 'archived')
    ),
    CONSTRAINT chk_skills_provider_exposure CHECK (
        provider_exposure IN ('local_only', 'agent_context_only', 'provider_allowed')
    ),
    UNIQUE (tenant_key, slug)
);

CREATE TABLE skill_versions (
    id UUID PRIMARY KEY,
    skill_id UUID NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    version INT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    body JSONB NOT NULL DEFAULT '{}'::jsonb,
    summary TEXT NOT NULL,
    examples JSONB NOT NULL DEFAULT '[]'::jsonb,
    constraints JSONB NOT NULL DEFAULT '[]'::jsonb,
    review_notes TEXT,
    created_by TEXT NOT NULL,
    approved_by TEXT,
    published_by TEXT,
    source_ref TEXT,
    dataset_pack_key TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    approved_at TIMESTAMPTZ,
    published_at TIMESTAMPTZ,
    CONSTRAINT chk_skill_versions_status CHECK (
        status IN ('draft', 'review', 'approved', 'published', 'retired')
    ),
    UNIQUE (skill_id, version)
);

CREATE TABLE skill_reviews (
    id UUID PRIMARY KEY,
    skill_version_id UUID NOT NULL REFERENCES skill_versions(id) ON DELETE CASCADE,
    action TEXT NOT NULL,
    actor_role TEXT NOT NULL,
    actor_name TEXT NOT NULL,
    comment TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_skill_reviews_action CHECK (
        action IN ('draft_created', 'submitted_for_review', 'approved', 'published')
    ),
    CONSTRAINT chk_skill_reviews_actor_role CHECK (
        actor_role IN ('author', 'reviewer', 'publisher')
    )
);

CREATE INDEX idx_skills_type_status_updated
    ON skills(skill_type, status, updated_at DESC);
CREATE INDEX idx_skill_versions_skill_version
    ON skill_versions(skill_id, version DESC);
CREATE INDEX idx_skill_reviews_version_created
    ON skill_reviews(skill_version_id, created_at DESC);
