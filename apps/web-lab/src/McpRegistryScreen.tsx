import { useEffect, useMemo, useState } from "react";

import { api } from "./api";
import type { McpServer, ProviderKind, ProviderView } from "./types";

type McpRegistryScreenProps = {
  providers: ProviderView[];
};

const FALLBACK_PROVIDER_KINDS: ProviderKind[] = ["ollama", "llama_cpp", "codex", "claude"];
const DEFAULT_ALLOWED_PROVIDERS: ProviderKind[] = ["ollama", "llama_cpp"];

export function McpRegistryScreen({ providers }: McpRegistryScreenProps) {
  const [mcpServers, setMcpServers] = useState<McpServer[]>([]);
  const [mcpName, setMcpName] = useState("");
  const [mcpCommand, setMcpCommand] = useState("");
  const [mcpArgs, setMcpArgs] = useState("");
  const [allowedProviders, setAllowedProviders] =
    useState<ProviderKind[]>(DEFAULT_ALLOWED_PROVIDERS);
  const [error, setError] = useState<string | null>(null);

  const providerChoices = useMemo(() => {
    if (providers.length === 0) return FALLBACK_PROVIDER_KINDS;
    const known = new Set<ProviderKind>();
    for (const provider of providers) {
      known.add(provider.provider);
    }
    return FALLBACK_PROVIDER_KINDS.filter((provider) => known.has(provider));
  }, [providers]);

  useEffect(() => {
    void loadServers();
  }, []);

  async function loadServers() {
    try {
      setMcpServers(await api.listMcpServers());
    } catch (err) {
      setError(asError(err));
    }
  }

  function toggleAllowedProvider(provider: ProviderKind) {
    setAllowedProviders((current) =>
      current.includes(provider)
        ? current.filter((item) => item !== provider)
        : [...current, provider],
    );
  }

  async function saveMcpServer() {
    if (!mcpName.trim() || !mcpCommand.trim()) return;
    try {
      setError(null);
      await api.upsertMcpServer({
        name: mcpName.trim(),
        command: mcpCommand.trim(),
        args: mcpArgs
          .split(" ")
          .map((value) => value.trim())
          .filter(Boolean),
        local_only: true,
        enabled: true,
        allowed_providers:
          allowedProviders.length > 0 ? allowedProviders : DEFAULT_ALLOWED_PROVIDERS,
      });
      setMcpName("");
      setMcpCommand("");
      setMcpArgs("");
      setAllowedProviders(DEFAULT_ALLOWED_PROVIDERS);
      await loadServers();
    } catch (err) {
      setError(asError(err));
    }
  }

  async function toggleMcp(server: McpServer) {
    try {
      setError(null);
      await api.setMcpServerEnabled(server.id, !server.enabled);
      await loadServers();
    } catch (err) {
      setError(asError(err));
    }
  }

  return (
    <section className="mcp-screen">
      <section className="card tooling-header">
        <div>
          <span className="eyebrow">Shared tooling</span>
          <h3>MCP registry</h3>
          <p className="section-copy">
            Keep MCP connectivity explicit and separate from model distribution. Local
            model inventory and curated installs now live in `Providers`, so this page
            stays focused on shared tool access and allowlists.
          </p>
        </div>

        <div className="agents-hero-stats">
          <ToolingMetric label="MCP servers" value={String(mcpServers.length)} />
          <ToolingMetric
            label="Enabled MCPs"
            value={String(mcpServers.filter((server) => server.enabled).length)}
          />
          <ToolingMetric label="Providers known" value={String(providerChoices.length)} />
        </div>
      </section>

      <section className="tooling-layout">
        <article className="card tooling-card">
          <div className="section-head">
            <div>
              <span className="eyebrow">MCPs</span>
              <h4>Create and allowlist servers</h4>
            </div>
          </div>

          <p className="section-copy">
            Register local MCP servers once, then decide which providers are allowed to
            use them in workflows.
          </p>

          <div className="form-row">
            <label>Name</label>
            <input value={mcpName} onChange={(event) => setMcpName(event.target.value)} />
          </div>

          <div className="form-row">
            <label>Command</label>
            <input
              value={mcpCommand}
              onChange={(event) => setMcpCommand(event.target.value)}
              placeholder="npx, uvx, or local binary"
            />
          </div>

          <div className="form-row">
            <label>Args</label>
            <input
              value={mcpArgs}
              onChange={(event) => setMcpArgs(event.target.value)}
              placeholder="space separated args"
            />
          </div>

          <div className="form-row">
            <label>Allowed providers</label>
            <div className="provider-pill-grid">
              {providerChoices.map((provider) => (
                <button
                  key={provider}
                  type="button"
                  className={
                    allowedProviders.includes(provider)
                      ? "provider-pill provider-pill-active"
                      : "provider-pill"
                  }
                  onClick={() => toggleAllowedProvider(provider)}
                >
                  {labelProvider(provider)}
                </button>
              ))}
            </div>
          </div>

          <button className="primary" onClick={() => void saveMcpServer()}>
            Save MCP server
          </button>
        </article>

        <article className="card tooling-card tooling-card-wide">
          <div className="section-head">
            <div>
              <span className="eyebrow">Registry state</span>
              <h4>Current MCP infrastructure</h4>
            </div>
          </div>

          {error ? <p className="agent-error">{error}</p> : null}

          {mcpServers.length === 0 ? (
            <p className="muted-copy">No MCP servers registered yet.</p>
          ) : (
            <div className="mini-list">
              {mcpServers.map((server) => (
                <div key={server.id} className="registry-item">
                  <div>
                    <div className="artifact-head">
                      <strong>{server.name}</strong>
                      <span className={`chip ${server.enabled ? "chip-blue" : ""}`}>
                        {server.enabled ? "enabled" : "disabled"}
                      </span>
                    </div>
                    <p className="muted-copy">{server.command}</p>
                    <p className="muted-copy">
                      Providers:{" "}
                      {server.allowed_providers.map((provider) => labelProvider(provider)).join(", ")}
                    </p>
                  </div>
                  <button className="ghost" onClick={() => void toggleMcp(server)}>
                    {server.enabled ? "Disable" : "Enable"}
                  </button>
                </div>
              ))}
            </div>
          )}
        </article>
      </section>
    </section>
  );
}

function ToolingMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="agent-metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function labelProvider(provider: ProviderKind) {
  switch (provider) {
    case "codex":
      return "Codex";
    case "claude":
      return "Claude";
    case "ollama":
      return "Ollama";
    case "llama_cpp":
      return "llama.cpp";
  }
}

function asError(value: unknown): string {
  return value instanceof Error ? value.message : "Unknown error";
}
