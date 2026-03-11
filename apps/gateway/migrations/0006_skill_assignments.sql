CREATE TABLE skill_assignments (
    id UUID PRIMARY KEY,
    skill_version_id UUID NOT NULL REFERENCES skill_versions(id) ON DELETE CASCADE,
    target_type TEXT NOT NULL,
    target_key TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT skill_assignments_target_type_check CHECK (
        target_type IN ('workflow_template', 'agent_role', 'provider')
    ),
    CONSTRAINT skill_assignments_unique UNIQUE (skill_version_id, target_type, target_key)
);

CREATE INDEX idx_skill_assignments_version_target
    ON skill_assignments(skill_version_id, target_type, target_key);
