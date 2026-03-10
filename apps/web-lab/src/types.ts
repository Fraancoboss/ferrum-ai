export type ProviderKind = "codex" | "claude";

export type ProviderView = {
  provider: ProviderKind;
  display_name: string;
  installed: boolean;
  version: string | null;
  auth_status: string;
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
