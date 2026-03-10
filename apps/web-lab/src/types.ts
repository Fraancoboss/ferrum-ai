export type ProviderKind = "codex" | "claude" | "ollama" | "llama_cpp";

export type ProviderView = {
  provider: ProviderKind;
  display_name: string;
  installed: boolean;
  version: string | null;
  auth_status: string;
  auth_required: boolean;
  data_boundary: string;
  detail: string | null;
  issues: string[];
  selected_model: string | null;
  selected_effort: string | null;
};

export type ProviderPreferences = {
  provider: ProviderKind;
  model: string | null;
  effort: string | null;
};

export type ChatSummary = {
  id: string;
  provider: ProviderKind;
  title: string;
  provider_session_ref: string | null;
  last_model: string | null;
  created_at: string;
  last_message_at: string | null;
};

export type ChatMessage = {
  id: string;
  session_id: string;
  role: "user" | "assistant" | "system" | string;
  content: string;
  created_at: string;
  source_run_id: string | null;
  usage: {
    model?: string;
    input_tokens?: number;
    output_tokens?: number;
    total_tokens?: number;
    estimated_cost_usd?: number;
  } | null;
};

export type UsageRow = {
  provider: ProviderKind;
  day: string;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
};

export type UsageLimit = {
  provider: ProviderKind;
  soft_limit_tokens: number | null;
  used_today_tokens: number;
};

export type UsageSummary = {
  daily: UsageRow[];
  limits: UsageLimit[];
};

export type RunLaunchResponse = {
  run_id: string;
};

export type AuthLaunchResponse = {
  auth_id: string;
};

export type StreamEvent = {
  event_kind:
    | "run_started"
    | "assistant_delta"
    | "assistant_final"
    | "usage_updated"
    | "provider_session_bound"
    | "auth_output"
    | "auth_url"
    | "stderr"
    | "run_completed"
    | "run_failed";
  text?: string;
  usage?: ChatMessage["usage"];
  provider_session_ref?: string | null;
  [key: string]: unknown;
};

export type WorkflowSummary = {
  id: string;
  title: string;
  objective: string;
  coordinator_provider: ProviderKind;
  sensitivity: string;
  status: string;
  template_key: string;
  phase: string;
  phase_gate_status: string;
  current_task_id: string | null;
  attempt_counter: number;
  next_action: string | null;
  blocked_reason: string | null;
  created_at: string;
  updated_at: string;
};

export type WorkflowAgent = {
  id: string;
  workflow_id: string;
  name: string;
  role: string;
  provider: ProviderKind;
  status: string;
  current_task: string;
  task_fingerprint: string;
  dependency_ids: string[];
  worktree_path: string | null;
  sensitivity: string;
  approval_required: boolean;
  created_at: string;
  updated_at: string;
};

export type TerminalSession = {
  id: string;
  workflow_id: string;
  agent_id: string;
  title: string;
  provider: ProviderKind;
  status: string;
  command: string | null;
  worktree_path: string | null;
  created_at: string;
  updated_at: string;
  finished_at: string | null;
};

export type TerminalOutput = {
  terminal_session_id: string;
  sequence: number;
  text: string;
  created_at: string;
};

export type ApprovalGate = {
  id: string;
  workflow_id: string;
  agent_id: string | null;
  gate_type: string;
  target_provider: ProviderKind | null;
  status: string;
  reason: string;
  requested_context: Record<string, unknown>;
  created_at: string;
  resolved_at: string | null;
};

export type WorkflowArtifact = {
  id: string;
  workflow_id: string;
  agent_id: string | null;
  title: string;
  kind: string;
  content: string;
  fingerprint: string;
  sensitivity: string;
  reusable: boolean;
  created_at: string;
};

export type WorkflowDetail = {
  workflow: WorkflowSummary;
  agents: WorkflowAgent[];
  terminals: TerminalSession[];
  approvals: ApprovalGate[];
  artifacts: WorkflowArtifact[];
  handoffs: WorkflowHandoff[];
  qa_verdicts: WorkflowQaVerdict[];
  release_verdicts: WorkflowReleaseVerdict[];
  evidence: WorkflowEvidenceRecord[];
  snapshots: WorkflowSnapshot[];
};

export type WorkflowTemplate = {
  id: string;
  template_key: string;
  name: string;
  description: string;
  phases: string[];
  default_agent_roles: string[];
  created_at: string;
  updated_at: string;
};

export type WorkflowHandoff = {
  id: string;
  workflow_id: string;
  from_agent_id: string | null;
  to_agent_id: string | null;
  phase: string;
  handoff_type: string;
  task_ref: string;
  priority: string;
  context_summary: string;
  relevant_artifact_ids: string[];
  dependencies: string[];
  constraints: string[];
  deliverable_request: string;
  acceptance_criteria: string[];
  evidence_required: string[];
  status: string;
  created_at: string;
  resolved_at: string | null;
};

export type WorkflowQaVerdict = {
  id: string;
  workflow_id: string;
  agent_id: string | null;
  phase: string;
  verdict: string;
  summary: string;
  findings: string[];
  evidence_artifact_ids: string[];
  attempt_number: number;
  created_at: string;
};

export type WorkflowReleaseVerdict = {
  id: string;
  workflow_id: string;
  agent_id: string | null;
  phase: string;
  verdict: string;
  summary: string;
  findings: string[];
  evidence_artifact_ids: string[];
  created_at: string;
};

export type WorkflowEvidenceRecord = {
  id: string;
  workflow_id: string;
  actor_type: string;
  actor_id: string | null;
  event_type: string;
  payload: Record<string, unknown>;
  prev_hash: string | null;
  record_hash: string;
  created_at: string;
};

export type WorkflowSnapshot = {
  id: string;
  workflow_id: string;
  agent_id: string | null;
  snapshot_type: string;
  label: string;
  payload: Record<string, unknown>;
  rollback_target: boolean;
  created_at: string;
};

export type WorkflowQaStatus = {
  qa_verdicts: WorkflowQaVerdict[];
  release_verdicts: WorkflowReleaseVerdict[];
};

export type McpServer = {
  id: string;
  name: string;
  command: string;
  args: string[];
  local_only: boolean;
  enabled: boolean;
  allowed_providers: ProviderKind[];
  created_at: string;
  updated_at: string;
};

export type LlamaCppModel = {
  id: string;
  alias: string;
  file_path: string;
  context_window: number | null;
  quantization: string | null;
  enabled: boolean;
  created_at: string;
  updated_at: string;
};
