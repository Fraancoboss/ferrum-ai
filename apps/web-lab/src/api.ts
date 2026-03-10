import type {
  AuthLaunchResponse,
  ChatMessage,
  ChatSummary,
  ProviderKind,
  ProviderPreferences,
  ProviderView,
  RunLaunchResponse,
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
  getChatMessages: (chatId: string) => request<ChatMessage[]>(`/api/chats/${chatId}/messages`),
  sendMessage: (chatId: string, payload: { content: string }) =>
    request<RunLaunchResponse>(`/api/chats/${chatId}/messages`, {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  usageSummary: () => request<UsageSummary>("/api/usage/summary"),
};

