import { useEffect, useMemo, useState } from "react";

import { api } from "./api";
import type {
  DeviceHardwareProfile,
  GovernanceProviderStatus,
  LlamaCppModel,
  LocalModelCatalogEntry,
  LocalModelInventoryView,
  ModelInstallJob,
  ProviderKind,
  ProvidersGovernanceView,
  ProviderView,
  UsageSummary,
} from "./types";

type ProvidersControlPlaneProps = {
  providers: ProviderView[];
  llamaModels: LlamaCppModel[];
  usage: UsageSummary;
  authenticatedProviders: number;
  onRefresh: () => Promise<unknown>;
  onAuth: (provider: ProviderKind, action: "login" | "logout") => Promise<void>;
  onSave: (provider: ProviderKind, model: string, effort: string) => Promise<void>;
};

const OBJECTIVE_OPTIONS = [
  { value: "all", label: "All objectives" },
  { value: "chat", label: "General chat" },
  { value: "coding", label: "Coding" },
  { value: "reasoning", label: "Reasoning" },
  { value: "analysis", label: "Analysis" },
  { value: "vision", label: "Vision" },
  { value: "document_extraction", label: "Document extraction" },
] as const;

const EFFORT_OPTIONS = ["low", "medium", "high", "xhigh"] as const;
const STATIC_MODEL_OPTIONS: Record<ProviderKind, string[]> = {
  codex: [
    "gpt-5.4",
    "gpt-5.3-codex",
    "gpt-5.2-codex",
    "gpt-5.2",
    "gpt-5.1-codex-max",
    "gpt-5.1-codex-mini",
    "gpt-5-codex",
    "gpt-5",
    "gpt-5-mini",
  ],
  claude: ["opus-4.1", "sonnet-4", "sonnet", "haiku"],
  ollama: ["llama3.1:8b", "qwen2.5-coder:7b", "deepseek-r1:8b"],
  llama_cpp: ["var/models/llama.cpp/model.gguf"],
};

export function ProvidersControlPlane(props: ProvidersControlPlaneProps) {
  const [section, setSection] = useState<"closed" | "local" | "marketplace" | "governance">(
    "marketplace",
  );
  const [objective, setObjective] = useState("all");
  const [hardware, setHardware] = useState<{
    authority: string;
    host: DeviceHardwareProfile;
    browser: DeviceHardwareProfile | null;
  } | null>(null);
  const [inventory, setInventory] = useState<LocalModelInventoryView>({
    ollama: [],
    gguf: [],
    issues: [],
  });
  const [catalog, setCatalog] = useState<LocalModelCatalogEntry[]>([]);
  const [jobs, setJobs] = useState<ModelInstallJob[]>([]);
  const [governance, setGovernance] = useState<ProvidersGovernanceView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyCatalogKey, setBusyCatalogKey] = useState<string | null>(null);
  const [importAlias, setImportAlias] = useState("");
  const [importPath, setImportPath] = useState("");
  const [importQuantization, setImportQuantization] = useState("");
  const [importContextWindow, setImportContextWindow] = useState("8192");

  const closedProviders = useMemo(
    () => props.providers.filter((provider) => !isLocalProvider(provider.provider)),
    [props.providers],
  );
  const localProviders = useMemo(
    () => props.providers.filter((provider) => isLocalProvider(provider.provider)),
    [props.providers],
  );
  const recommendedCatalog = useMemo(
    () =>
      catalog.filter(
        (entry) =>
          entry.recommendation_band === "recommended" ||
          entry.recommendation_band === "possible_with_tradeoffs",
      ),
    [catalog],
  );
  const activeJobs = useMemo(
    () => jobs.filter((job) => job.status === "pending" || job.status === "running"),
    [jobs],
  );
  const governanceIssues = useMemo(() => {
    const issues = new Set<string>();

    for (const issue of inventory.issues) {
      issues.add(issue);
    }

    for (const issue of governance?.inventory_issues ?? []) {
      issues.add(issue);
    }

    for (const provider of governance?.local_providers ?? []) {
      if (provider.status_label !== "healthy" && provider.detail) {
        issues.add(`${provider.display_name}: ${provider.detail}`);
      }

      for (const issue of provider.issues) {
        issues.add(`${provider.display_name}: ${issue}`);
      }
    }

    return Array.from(issues);
  }, [governance?.inventory_issues, governance?.local_providers, inventory.issues]);

  useEffect(() => {
    void loadData();
  }, [objective]);

  useEffect(() => {
    void saveBrowserSnapshot();
  }, []);

  useEffect(() => {
    if (activeJobs.length === 0) return;
    const timer = window.setInterval(() => {
      void Promise.all([reloadInventory(), reloadJobs(), reloadCatalog(), reloadGovernance()]);
    }, 2500);
    return () => window.clearInterval(timer);
  }, [activeJobs.length, objective]);

  async function loadData() {
    try {
      setLoading(true);
      setError(null);
      await Promise.all([
        reloadHardware(),
        reloadInventory(),
        reloadJobs(),
        reloadCatalog(),
        reloadGovernance(),
      ]);
    } catch (err) {
      setError(asError(err));
    } finally {
      setLoading(false);
    }
  }

  async function reloadHardware() {
    setHardware(await api.getProvidersHardware());
  }

  async function reloadInventory() {
    setInventory(await api.listLocalModelsInstalled());
  }

  async function reloadJobs() {
    setJobs(await api.listLocalModelInstallJobs());
  }

  async function reloadGovernance() {
    setGovernance(await api.getProvidersGovernance());
  }

  async function reloadCatalog() {
    setCatalog(
      await api.listLocalModelCatalog({
        objective: objective === "all" ? undefined : objective,
      }),
    );
  }

  async function saveBrowserSnapshot() {
    try {
      const snapshot = detectBrowserHardware();
      if (!snapshot) return;
      await api.saveBrowserHardwareSnapshot(snapshot);
      await Promise.all([reloadHardware(), reloadGovernance()]);
    } catch {
      // Browser telemetry is optional.
    }
  }

  async function handleRefreshAll() {
    await Promise.all([props.onRefresh(), loadData()]);
  }

  async function handleInstall(catalogKey: string) {
    try {
      setBusyCatalogKey(catalogKey);
      setError(null);
      await api.installOllamaCatalogModel({
        catalog_key: catalogKey,
        actor_name: "local-operator",
        source_app: "ferrum-web",
        source_channel: "providers",
      });
      setSection("local");
      await Promise.all([
        reloadJobs(),
        reloadInventory(),
        reloadCatalog(),
        reloadGovernance(),
        props.onRefresh(),
      ]);
    } catch (err) {
      setError(asError(err));
    } finally {
      setBusyCatalogKey(null);
    }
  }

  async function handleImport() {
    if (!importAlias.trim() || !importPath.trim()) return;
    try {
      setError(null);
      await api.importLocalGguf({
        alias: importAlias.trim(),
        file_path: importPath.trim(),
        quantization: importQuantization.trim() || undefined,
        context_window: Number.parseInt(importContextWindow, 10) || undefined,
        actor_name: "local-operator",
        source_app: "ferrum-web",
        source_channel: "providers",
      });
      setImportAlias("");
      setImportPath("");
      setImportQuantization("");
      setImportContextWindow("8192");
      setSection("local");
      await Promise.all([reloadInventory(), reloadJobs(), reloadGovernance(), props.onRefresh()]);
    } catch (err) {
      setError(asError(err));
    }
  }

  return (
    <section className="providers-screen providers-control-plane">
      <section className="providers-hero card">
        <div className="providers-hero-copy">
          <span className="eyebrow">Secure provider control plane</span>
          <h3>Closed providers, local inventory, and curated installs in one place</h3>
          <p>
            The Ferrum host is authoritative for install and compatibility decisions.
            Browser telemetry is supporting context only, and local model ingress stays
            on approved or controlled paths.
          </p>
        </div>

        <div className="hero-stats providers-hero-stats-extended">
          <MetricCard
            label="Closed providers authenticated"
            value={`${props.authenticatedProviders}/${closedProviders.length}`}
          />
          <MetricCard
            label="Installed local models"
            value={String(inventory.ollama.length + inventory.gguf.length)}
          />
          <MetricCard label="Recommended now" value={String(recommendedCatalog.length)} />
          <MetricCard label="Active install jobs" value={String(activeJobs.length)} />
        </div>

        <div className="providers-section-tabs">
          <button
            className={section === "closed" ? "provider-pill provider-pill-active" : "provider-pill"}
            onClick={() => setSection("closed")}
          >
            Closed Providers
          </button>
          <button
            className={section === "local" ? "provider-pill provider-pill-active" : "provider-pill"}
            onClick={() => setSection("local")}
          >
            Local Models
          </button>
          <button
            className={section === "marketplace" ? "provider-pill provider-pill-active" : "provider-pill"}
            onClick={() => setSection("marketplace")}
          >
            Local Model Marketplace
          </button>
          <button
            className={section === "governance" ? "provider-pill provider-pill-active" : "provider-pill"}
            onClick={() => setSection("governance")}
          >
            Governance
          </button>
          <button className="ghost refresh-button" onClick={() => void handleRefreshAll()}>
            Refresh all
          </button>
        </div>
      </section>

      {error ? <p className="agent-error">{error}</p> : null}
      {loading ? <p className="muted-copy">Loading provider control plane...</p> : null}

      {section === "closed" ? (
        <section className="provider-grid">
          {closedProviders.map((provider) => (
            <ProviderCard
              key={provider.provider}
              provider={provider}
              usage={props.usage.daily.find((row) => row.provider === provider.provider)}
              llamaModels={props.llamaModels}
              onAuth={props.onAuth}
              onSave={props.onSave}
            />
          ))}
        </section>
      ) : null}

      {section === "local" ? (
        <section className="providers-local-layout">
          <article className="card providers-local-panel">
            <div className="section-head">
              <div>
                <span className="eyebrow">Runtime health</span>
                <h4>Local providers</h4>
              </div>
            </div>
            <div className="provider-grid provider-grid-single">
              {localProviders.map((provider) => (
                <ProviderCard
                  key={provider.provider}
                  provider={provider}
                  usage={props.usage.daily.find((row) => row.provider === provider.provider)}
                  llamaModels={props.llamaModels}
                  onAuth={props.onAuth}
                  onSave={props.onSave}
                />
              ))}
            </div>
          </article>

          <article className="card providers-local-panel">
            <div className="section-head">
              <div>
                <span className="eyebrow">Installed</span>
                <h4>Inventory snapshot</h4>
              </div>
            </div>

            {inventory.issues.length > 0 ? (
              <div className="issues-list">
                {inventory.issues.map((issue) => (
                  <span key={issue} className="issue-pill">
                    {issue}
                  </span>
                ))}
              </div>
            ) : null}

            <div className="tooling-list-grid providers-inventory-grid">
              <div className="tooling-list-block">
                <strong>Ollama</strong>
                {inventory.ollama.length === 0 ? (
                  <p className="muted-copy">No local Ollama models reported yet.</p>
                ) : (
                  <div className="mini-list">
                    {inventory.ollama.map((model) => (
                      <InventoryItem key={model.model_ref} model={model} />
                    ))}
                  </div>
                )}
              </div>

              <div className="tooling-list-block">
                <strong>GGUF / llama.cpp</strong>
                {inventory.gguf.length === 0 ? (
                  <p className="muted-copy">No GGUF models imported into the registry yet.</p>
                ) : (
                  <div className="mini-list">
                    {inventory.gguf.map((model) => (
                      <InventoryItem key={model.model_ref} model={model} />
                    ))}
                  </div>
                )}
              </div>
            </div>
          </article>

          <article className="card providers-local-panel">
            <div className="section-head">
              <div>
                <span className="eyebrow">Audit</span>
                <h4>Install jobs</h4>
              </div>
            </div>
            {jobs.length === 0 ? (
              <p className="muted-copy">No install activity recorded yet.</p>
            ) : (
              <div className="mini-list">
                {jobs.map((job) => (
                  <InstallJobItem key={job.id} job={job} />
                ))}
              </div>
            )}
          </article>
        </section>
      ) : null}

      {section === "marketplace" ? (
        <section className="providers-marketplace">
          <article className="card providers-hardware-panel">
            <div className="section-head">
              <div>
                <span className="eyebrow">Hardware authority</span>
                <h4>Host decides, browser informs</h4>
              </div>
              <div className="provider-current">
                <span className="chip chip-blue">Authority: {hardware?.authority ?? "host"}</span>
                <span className="chip">Enterprise restricted</span>
              </div>
            </div>
            <div className="providers-hardware-grid">
              <HardwareCard title="Ferrum host" profile={hardware?.host ?? null} authoritative />
              <HardwareCard title="Browser context" profile={hardware?.browser ?? null} />
            </div>
          </article>

          <article className="card providers-marketplace-panel">
            <div className="section-head">
              <div>
                <span className="eyebrow">Recommended</span>
                <h4>Curated models for this host</h4>
              </div>
              <div className="form-row objective-filter-row">
                <label>Objective</label>
                <select value={objective} onChange={(event) => setObjective(event.target.value)}>
                  {OBJECTIVE_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </div>
            </div>
            <div className="providers-market-grid">
              {recommendedCatalog.map((entry) => (
                <CatalogCard
                  key={entry.key}
                  entry={entry}
                  busy={busyCatalogKey === entry.key}
                  onInstall={handleInstall}
                />
              ))}
            </div>
          </article>

          <article className="card providers-marketplace-panel">
            <div className="section-head">
              <div>
                <span className="eyebrow">Catalog</span>
                <h4>Approved and blocked entries</h4>
              </div>
            </div>
            <div className="providers-market-grid providers-catalog-grid">
              {catalog.map((entry) => (
                <CatalogCard
                  key={entry.key}
                  entry={entry}
                  busy={busyCatalogKey === entry.key}
                  compact
                  onInstall={handleInstall}
                />
              ))}
            </div>
          </article>

          <article className="card providers-marketplace-panel">
            <div className="section-head">
              <div>
                <span className="eyebrow">Import GGUF</span>
                <h4>Controlled advanced path</h4>
              </div>
            </div>
            <p className="section-copy">
              Use this only for a local file you already trust. Ferrum validates the
              extension, computes a checksum, and adds the model to the managed llama.cpp
              registry without accepting arbitrary download URLs.
            </p>
            <div className="skills-form-grid">
              <div className="form-row">
                <label>Alias</label>
                <input
                  value={importAlias}
                  onChange={(event) => setImportAlias(event.target.value)}
                  placeholder="phi4-mini-q4"
                />
              </div>
              <div className="form-row">
                <label>GGUF path</label>
                <input
                  value={importPath}
                  onChange={(event) => setImportPath(event.target.value)}
                  placeholder="absolute path or relative to LLAMA_CPP_MODEL_DIR"
                />
              </div>
              <div className="form-row">
                <label>Quantization</label>
                <input
                  value={importQuantization}
                  onChange={(event) => setImportQuantization(event.target.value)}
                  placeholder="Q4_K_M"
                />
              </div>
              <div className="form-row">
                <label>Context window</label>
                <input
                  value={importContextWindow}
                  onChange={(event) => setImportContextWindow(event.target.value)}
                  placeholder="8192"
                />
              </div>
            </div>
            <div className="provider-actions">
              <button
                className="primary"
                disabled={!importAlias.trim() || !importPath.trim()}
                onClick={() => void handleImport()}
              >
                Import local GGUF
              </button>
            </div>
          </article>
        </section>
      ) : null}

      {section === "governance" ? (
        <section className="providers-governance-layout">
          <article className="card providers-governance-panel">
            <div className="section-head">
              <div>
                <span className="eyebrow">Governance</span>
                <h4>Runtime authority and service posture</h4>
              </div>
              <span className="chip chip-blue">
                Refreshed {formatTimestamp(governance?.last_refresh_at)}
              </span>
            </div>
            <div className="tooling-list-grid providers-governance-summary">
              <MetricInline
                label="Ollama mode"
                value={labelRuntimeMode(governance?.ollama_runtime_mode ?? "endpoint_only")}
              />
              <MetricInline
                label="Inventory issues"
                value={String(governanceIssues.length)}
              />
              <MetricInline
                label="Recent jobs"
                value={String(governance?.recent_jobs.length ?? jobs.length)}
              />
              <MetricInline
                label="Authority"
                value={governance?.authority ?? hardware?.authority ?? "host"}
              />
            </div>
            <div className="provider-current">
              <span className="chip">Endpoint: {governance?.ollama_api_base ?? "Unknown"}</span>
              <span className="chip">
                Browser telemetry: {governance?.browser ? "available" : "optional/unavailable"}
              </span>
              <span className="chip">Host-first</span>
            </div>
            {governanceIssues.length > 0 ? (
              <p className="provider-inline-note">
                Governance has active runtime issues. Review local provider health before relying
                on host-local execution.
              </p>
            ) : null}
          </article>

          <article className="card providers-governance-panel">
            <div className="section-head">
              <div>
                <span className="eyebrow">Runtime health</span>
                <h4>Local providers</h4>
              </div>
            </div>
            <div className="mini-list">
              {(governance?.local_providers ?? []).map((provider) => (
                <GovernanceProviderItem key={provider.provider} provider={provider} />
              ))}
            </div>
          </article>

          <article className="card providers-governance-panel">
            <div className="section-head">
              <div>
                <span className="eyebrow">Issues</span>
                <h4>Current blockers and warnings</h4>
              </div>
            </div>
            {governanceIssues.length > 0 ? (
              <div className="issues-list">
                {governanceIssues.map((issue) => (
                  <span key={issue} className="issue-pill">
                    {issue}
                  </span>
                ))}
              </div>
            ) : (
              <p className="muted-copy">
                No inventory issues reported. Governance is intentionally lightweight in this
                phase.
              </p>
            )}
          </article>

          <article className="card providers-governance-panel">
            <div className="section-head">
              <div>
                <span className="eyebrow">Recent activity</span>
                <h4>Install jobs</h4>
              </div>
            </div>
            {(governance?.recent_jobs.length ?? 0) > 0 ? (
              <div className="mini-list">
                {governance?.recent_jobs.map((job) => (
                  <InstallJobItem key={job.id} job={job} />
                ))}
              </div>
            ) : (
              <p className="muted-copy">No recent install jobs captured yet.</p>
            )}
          </article>
        </section>
      ) : null}
    </section>
  );
}

function GovernanceProviderItem(props: { provider: GovernanceProviderStatus }) {
  return (
    <div className="registry-item governance-provider-item">
      <div>
        <div className="artifact-head">
          <strong>{props.provider.display_name}</strong>
          <span className={`chip ${props.provider.status_label === "healthy" ? "chip-blue" : ""}`}>
            {props.provider.status_label}
          </span>
        </div>
        <p className="muted-copy">
          {props.provider.version ?? "Version unknown"} · {props.provider.auth_status}
        </p>
        <p className="muted-copy">{props.provider.detail ?? "No additional runtime detail."}</p>
        {props.provider.issues.length > 0 ? (
          <div className="issues-list">
            {props.provider.issues.map((issue) => (
              <span key={issue} className="issue-pill">
                {issue}
              </span>
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function ProviderCard(props: {
  provider: ProviderView;
  usage?: UsageSummary["daily"][number];
  llamaModels: LlamaCppModel[];
  onAuth: (provider: ProviderKind, action: "login" | "logout") => Promise<void>;
  onSave: (provider: ProviderKind, model: string, effort: string) => Promise<void>;
}) {
  const models = getModelOptions(props.provider.provider, props.llamaModels);
  const [model, setModel] = useState(
    props.provider.selected_model && models.includes(props.provider.selected_model)
      ? props.provider.selected_model
      : models[0] ?? "",
  );
  const [effort, setEffort] = useState(props.provider.selected_effort ?? "medium");

  useEffect(() => {
    setModel(
      props.provider.selected_model && models.includes(props.provider.selected_model)
        ? props.provider.selected_model
        : models[0] ?? "",
    );
    setEffort(props.provider.selected_effort ?? "medium");
  }, [models, props.provider.selected_effort, props.provider.selected_model]);

  return (
    <article className="provider-card card">
      <div className="provider-card-header">
        <div>
          <span className="provider-kicker">{labelProvider(props.provider.provider)}</span>
          <h3>{props.provider.version ?? "Version unknown"}</h3>
        </div>
        <span className={`badge ${props.provider.auth_status}`}>{props.provider.auth_status}</span>
      </div>

      <div className="provider-current">
        <span className="chip">{props.provider.display_name}</span>
        <span className="chip">{props.provider.data_boundary}</span>
        {isLocalProvider(props.provider.provider) ? (
          <span className="chip">Host-local runtime</span>
        ) : null}
      </div>

      <div className="workflow-create-grid">
        <div className="form-row">
          <label>Model</label>
          <select value={model} onChange={(event) => setModel(event.target.value)}>
            {models.map((candidate) => (
              <option key={candidate} value={candidate}>
                {candidate}
              </option>
            ))}
          </select>
        </div>
        {supportsEffort(props.provider.provider) ? (
          <div className="form-row">
            <label>Effort</label>
            <select value={effort} onChange={(event) => setEffort(event.target.value)}>
              {EFFORT_OPTIONS.map((candidate) => (
                <option key={candidate} value={candidate}>
                  {candidate}
                </option>
              ))}
            </select>
          </div>
        ) : (
          <div className="provider-inline-note">
            Effort is not used by local runtimes yet. Prioritize the right model and hardware fit.
          </div>
        )}
      </div>

      <div className="provider-actions">
        <button
          className="primary"
          disabled={!model}
          onClick={() => void props.onSave(props.provider.provider, model, effort)}
        >
          Save defaults
        </button>
        {props.provider.auth_required ? (
          props.provider.auth_status === "authenticated" ? (
            <button className="ghost" onClick={() => void props.onAuth(props.provider.provider, "logout")}>
              Logout
            </button>
          ) : (
            <button className="ghost" onClick={() => void props.onAuth(props.provider.provider, "login")}>
              Login
            </button>
          )
        ) : null}
      </div>

      <p className="provider-detail">{props.provider.detail ?? "No extra detail reported."}</p>
      {props.usage ? (
        <div className="provider-usage">
          <MetricInline label="Input tokens today" value={formatNumber(props.usage.input_tokens)} />
          <MetricInline label="Output tokens today" value={formatNumber(props.usage.output_tokens)} />
          <MetricInline label="Total tokens today" value={formatNumber(props.usage.total_tokens)} />
        </div>
      ) : (
        <p className="provider-detail">No token usage recorded today.</p>
      )}
      {props.provider.issues.length > 0 ? (
        <div className="issues-list">
          {props.provider.issues.map((issue) => (
            <span key={issue} className="issue-pill">
              {issue}
            </span>
          ))}
        </div>
      ) : null}
    </article>
  );
}

function InventoryItem(props: { model: LocalModelInventoryView["ollama"][number] }) {
  return (
    <div className="registry-item">
      <div>
        <div className="artifact-head">
          <strong>{props.model.display_name}</strong>
          <span className={props.model.enabled ? "chip chip-blue" : "chip"}>
            {props.model.enabled ? "enabled" : "disabled"}
          </span>
        </div>
        <p className="muted-copy">{props.model.file_path ?? props.model.model_ref}</p>
        <p className="muted-copy">
          {props.model.quantization ?? "quant pending"}
          {props.model.context_window ? ` · ${props.model.context_window} ctx` : ""}
          {props.model.installed_from_catalog ? ` · curated: ${props.model.installed_from_catalog}` : ""}
        </p>
      </div>
    </div>
  );
}

function InstallJobItem(props: { job: ModelInstallJob }) {
  return (
    <div className="registry-item install-job-item">
      <div>
        <div className="artifact-head">
          <strong>{props.job.catalog_key ?? props.job.source_ref ?? props.job.runtime_target}</strong>
          <span className={`chip ${props.job.status === "completed" ? "chip-blue" : ""}`}>
            {props.job.status}
          </span>
        </div>
        <p className="muted-copy">
          {props.job.runtime_target} · {props.job.actor_name} · {props.job.source_app}
        </p>
        <p className="muted-copy">
          {props.job.detail ?? "No detail recorded."}
          {props.job.error_text ? ` · ${props.job.error_text}` : ""}
        </p>
      </div>
      <div className="install-job-progress">
        <strong>{props.job.progress_percent}%</strong>
        <div className="progress-track">
          <span style={{ width: `${props.job.progress_percent}%` }} />
        </div>
      </div>
    </div>
  );
}

function HardwareCard(props: {
  title: string;
  profile: DeviceHardwareProfile | null;
  authoritative?: boolean;
}) {
  return (
    <article className="provider-card card provider-card-soft">
      <div className="provider-card-header">
        <div>
          <span className="provider-kicker">{props.title}</span>
          <h3>{props.profile?.platform ?? "Snapshot unavailable"}</h3>
        </div>
        <span className={props.authoritative ? "chip chip-blue" : "chip"}>
          {props.authoritative ? "authoritative" : "supporting"}
        </span>
      </div>
      <div className="tooling-list-grid providers-hardware-metrics">
        <MetricInline label="CPU" value={props.profile?.cpu_brand ?? "Unknown"} />
        <MetricInline
          label="Logical cores"
          value={props.profile?.logical_cores ? String(props.profile.logical_cores) : "Unknown"}
        />
        <MetricInline
          label="Memory"
          value={
            props.profile?.total_memory_gb
              ? `${props.profile.total_memory_gb.toFixed(1)} GB`
              : "Unknown"
          }
        />
        <MetricInline
          label="Available"
          value={
            props.profile?.available_memory_gb
              ? `${props.profile.available_memory_gb.toFixed(1)} GB`
              : "Unknown"
          }
        />
      </div>
      <p className="provider-detail">
        {props.profile?.gpu_renderer ?? "GPU telemetry unavailable"}
      </p>
    </article>
  );
}

function CatalogCard(props: {
  entry: LocalModelCatalogEntry;
  busy: boolean;
  compact?: boolean;
  onInstall: (catalogKey: string) => Promise<void>;
}) {
  const canInstall =
    props.entry.runtime_target === "ollama" &&
    props.entry.policy_state === "approved_for_install";

  return (
    <article className={props.compact ? "provider-card card provider-card-soft" : "provider-card card"}>
      <div className="provider-card-header">
        <div>
          <span className="provider-kicker">
            {props.entry.family} · {props.entry.runtime_target}
          </span>
          <h3>{props.entry.display_name}</h3>
        </div>
        <span className={`chip ${props.entry.recommendation_band === "recommended" ? "chip-blue" : ""}`}>
          {labelBand(props.entry.recommendation_band)}
        </span>
      </div>
      <div className="provider-current">
        <span className="chip">{labelPolicyState(props.entry.policy_state)}</span>
        <span className="chip">{props.entry.modality}</span>
        {props.entry.objectives.map((objective) => (
          <span key={objective} className="chip">
            {objective}
          </span>
        ))}
      </div>
      <p className="provider-detail">{props.entry.summary}</p>
      <p className="provider-detail">{props.entry.fit_reason}</p>
      <div className="tooling-list-grid providers-catalog-metrics">
        <MetricInline label="Params" value={`${props.entry.parameter_size_b.toFixed(1)}B`} />
        <MetricInline label="Artifact" value={`${props.entry.artifact_size_gb.toFixed(1)} GB`} />
        <MetricInline label="Memory min" value={`${props.entry.memory_min_gb.toFixed(1)} GB`} />
        <MetricInline label="Memory rec" value={`${props.entry.memory_recommended_gb.toFixed(1)} GB`} />
      </div>
      <div className="provider-actions">
        {props.entry.installed ? (
          <button className="ghost" disabled>
            Already installed
          </button>
        ) : canInstall ? (
          <button
            className="primary"
            disabled={props.busy}
            onClick={() => void props.onInstall(props.entry.key)}
          >
            {props.busy ? "Installing..." : "Install via Ollama"}
          </button>
        ) : props.entry.runtime_target === "llama_cpp" &&
          props.entry.policy_state === "approved_for_install" ? (
          <button className="ghost" disabled>
            Curated GGUF download next
          </button>
        ) : (
          <button className="ghost" disabled>
            Blocked by policy
          </button>
        )}
      </div>
    </article>
  );
}

function MetricCard(props: { label: string; value: string }) {
  return (
    <article className="metric-card">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </article>
  );
}

function MetricInline(props: { label: string; value: string }) {
  return (
    <div className="metric-inline">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  );
}

function detectBrowserHardware() {
  if (typeof navigator === "undefined") return null;
  const renderer = readWebGlRenderer();
  return {
    platform: navigator.platform ?? null,
    cpu_brand: null,
    logical_cores: navigator.hardwareConcurrency ?? null,
    device_memory_gb:
      "deviceMemory" in navigator
        ? Number((navigator as Navigator & { deviceMemory?: number }).deviceMemory ?? 0) || null
        : null,
    total_memory_gb: null,
    available_memory_gb: null,
    gpu_vendor: renderer?.vendor ?? null,
    gpu_renderer: renderer?.renderer ?? null,
    user_agent: navigator.userAgent || null,
  };
}

function readWebGlRenderer() {
  try {
    const canvas = document.createElement("canvas");
    const gl =
      canvas.getContext("webgl") || canvas.getContext("experimental-webgl");
    if (!gl || typeof WebGLRenderingContext === "undefined") return null;
    const context = gl as WebGLRenderingContext;
    const ext = context.getExtension("WEBGL_debug_renderer_info");
    if (!ext) return null;
    return {
      vendor: context.getParameter(ext.UNMASKED_VENDOR_WEBGL) as string,
      renderer: context.getParameter(ext.UNMASKED_RENDERER_WEBGL) as string,
    };
  } catch {
    return null;
  }
}

function isLocalProvider(provider: ProviderKind) {
  return provider === "ollama" || provider === "llama_cpp";
}

function supportsEffort(provider: ProviderKind) {
  return provider === "codex" || provider === "claude";
}

function getModelOptions(provider: ProviderKind, llamaModels: LlamaCppModel[]) {
  if (provider === "llama_cpp") {
    const registered = llamaModels
      .filter((model) => model.enabled)
      .map((model) => model.file_path);
    return registered.length > 0 ? registered : STATIC_MODEL_OPTIONS.llama_cpp;
  }
  return STATIC_MODEL_OPTIONS[provider];
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

function labelBand(value: string) {
  switch (value) {
    case "recommended":
      return "Recommended";
    case "possible_with_tradeoffs":
      return "Possible with tradeoffs";
    case "visible_but_blocked":
      return "Visible but blocked";
    default:
      return "Not recommended";
  }
}

function labelPolicyState(value: string) {
  switch (value) {
    case "already_installed":
      return "Already installed";
    case "approved_for_install":
      return "Approved for install";
    default:
      return "Visible but blocked";
  }
}

function formatNumber(value: number) {
  return new Intl.NumberFormat().format(value);
}

function asError(value: unknown): string {
  return value instanceof Error ? value.message : "Unknown error";
}

function labelRuntimeMode(value: string) {
  switch (value) {
    case "host":
      return "Host";
    case "docker":
      return "Docker";
    default:
      return "Endpoint only";
  }
}

function formatTimestamp(value?: string | null) {
  if (!value) return "just now";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "just now" : date.toLocaleString();
}
