import { useEffect, useMemo, useState } from "react";

import { api } from "./api";
import type {
  ApprovalGate,
  LlamaCppModel,
  McpServer,
  ProviderKind,
  ProviderView,
  TerminalOutput,
  WorkflowDetail,
  WorkflowTemplate,
  WorkflowSummary,
} from "./types";

type AgentModeProps = {
  providers: ProviderView[];
};

export function AgentMode({ providers }: AgentModeProps) {
  const [workflows, setWorkflows] = useState<WorkflowSummary[]>([]);
  const [templates, setTemplates] = useState<WorkflowTemplate[]>([]);
  const [activeWorkflowId, setActiveWorkflowId] = useState<string | null>(null);
  const [detail, setDetail] = useState<WorkflowDetail | null>(null);
  const [terminalBuffers, setTerminalBuffers] = useState<Record<string, string[]>>({});
  const [title, setTitle] = useState("");
  const [objective, setObjective] = useState("");
  const [sensitivity, setSensitivity] = useState("internal");
  const [templateKey, setTemplateKey] = useState("engineering_pipeline");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mcpServers, setMcpServers] = useState<McpServer[]>([]);
  const [llamaModels, setLlamaModels] = useState<LlamaCppModel[]>([]);
  const [snapshotLabel, setSnapshotLabel] = useState("");
  const [mcpName, setMcpName] = useState("");
  const [mcpCommand, setMcpCommand] = useState("");
  const [mcpArgs, setMcpArgs] = useState("");
  const [llamaAlias, setLlamaAlias] = useState("");
  const [llamaPath, setLlamaPath] = useState("");
  const [llamaQuant, setLlamaQuant] = useState("");
  const [llamaContext, setLlamaContext] = useState("8192");
  const [workflowPage, setWorkflowPage] = useState(0);
  const [approvalsPage, setApprovalsPage] = useState(0);
  const [artifactsPage, setArtifactsPage] = useState(0);
  const [handoffsPage, setHandoffsPage] = useState(0);
  const [verdictsPage, setVerdictsPage] = useState(0);
  const [evidencePage, setEvidencePage] = useState(0);
  const [snapshotsPage, setSnapshotsPage] = useState(0);

  const coordinatorOptions = useMemo(
    () =>
      providers.length > 0
        ? providers.map((provider) => provider.provider)
        : (["ollama", "llama_cpp", "codex", "claude"] as ProviderKind[]),
    [providers],
  );
  const [coordinator, setCoordinator] = useState<ProviderKind>("ollama");

  useEffect(() => {
    void loadWorkflows();
    void loadLocalTooling();
    void loadTemplates();
  }, []);

  useEffect(() => {
    if (!activeWorkflowId) {
      setDetail(null);
      return;
    }

    let cancelled = false;
    const poll = async () => {
      try {
        const next = await api.getWorkflow(activeWorkflowId);
        if (!cancelled) {
          setDetail(next);
        }
      } catch (err) {
        if (!cancelled) {
          setError(asError(err));
        }
      }
    };

    void poll();
    const interval = window.setInterval(() => {
      void poll();
      void loadWorkflows(false);
    }, 3500);

    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [activeWorkflowId]);

  useEffect(() => {
    if (!detail) {
      setTerminalBuffers({});
      return;
    }

    setTerminalBuffers({});
    const sources = detail.terminals.map((terminal) => {
      const source = new EventSource(`/api/terminals/${terminal.id}/stream`);
      source.onmessage = (event) => {
        const payload = safeParse<TerminalOutput>(event.data);
        if (!payload) return;
        setTerminalBuffers((previous) => ({
          ...previous,
          [terminal.id]: [...(previous[terminal.id] ?? []), payload.text],
        }));
      };
      return source;
    });

    return () => {
      for (const source of sources) {
        source.close();
      }
    };
  }, [detail?.workflow.id, detail?.terminals.length]);

  useEffect(() => {
    setApprovalsPage(0);
    setArtifactsPage(0);
    setHandoffsPage(0);
    setVerdictsPage(0);
    setEvidencePage(0);
    setSnapshotsPage(0);
  }, [detail?.workflow.id]);

  async function loadWorkflows(updateActive = true) {
    const next = await api.listWorkflows();
    setWorkflows(next);
    if (updateActive && !activeWorkflowId && next.length > 0) {
      setActiveWorkflowId(next[0].id);
    }
  }

  async function loadLocalTooling() {
    const [servers, models] = await Promise.all([
      api.listMcpServers(),
      api.listLlamaCppModels(),
    ]);
    setMcpServers(servers);
    setLlamaModels(models);
  }

  async function loadTemplates() {
    setTemplates(await api.listWorkflowTemplates());
  }

  async function createWorkflow() {
    if (!objective.trim()) return;
    setCreating(true);
    setError(null);
    try {
      const created = await api.createWorkflow({
        title: title.trim() || undefined,
        objective: objective.trim(),
        sensitivity,
        coordinator_provider: coordinator,
        template_key: templateKey,
        auto_start: true,
      });
      setWorkflows((previous) => [created.workflow, ...previous]);
      setActiveWorkflowId(created.workflow.id);
      setDetail(created);
      setTitle("");
      setObjective("");
      setSensitivity("internal");
      setTemplateKey("engineering_pipeline");
    } catch (err) {
      setError(asError(err));
    } finally {
      setCreating(false);
    }
  }

  async function decideApproval(approval: ApprovalGate, approved: boolean) {
    try {
      await api.decideApproval(approval.id, approved);
      if (activeWorkflowId) {
        setDetail(await api.getWorkflow(activeWorkflowId));
        await loadWorkflows(false);
      }
    } catch (err) {
      setError(asError(err));
    }
  }

  async function restartWorkflow() {
    if (!activeWorkflowId) return;
    try {
      setDetail(await api.startWorkflow(activeWorkflowId));
      await loadWorkflows(false);
    } catch (err) {
      setError(asError(err));
    }
  }

  async function reassignAgent(agentId: string, provider: ProviderKind) {
    try {
      const refreshed = await api.updateAgentProvider(agentId, provider);
      setDetail(refreshed);
      await loadWorkflows(false);
    } catch (err) {
      setError(asError(err));
    }
  }

  async function retryAgent(agentId: string) {
    try {
      const refreshed = await api.retryAgent(agentId);
      setDetail(refreshed);
      await loadWorkflows(false);
    } catch (err) {
      setError(asError(err));
    }
  }

  async function escalateAgent(agentId: string) {
    try {
      const refreshed = await api.escalateAgent(agentId, "Operator escalation from Agent Mode");
      setDetail(refreshed);
      await loadWorkflows(false);
    } catch (err) {
      setError(asError(err));
    }
  }

  async function createCheckpoint() {
    if (!activeWorkflowId) return;
    try {
      await api.createWorkflowSnapshot(activeWorkflowId, {
        label: snapshotLabel.trim() || "Manual checkpoint",
        snapshot_type: "checkpoint",
        rollback_target: true,
      });
      setSnapshotLabel("");
      setDetail(await api.getWorkflow(activeWorkflowId));
    } catch (err) {
      setError(asError(err));
    }
  }

  async function rollbackToSnapshot(snapshotId: string) {
    if (!activeWorkflowId) return;
    try {
      const refreshed = await api.rollbackWorkflow(activeWorkflowId, snapshotId);
      setDetail(refreshed);
      await loadWorkflows(false);
    } catch (err) {
      setError(asError(err));
    }
  }

  const selectedTemplate = useMemo(
    () => templates.find((template) => template.template_key === templateKey) ?? null,
    [templateKey, templates],
  );
  const visibleWorkflows = useMemo(
    () => paginateItems(workflows, workflowPage, 4),
    [workflowPage, workflows],
  );
  const visibleApprovals = useMemo(
    () => paginateItems(detail?.approvals ?? [], approvalsPage, 2),
    [approvalsPage, detail?.approvals],
  );
  const visibleArtifacts = useMemo(
    () => paginateItems(detail?.artifacts ?? [], artifactsPage, 2),
    [artifactsPage, detail?.artifacts],
  );
  const visibleHandoffs = useMemo(
    () => paginateItems((detail?.handoffs ?? []).slice().reverse(), handoffsPage, 3),
    [detail?.handoffs, handoffsPage],
  );
  const visibleVerdicts = useMemo(
    () =>
      paginateItems(
        [...(detail?.qa_verdicts ?? []), ...(detail?.release_verdicts ?? [])],
        verdictsPage,
        3,
      ),
    [detail?.qa_verdicts, detail?.release_verdicts, verdictsPage],
  );
  const visibleEvidence = useMemo(
    () => paginateItems((detail?.evidence ?? []).slice().reverse(), evidencePage, 3),
    [detail?.evidence, evidencePage],
  );
  const visibleSnapshots = useMemo(
    () => paginateItems(detail?.snapshots ?? [], snapshotsPage, 2),
    [detail?.snapshots, snapshotsPage],
  );

  async function saveMcpServer() {
    if (!mcpName.trim() || !mcpCommand.trim()) return;
    try {
      await api.upsertMcpServer({
        name: mcpName.trim(),
        command: mcpCommand.trim(),
        args: mcpArgs
          .split(" ")
          .map((value) => value.trim())
          .filter(Boolean),
        local_only: true,
        enabled: true,
        allowed_providers: ["ollama", "llama_cpp"],
      });
      setMcpName("");
      setMcpCommand("");
      setMcpArgs("");
      await loadLocalTooling();
    } catch (err) {
      setError(asError(err));
    }
  }

  async function toggleMcp(server: McpServer) {
    try {
      await api.setMcpServerEnabled(server.id, !server.enabled);
      await loadLocalTooling();
    } catch (err) {
      setError(asError(err));
    }
  }

  async function saveLlamaModel() {
    if (!llamaAlias.trim() || !llamaPath.trim()) return;
    try {
      await api.upsertLlamaCppModel({
        alias: llamaAlias.trim(),
        file_path: llamaPath.trim(),
        quantization: llamaQuant.trim() || null,
        context_window: Number.parseInt(llamaContext, 10) || null,
        enabled: true,
      });
      setLlamaAlias("");
      setLlamaPath("");
      setLlamaQuant("");
      setLlamaContext("8192");
      await loadLocalTooling();
    } catch (err) {
      setError(asError(err));
    }
  }

  async function toggleLlamaModel(model: LlamaCppModel) {
    try {
      await api.setLlamaCppModelEnabled(model.id, !model.enabled);
      await loadLocalTooling();
    } catch (err) {
      setError(asError(err));
    }
  }

  return (
    <section className="agents-screen">
      <section className="agents-sidebar card">
        <div className="agents-sidebar-head">
          <div>
            <span className="eyebrow">Agent mode</span>
            <h3>Coordinated workflows</h3>
            <p>Up to four background terminals with gated external providers.</p>
          </div>
          <button className="ghost" onClick={() => void loadWorkflows()}>
            Refresh
          </button>
        </div>

        <div className="workflow-create">
          <div className="form-row">
            <label>Title</label>
            <input
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              placeholder="Optional workflow title"
            />
          </div>

          <div className="form-row">
            <label>Objective</label>
            <textarea
              value={objective}
              onChange={(event) => setObjective(event.target.value)}
              rows={5}
              placeholder="Describe the larger task you want the agent team to break down and execute."
            />
          </div>

          <div className="workflow-create-grid">
            <div className="form-row">
              <label>Sensitivity</label>
              <select
                value={sensitivity}
                onChange={(event) => setSensitivity(event.target.value)}
              >
                <option value="public">public</option>
                <option value="internal">internal</option>
                <option value="sensitive">sensitive</option>
              </select>
            </div>
          </div>

          <div className="form-row">
            <label>Coordinator</label>
            <div className="option-grid">
              {coordinatorOptions.map((provider) => {
                const providerMeta = providers.find((item) => item.provider === provider);
                return (
                  <button
                    key={provider}
                    type="button"
                    className={
                      coordinator === provider
                        ? "option-card option-card-active"
                        : "option-card"
                    }
                    onClick={() => setCoordinator(provider)}
                  >
                    <strong>{labelProvider(provider)}</strong>
                    <span>
                      {providerMeta?.data_boundary === "external" ? "external" : "local-only"}
                    </span>
                    <span>
                      {providerMeta?.installed === false ? "not installed" : "available"}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>

          <div className="form-row">
            <label>Template</label>
            <div className="option-grid">
              {templates.map((template) => (
                <button
                  key={template.id}
                  type="button"
                  className={
                    templateKey === template.template_key
                      ? "option-card option-card-active"
                      : "option-card"
                  }
                  onClick={() => setTemplateKey(template.template_key)}
                >
                  <strong>{template.name}</strong>
                  <span>{template.template_key}</span>
                  <span>{template.phases.join(" -> ")}</span>
                </button>
              ))}
            </div>
          </div>

          {selectedTemplate ? <p className="muted-copy">{selectedTemplate.description}</p> : null}

          <button className="primary" disabled={creating} onClick={() => void createWorkflow()}>
            {creating ? "Creating..." : "Create workflow"}
          </button>
          {error ? <p className="agent-error">{error}</p> : null}
        </div>

        <div className="workflow-list">
          {visibleWorkflows.items.map((workflow) => (
            <button
              key={workflow.id}
              className={
                workflow.id === activeWorkflowId
                  ? "workflow-list-item active"
                  : "workflow-list-item"
              }
              onClick={() => setActiveWorkflowId(workflow.id)}
            >
              <div>
                <strong>{workflow.title}</strong>
                <p>{workflow.objective}</p>
                <p className="muted-copy">
                  {workflow.template_key} · {workflow.phase} · attempt {workflow.attempt_counter}
                </p>
              </div>
              <span className={`badge workflow-${workflow.status}`}>
                {workflow.status}
              </span>
            </button>
          ))}
        </div>
        <PaginationControls
          page={workflowPage}
          totalPages={visibleWorkflows.totalPages}
          onChange={setWorkflowPage}
        />

        <div className="tooling-panel">
          <div className="section-head">
            <div>
              <span className="eyebrow">Local tooling</span>
              <h4>MCP and llama.cpp</h4>
            </div>
          </div>

          <div className="tooling-block">
            <strong>MCP registry</strong>
            <div className="form-row">
              <label>Name</label>
              <input value={mcpName} onChange={(event) => setMcpName(event.target.value)} />
            </div>
            <div className="form-row">
              <label>Command</label>
              <input
                value={mcpCommand}
                onChange={(event) => setMcpCommand(event.target.value)}
                placeholder="npx or local binary"
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
            <button className="ghost" onClick={() => void saveMcpServer()}>
              Save MCP server
            </button>
            <div className="mini-list">
              {mcpServers.map((server) => (
                <button
                  key={server.id}
                  className="mini-list-item"
                  onClick={() => void toggleMcp(server)}
                >
                  <span>{server.name}</span>
                  <span>{server.enabled ? "enabled" : "disabled"}</span>
                </button>
              ))}
            </div>
          </div>

          <div className="tooling-block">
            <strong>llama.cpp models</strong>
            <div className="form-row">
              <label>Alias</label>
              <input
                value={llamaAlias}
                onChange={(event) => setLlamaAlias(event.target.value)}
                placeholder="llama-8b-q4"
              />
            </div>
            <div className="form-row">
              <label>GGUF path</label>
              <input
                value={llamaPath}
                onChange={(event) => setLlamaPath(event.target.value)}
                placeholder="relative to LLAMA_CPP_MODEL_DIR or absolute path"
              />
            </div>
            <div className="workflow-create-grid">
              <div className="form-row">
                <label>Quant</label>
                <input
                  value={llamaQuant}
                  onChange={(event) => setLlamaQuant(event.target.value)}
                  placeholder="Q4_K_M"
                />
              </div>
              <div className="form-row">
                <label>Context</label>
                <input
                  value={llamaContext}
                  onChange={(event) => setLlamaContext(event.target.value)}
                  placeholder="8192"
                />
              </div>
            </div>
            <button className="ghost" onClick={() => void saveLlamaModel()}>
              Save GGUF model
            </button>
            <div className="mini-list">
              {llamaModels.map((model) => (
                <button
                  key={model.id}
                  className="mini-list-item"
                  onClick={() => void toggleLlamaModel(model)}
                >
                  <span>{model.alias}</span>
                  <span>{model.enabled ? "enabled" : "disabled"}</span>
                </button>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section className="agents-main">
        {detail ? (
          <>
            <section className="agents-hero card">
              <div>
                <span className="eyebrow">Workflow</span>
                <h3>{detail.workflow.title}</h3>
                <p>{detail.workflow.objective}</p>
                <p className="muted-copy">
                  Template {detail.workflow.template_key} · phase {detail.workflow.phase} · gate{" "}
                  {detail.workflow.phase_gate_status}
                </p>
                {detail.workflow.next_action ? (
                  <p className="muted-copy">Next action: {detail.workflow.next_action}</p>
                ) : null}
                {detail.workflow.blocked_reason ? (
                  <p className="agent-error">Blocked: {detail.workflow.blocked_reason}</p>
                ) : null}
              </div>

              <div className="agents-hero-stats">
                <AgentMetric label="Coordinator" value={labelProvider(detail.workflow.coordinator_provider)} />
                <AgentMetric label="Sensitivity" value={detail.workflow.sensitivity} />
                <AgentMetric label="Status" value={detail.workflow.status} />
                <AgentMetric label="Attempts" value={String(detail.workflow.attempt_counter)} />
              </div>

              <div className="approval-actions">
                <button className="ghost" onClick={() => void restartWorkflow()}>
                  Re-run scheduler
                </button>
                <button className="ghost" onClick={() => void createCheckpoint()}>
                  Checkpoint
                </button>
              </div>
            </section>

            <section className="agents-grid">
              {detail.agents.map((agent) => {
                const terminal = detail.terminals.find((item) => item.agent_id === agent.id);
                const lines = terminal ? (terminalBuffers[terminal.id] ?? []).slice(-6) : [];
                const latestQa = detail.qa_verdicts.find((verdict) => verdict.agent_id === agent.id);
                return (
                  <article key={agent.id} className="agent-panel card">
                    <div className="agent-panel-head">
                      <div>
                        <span className="eyebrow">{agent.role}</span>
                        <h4>{agent.name}</h4>
                      </div>
                      <div className="agent-panel-tags">
                        <span className="chip chip-blue">{agent.status}</span>
                      </div>
                    </div>

                    <div className="form-row compact">
                      <label>Provider</label>
                      <select
                        value={agent.provider}
                        onChange={(event) =>
                          void reassignAgent(agent.id, event.target.value as ProviderKind)
                        }
                      >
                        <option value="ollama">Ollama</option>
                        <option value="llama_cpp">llama.cpp</option>
                        <option value="codex">Codex</option>
                        <option value="claude">Claude</option>
                      </select>
                    </div>

                    <p className="agent-task">{agent.current_task}</p>
                    <div className="agent-meta">
                      <span>Sensitivity: {agent.sensitivity}</span>
                      <span>Phase: {detail.workflow.phase}</span>
                      <span>QA: {latestQa?.verdict ?? "n/a"}</span>
                      <span>{terminal?.worktree_path ?? "workspace pending"}</span>
                    </div>

                    <div className="approval-actions">
                      <button className="ghost" onClick={() => void retryAgent(agent.id)}>
                        Retry
                      </button>
                      <button className="ghost" onClick={() => void escalateAgent(agent.id)}>
                        Escalate
                      </button>
                    </div>

                    <div className="terminal-screen">
                      <div className="terminal-command">{terminal?.command ?? "$ waiting for launch"}</div>
                      <pre>{lines.length > 0 ? lines.join("\n") : "No terminal output yet."}</pre>
                    </div>
                  </article>
                );
              })}
            </section>

            <section className="agents-dashboard-grid">
              <article className="card approvals-card">
                <div className="section-head">
                  <div>
                    <span className="eyebrow">Approval gates</span>
                    <h4>External provider approvals</h4>
                  </div>
                </div>

                {detail.approvals.length === 0 ? (
                  <p className="muted-copy">No approval gates for this workflow.</p>
                ) : (
                  <div className="approval-list">
                    {visibleApprovals.items.map((approval) => (
                      <div key={approval.id} className="approval-item">
                        <div>
                          <strong>{approval.reason}</strong>
                          <p>
                            {approval.target_provider
                              ? `${labelProvider(approval.target_provider)} · `
                              : ""}
                            {approval.status}
                          </p>
                        </div>

                        {approval.status === "pending" ? (
                          <div className="approval-actions">
                            <button
                              className="ghost"
                              onClick={() => void decideApproval(approval, false)}
                            >
                              Reject
                            </button>
                            <button
                              className="primary"
                              onClick={() => void decideApproval(approval, true)}
                            >
                              Approve
                            </button>
                          </div>
                        ) : null}
                      </div>
                    ))}
                  </div>
                )}
                <PaginationControls
                  page={approvalsPage}
                  totalPages={visibleApprovals.totalPages}
                  onChange={setApprovalsPage}
                />
              </article>

              <article className="card artifacts-card">
                <div className="section-head">
                  <div>
                    <span className="eyebrow">Artifacts</span>
                    <h4>Shared outputs</h4>
                  </div>
                </div>

                {detail.artifacts.length === 0 ? (
                  <p className="muted-copy">Artifacts will appear here once agents finish subtasks.</p>
                ) : (
                  <div className="artifact-list">
                    {visibleArtifacts.items.map((artifact) => (
                      <div key={artifact.id} className="artifact-item">
                        <div className="artifact-head">
                          <strong>{artifact.title}</strong>
                          <span className="chip">{artifact.sensitivity}</span>
                        </div>
                        <p>{artifact.content.slice(0, 260)}</p>
                      </div>
                    ))}
                  </div>
                )}
                <PaginationControls
                  page={artifactsPage}
                  totalPages={visibleArtifacts.totalPages}
                  onChange={setArtifactsPage}
                />
              </article>

              <article className="card approvals-card">
                <div className="section-head">
                  <div>
                    <span className="eyebrow">Pipeline</span>
                    <h4>Handoffs and QA</h4>
                  </div>
                </div>

                <div className="artifact-list">
                  {visibleHandoffs.items.map((handoff) => (
                    <div key={handoff.id} className="artifact-item">
                      <div className="artifact-head">
                        <strong>{handoff.handoff_type}</strong>
                        <span className="chip">{handoff.phase}</span>
                      </div>
                      <p>{handoff.context_summary}</p>
                      <p className="muted-copy">
                        {handoff.task_ref} · {handoff.status} · {handoff.priority}
                      </p>
                    </div>
                  ))}
                  {detail.handoffs.length === 0 ? (
                    <p className="muted-copy">No handoffs recorded yet.</p>
                  ) : null}
                </div>
                <PaginationControls
                  page={handoffsPage}
                  totalPages={visibleHandoffs.totalPages}
                  onChange={setHandoffsPage}
                />

                <div className="artifact-list">
                  {visibleVerdicts.items.map((verdict) => (
                    <div key={verdict.id} className="artifact-item">
                      <div className="artifact-head">
                        <strong>{"attempt_number" in verdict ? "QA verdict" : "Release verdict"}</strong>
                        <span className="chip">{verdict.verdict}</span>
                      </div>
                      <p>{verdict.summary}</p>
                      <p className="muted-copy">
                        {verdict.phase}
                        {"attempt_number" in verdict ? ` · attempt ${verdict.attempt_number}` : ""}
                      </p>
                    </div>
                  ))}
                  {detail.qa_verdicts.length === 0 && detail.release_verdicts.length === 0 ? (
                    <p className="muted-copy">No QA or release verdicts yet.</p>
                  ) : null}
                </div>
                <PaginationControls
                  page={verdictsPage}
                  totalPages={visibleVerdicts.totalPages}
                  onChange={setVerdictsPage}
                />
              </article>

              <article className="card artifacts-card">
                <div className="section-head">
                  <div>
                    <span className="eyebrow">Audit</span>
                    <h4>Evidence chain and rollback</h4>
                  </div>
                </div>

                <div className="form-row compact">
                  <label>Checkpoint label</label>
                  <input
                    value={snapshotLabel}
                    onChange={(event) => setSnapshotLabel(event.target.value)}
                    placeholder="Manual checkpoint"
                  />
                </div>

                <div className="artifact-list">
                  {visibleEvidence.items.map((record) => (
                    <div key={record.id} className="artifact-item">
                      <div className="artifact-head">
                        <strong>{record.event_type}</strong>
                        <span className="chip">{record.actor_type}</span>
                      </div>
                      <p className="muted-copy">
                        hash {record.record_hash.slice(0, 12)}
                        {record.prev_hash ? ` <- ${record.prev_hash.slice(0, 12)}` : ""}
                      </p>
                    </div>
                  ))}
                  {detail.evidence.length === 0 ? (
                    <p className="muted-copy">No evidence records yet.</p>
                  ) : null}
                </div>
                <PaginationControls
                  page={evidencePage}
                  totalPages={visibleEvidence.totalPages}
                  onChange={setEvidencePage}
                />

                <div className="artifact-list">
                  {visibleSnapshots.items.map((snapshot) => (
                    <div key={snapshot.id} className="artifact-item">
                      <div className="artifact-head">
                        <strong>{snapshot.label}</strong>
                        <span className="chip">{snapshot.snapshot_type}</span>
                      </div>
                      <p className="muted-copy">{new Date(snapshot.created_at).toLocaleString()}</p>
                      <button className="ghost" onClick={() => void rollbackToSnapshot(snapshot.id)}>
                        Roll back
                      </button>
                    </div>
                  ))}
                  {detail.snapshots.length === 0 ? (
                    <p className="muted-copy">No checkpoints yet.</p>
                  ) : null}
                </div>
                <PaginationControls
                  page={snapshotsPage}
                  totalPages={visibleSnapshots.totalPages}
                  onChange={setSnapshotsPage}
                />
              </article>
            </section>
          </>
        ) : (
          <section className="card agent-empty-state">
            <span className="eyebrow">Agent mode</span>
            <h3>Create a workflow to spin up the first team of agents</h3>
            <p>
              Planner, QA, and release gating stay local by default. External research or coding
              providers require approval on non-public work before data can leave the host.
            </p>
          </section>
        )}
      </section>
    </section>
  );
}

function AgentMetric({ label, value }: { label: string; value: string }) {
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

function PaginationControls({
  page,
  totalPages,
  onChange,
}: {
  page: number;
  totalPages: number;
  onChange: (page: number) => void;
}) {
  if (totalPages <= 1) return null;
  return (
    <div className="pagination-row">
      <button className="ghost" onClick={() => onChange(Math.max(0, page - 1))} disabled={page === 0}>
        Prev
      </button>
      <span>
        {page + 1}/{totalPages}
      </span>
      <button
        className="ghost"
        onClick={() => onChange(Math.min(totalPages - 1, page + 1))}
        disabled={page >= totalPages - 1}
      >
        Next
      </button>
    </div>
  );
}

function asError(value: unknown): string {
  return value instanceof Error ? value.message : "Unknown error";
}

function safeParse<T>(value: string): T | null {
  try {
    return JSON.parse(value) as T;
  } catch {
    return null;
  }
}

function paginateItems<T>(items: T[], page: number, pageSize: number) {
  const totalPages = Math.max(1, Math.ceil(items.length / pageSize));
  const safePage = Math.min(page, totalPages - 1);
  const start = safePage * pageSize;
  return {
    items: items.slice(start, start + pageSize),
    totalPages,
  };
}
