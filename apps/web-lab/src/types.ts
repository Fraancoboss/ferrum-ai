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
  resolved_skills: ResolvedAgentSkill[];
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

export type DeviceHardwareProfile = {
  source: "host" | "browser" | string;
  platform: string | null;
  cpu_brand: string | null;
  logical_cores: number | null;
  total_memory_gb: number | null;
  available_memory_gb: number | null;
  device_memory_gb: number | null;
  gpu_vendor: string | null;
  gpu_renderer: string | null;
  user_agent: string | null;
  updated_at: string;
};

export type ProvidersHardwareView = {
  authority: "host" | string;
  host: DeviceHardwareProfile;
  browser: DeviceHardwareProfile | null;
};

export type GovernanceProviderStatus = {
  provider: ProviderKind;
  display_name: string;
  installed: boolean;
  auth_status: string;
  detail: string | null;
  issues: string[];
  version: string | null;
  status_label: "healthy" | "attention" | "missing" | string;
};

export type ProvidersGovernanceView = {
  authority: "host" | string;
  ollama_api_base: string;
  ollama_runtime_mode: "host" | "docker" | "endpoint_only" | string;
  host: DeviceHardwareProfile;
  browser: DeviceHardwareProfile | null;
  local_providers: GovernanceProviderStatus[];
  inventory_issue_count: number;
  inventory_issues: string[];
  recent_jobs: ModelInstallJob[];
  last_refresh_at: string;
};

export type InstalledLocalModel = {
  runtime_target: "ollama" | "llama_cpp" | string;
  model_ref: string;
  display_name: string;
  alias: string | null;
  enabled: boolean;
  context_window: number | null;
  quantization: string | null;
  file_path: string | null;
  installed_from_catalog: string | null;
};

export type LocalModelInventoryView = {
  ollama: InstalledLocalModel[];
  gguf: InstalledLocalModel[];
  issues: string[];
};

export type LocalModelCatalogEntry = {
  key: string;
  runtime_target: "ollama" | "llama_cpp" | string;
  model_ref: string;
  display_name: string;
  family: string;
  summary: string;
  objectives: string[];
  modality: string;
  parameter_size_b: number;
  artifact_size_gb: number;
  context_window: number;
  quantization: string | null;
  memory_min_gb: number;
  memory_recommended_gb: number;
  install_policy: string;
  benchmark_coding: number | null;
  benchmark_reasoning: number | null;
  benchmark_vision: number | null;
  source_label: string;
  policy_state: "approved_for_install" | "visible_but_blocked" | "already_installed" | string;
  recommendation_band:
    | "recommended"
    | "possible_with_tradeoffs"
    | "visible_but_blocked"
    | "not_recommended"
    | string;
  fit_reason: string;
  installed: boolean;
};

export type ModelInstallJob = {
  id: string;
  actor_name: string;
  source_app: string;
  source_channel: string;
  runtime_target: string;
  catalog_key: string | null;
  source_ref: string | null;
  checksum_expected: string | null;
  checksum_actual: string | null;
  destination_ref: string | null;
  status: string;
  progress_percent: number;
  detail: string | null;
  error_text: string | null;
  created_at: string;
  updated_at: string;
  finished_at: string | null;
};

export type SkillType = "library" | "agent-context" | "cli" | "provider" | "policy";

export type SkillProviderExposure =
  | "local_only"
  | "agent_context_only"
  | "provider_allowed";

export type SkillVersionStatus =
  | "draft"
  | "review"
  | "approved"
  | "published"
  | "retired";

export type SkillSummary = {
  id: string;
  tenant_key: string;
  slug: string;
  name: string;
  skill_type: SkillType | string;
  description: string;
  status: string;
  owner: string;
  visibility: string;
  tags: string[];
  allowed_sensitivity_levels: string[];
  provider_exposure: SkillProviderExposure | string;
  source_kind: string;
  assignment_count: number;
  latest_version: number | null;
  latest_version_status: SkillVersionStatus | string | null;
  latest_version_summary: string | null;
  latest_version_updated_at: string | null;
  created_at: string;
  updated_at: string;
};

export type SkillVersion = {
  id: string;
  skill_id: string;
  version: number;
  status: SkillVersionStatus | string;
  body: Record<string, unknown>;
  summary: string;
  examples: string[];
  constraints: string[];
  review_notes: string | null;
  created_by: string;
  approved_by: string | null;
  published_by: string | null;
  source_ref: string | null;
  dataset_pack_key: string | null;
  created_at: string;
  updated_at: string;
  approved_at: string | null;
  published_at: string | null;
};

export type SkillReviewEvent = {
  id: string;
  skill_version_id: string;
  skill_id: string;
  action: string;
  actor_role: string;
  actor_name: string;
  comment: string | null;
  created_at: string;
};

export type SkillDetail = {
  skill: SkillSummary;
  versions: SkillVersion[];
  reviews: SkillReviewEvent[];
  assignments: SkillAssignment[];
};

export type SkillAssignment = {
  id: string;
  skill_version_id: string;
  skill_id: string;
  skill_version: number;
  target_type: "workflow_template" | "agent_role" | "provider" | string;
  target_key: string;
  created_at: string;
};

export type SkillAssignmentTargets = {
  workflow_templates: string[];
  agent_roles: string[];
  providers: ProviderKind[] | string[];
};

export type ResolvedAgentSkill = {
  agent_id: string;
  skill_id: string;
  skill_version_id: string;
  skill_name: string;
  skill_version: number;
  skill_type: SkillType | string;
  provider_exposure: SkillProviderExposure | string;
  source_target_type: string;
  source_target_key: string;
  resolution_order: number;
  applies_to_local_prompt: boolean;
  applies_to_external_context: boolean;
};
