CREATE TABLE mcp_servers (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    command TEXT NOT NULL,
    args JSONB NOT NULL DEFAULT '[]'::jsonb,
    local_only BOOLEAN NOT NULL DEFAULT true,
    enabled BOOLEAN NOT NULL DEFAULT true,
    allowed_providers JSONB NOT NULL DEFAULT '["ollama","llama_cpp"]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE llama_cpp_models (
    id UUID PRIMARY KEY,
    alias TEXT NOT NULL UNIQUE,
    file_path TEXT NOT NULL UNIQUE,
    context_window INT,
    quantization TEXT,
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_mcp_servers_enabled ON mcp_servers(enabled, created_at);
CREATE INDEX idx_llama_cpp_models_enabled ON llama_cpp_models(enabled, created_at);
