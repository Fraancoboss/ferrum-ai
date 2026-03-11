import type {
  AuthLaunchResponse,
  ChatMessage,
  ChatSummary,
  DeviceHardwareProfile,
  ApprovalGate,
  LlamaCppModel,
  LocalModelCatalogEntry,
  LocalModelInventoryView,
  McpServer,
  ModelInstallJob,
  ProviderKind,
  ProviderPreferences,
  ProviderView,
  ProvidersGovernanceView,
  ProvidersHardwareView,
  RunLaunchResponse,
  SkillDetail,
  SkillAssignment,
  SkillAssignmentTargets,
  SkillSummary,
  WorkflowDetail,
  WorkflowEvidenceRecord,
  WorkflowHandoff,
  WorkflowQaStatus,
  WorkflowSnapshot,
  WorkflowTemplate,
  WorkflowSummary,
  UsageSummary,
} from "./types";

async function request<T>(url: string, options: RequestInit = {}): Promise<T> {
  const response = await fetch(url, {
    headers: {
      "Content-Type": "application/json",
    },
    ...options,
  });

  if (!response.ok) {
    let body: unknown = null;
    try {
      body = await response.json();
    } catch {
      body = null;
    }
    const message =
      body && typeof body === "object" && "error" in body
        ? String((body as Record<string, unknown>).error)
        : `Request failed: ${response.status}`;
    throw new Error(message);
  }

  if (response.status === 204) {
    return null as T;
  }
  return (await response.json()) as T;
}

export const api = {
  health: () => request<{ status: string }>("/api/health"),
  listProviders: () => request<ProviderView[]>("/api/providers"),
  getProvidersHardware: () => request<ProvidersHardwareView>("/api/providers/hardware"),
  getProvidersGovernance: () =>
    request<ProvidersGovernanceView>("/api/providers/governance"),
  saveBrowserHardwareSnapshot: (payload: {
    platform?: string | null;
    cpu_brand?: string | null;
    logical_cores?: number | null;
    device_memory_gb?: number | null;
    total_memory_gb?: number | null;
    available_memory_gb?: number | null;
    gpu_vendor?: string | null;
    gpu_renderer?: string | null;
    user_agent?: string | null;
  }) =>
    request<DeviceHardwareProfile>("/api/providers/hardware/browser-snapshot", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  getProviderPreferences: (provider: ProviderKind) =>
    request<ProviderPreferences>(`/api/providers/${provider}/preferences`),
  updateProviderPreferences: (
    provider: ProviderKind,
    payload: { model?: string | null; effort?: string | null },
  ) =>
    request<ProviderPreferences>(`/api/providers/${provider}/preferences`, {
      method: "PUT",
      body: JSON.stringify(payload),
    }),
  loginProvider: (provider: ProviderKind) =>
    request<AuthLaunchResponse>(`/api/providers/${provider}/login`, { method: "POST" }),
  logoutProvider: (provider: ProviderKind) =>
    request<AuthLaunchResponse>(`/api/providers/${provider}/logout`, { method: "POST" }),
  listChats: () => request<ChatSummary[]>("/api/chats"),
  createChat: (payload: { provider: ProviderKind; title?: string }) =>
    request<ChatSummary>("/api/chats", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  listWorkflows: () => request<WorkflowSummary[]>("/api/workflows"),
  createWorkflow: (payload: {
    title?: string;
    objective: string;
    sensitivity?: string;
    coordinator_provider?: ProviderKind;
    template_key?: string;
    auto_start?: boolean;
  }) =>
    request<WorkflowDetail>("/api/workflows", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  getWorkflow: (workflowId: string) =>
    request<WorkflowDetail>(`/api/workflows/${workflowId}`),
  listWorkflowTemplates: () => request<WorkflowTemplate[]>("/api/workflow-templates"),
  listWorkflowHandoffs: (workflowId: string) =>
    request<WorkflowHandoff[]>(`/api/workflows/${workflowId}/handoffs`),
  listWorkflowEvidence: (workflowId: string) =>
    request<WorkflowEvidenceRecord[]>(`/api/workflows/${workflowId}/evidence`),
  getWorkflowQaStatus: (workflowId: string) =>
    request<WorkflowQaStatus>(`/api/workflows/${workflowId}/qa-status`),
  listWorkflowSnapshots: (workflowId: string) =>
    request<WorkflowSnapshot[]>(`/api/workflows/${workflowId}/snapshots`),
  createWorkflowSnapshot: (
    workflowId: string,
    payload: {
      agent_id?: string | null;
      snapshot_type?: string;
      label?: string;
      rollback_target?: boolean;
    },
  ) =>
    request<WorkflowSnapshot>(`/api/workflows/${workflowId}/snapshots`, {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  rollbackWorkflow: (workflowId: string, snapshot_id: string) =>
    request<WorkflowDetail>(`/api/workflows/${workflowId}/rollback`, {
      method: "POST",
      body: JSON.stringify({ snapshot_id }),
    }),
  startWorkflow: (workflowId: string) =>
    request<WorkflowDetail>(`/api/workflows/${workflowId}/start`, {
      method: "POST",
    }),
  updateAgentProvider: (agentId: string, provider: ProviderKind) =>
    request<WorkflowDetail>(`/api/agents/${agentId}/provider`, {
      method: "POST",
      body: JSON.stringify({ provider }),
    }),
  retryAgent: (agentId: string) =>
    request<WorkflowDetail>(`/api/agents/${agentId}/retry`, {
      method: "POST",
    }),
  escalateAgent: (agentId: string, reason?: string) =>
    request<WorkflowDetail>(`/api/agents/${agentId}/escalate`, {
      method: "POST",
      body: JSON.stringify({ reason }),
    }),
  decideApproval: (approvalId: string, approved: boolean) =>
    request<ApprovalGate>(`/api/approvals/${approvalId}`, {
      method: "POST",
      body: JSON.stringify({ approved }),
    }),
  listMcpServers: () => request<McpServer[]>("/api/mcp/servers"),
  upsertMcpServer: (payload: {
    name: string;
    command: string;
    args?: string[];
    local_only?: boolean;
    enabled?: boolean;
    allowed_providers?: ProviderKind[];
  }) =>
    request<McpServer>("/api/mcp/servers", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  setMcpServerEnabled: (serverId: string, enabled: boolean) =>
    request<McpServer>(`/api/mcp/servers/${serverId}`, {
      method: "POST",
      body: JSON.stringify({ enabled }),
    }),
  listLlamaCppModels: () => request<LlamaCppModel[]>("/api/llama-cpp/models"),
  upsertLlamaCppModel: (payload: {
    alias: string;
    file_path: string;
    context_window?: number | null;
    quantization?: string | null;
    enabled?: boolean;
  }) =>
    request<LlamaCppModel>("/api/llama-cpp/models", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  setLlamaCppModelEnabled: (modelId: string, enabled: boolean) =>
    request<LlamaCppModel>(`/api/llama-cpp/models/${modelId}`, {
      method: "POST",
      body: JSON.stringify({ enabled }),
    }),
  listLocalModelCatalog: (filters?: { objective?: string; runtime?: string }) => {
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(filters ?? {})) {
      if (value) params.set(key, value);
    }
    const suffix = params.toString();
    return request<LocalModelCatalogEntry[]>(
      suffix ? `/api/local-models/catalog?${suffix}` : "/api/local-models/catalog",
    );
  },
  listLocalModelsInstalled: () =>
    request<LocalModelInventoryView>("/api/local-models/installed"),
  listLocalModelInstallJobs: () =>
    request<ModelInstallJob[]>("/api/local-models/install-jobs"),
  installOllamaCatalogModel: (payload: {
    catalog_key: string;
    actor_name?: string;
    source_app?: string;
    source_channel?: string;
  }) =>
    request<ModelInstallJob>("/api/local-models/ollama/install", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  importLocalGguf: (payload: {
    alias: string;
    file_path: string;
    context_window?: number | null;
    quantization?: string | null;
    actor_name?: string;
    source_app?: string;
    source_channel?: string;
  }) =>
    request<{ model: LlamaCppModel; job: ModelInstallJob }>("/api/local-models/gguf/import", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  listSkills: (filters?: {
    tenant_key?: string;
    skill_type?: string;
    status?: string;
    tag?: string;
    owner?: string;
    sensitivity?: string;
  }) => {
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(filters ?? {})) {
      if (value) params.set(key, value);
    }
    const suffix = params.toString();
    return request<SkillSummary[]>(suffix ? `/api/skills?${suffix}` : "/api/skills");
  },
  createSkill: (payload: {
    tenant_key?: string;
    slug: string;
    name: string;
    skill_type: string;
    description: string;
    owner?: string;
    visibility?: string;
    tags?: string[];
    allowed_sensitivity_levels?: string[];
    provider_exposure?: string;
    source_kind?: string;
    initial_version: {
      summary: string;
      body: Record<string, unknown>;
      examples?: string[];
      constraints?: string[];
      review_notes?: string | null;
      created_by?: string;
      source_ref?: string | null;
      dataset_pack_key?: string | null;
    };
  }) =>
    request<SkillDetail>("/api/skills", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  getSkill: (skillId: string) => request<SkillDetail>(`/api/skills/${skillId}`),
  listSkillVersions: (skillId: string) => request<SkillDetail["versions"]>(`/api/skills/${skillId}/versions`),
  listSkillAssignments: (skillId: string) =>
    request<SkillAssignment[]>(`/api/skills/${skillId}/assignments`),
  createSkillVersion: (
    skillId: string,
    payload: {
      summary: string;
      body: Record<string, unknown>;
      examples?: string[];
      constraints?: string[];
      review_notes?: string | null;
      created_by?: string;
      source_ref?: string | null;
      dataset_pack_key?: string | null;
    },
  ) =>
    request<SkillDetail>(`/api/skills/${skillId}/versions`, {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  skillAssignmentTargets: () =>
    request<SkillAssignmentTargets>("/api/skill-assignments/targets"),
  createSkillAssignment: (
    skillId: string,
    payload: {
      skill_version_id: string;
      target_type: string;
      target_key: string;
    },
  ) =>
    request<SkillDetail>(`/api/skills/${skillId}/assignments`, {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  deleteSkillAssignment: (assignmentId: string) =>
    request<SkillDetail>(`/api/skill-assignments/${assignmentId}`, {
      method: "POST",
      body: JSON.stringify({}),
    }),
  submitSkillVersionForReview: (versionId: string, payload?: { actor?: string; comment?: string }) =>
    request<SkillDetail>(`/api/skill-versions/${versionId}/submit-review`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    }),
  approveSkillVersion: (versionId: string, payload?: { actor?: string; comment?: string }) =>
    request<SkillDetail>(`/api/skill-versions/${versionId}/approve`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    }),
  publishSkillVersion: (versionId: string, payload?: { actor?: string; comment?: string }) =>
    request<SkillDetail>(`/api/skill-versions/${versionId}/publish`, {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    }),
  getChatMessages: (chatId: string) => request<ChatMessage[]>(`/api/chats/${chatId}/messages`),
  sendMessage: (chatId: string, payload: { content: string }) =>
    request<RunLaunchResponse>(`/api/chats/${chatId}/messages`, {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  usageSummary: () => request<UsageSummary>("/api/usage/summary"),
};
