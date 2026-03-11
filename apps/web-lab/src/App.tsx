import {
  Fragment,
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
  type ChangeEvent,
  type KeyboardEvent,
} from "react";

import { AgentMode } from "./AgentMode";
import { api } from "./api";
import { McpRegistryScreen } from "./McpRegistryScreen";
import { ProvidersControlPlane } from "./ProvidersControlPlane";
import { SkillsScreen } from "./SkillsScreen";
import type {
  AuthLaunchResponse,
  ChatMessage,
  ChatSummary,
  LlamaCppModel,
  ProviderKind,
  ProviderPreferences,
  ProviderView,
  StreamEvent,
  UsageSummary,
} from "./types";

type Page = "providers" | "agents" | "skills" | "mcps" | "chats" | "chat";

type MarkdownBlock =
  | { type: "heading"; level: 1 | 2 | 3 | 4; text: string }
  | { type: "paragraph"; text: string }
  | { type: "unordered-list"; items: string[] }
  | { type: "ordered-list"; items: string[] }
  | { type: "code"; text: string };

type ComposerAttachment = {
  id: string;
  name: string;
  mime: string;
  size: number;
  inlineContent: string | null;
};

const PAGE_SIZE = 10;
const EFFORT_OPTIONS = ["low", "medium", "high", "xhigh"] as const;
const MODEL_OPTIONS: Record<ProviderKind, string[]> = {
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

export function App() {
  const [page, setPage] = useState<Page>("chats");
  const [sidebarOpen, setSidebarOpen] = useState(true);

  const [providers, setProviders] = useState<ProviderView[]>([]);
  const [llamaModels, setLlamaModels] = useState<LlamaCppModel[]>([]);
  const [usage, setUsage] = useState<UsageSummary>({ daily: [], limits: [] });
  const [chats, setChats] = useState<ChatSummary[]>([]);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [activeChatId, setActiveChatId] = useState<string | null>(null);

  const [composer, setComposer] = useState("");
  const [attachments, setAttachments] = useState<ComposerAttachment[]>([]);
  const [sending, setSending] = useState(false);
  const [pendingText, setPendingText] = useState("");
  const [runUsage, setRunUsage] = useState<ChatMessage["usage"]>(null);
  const [status, setStatus] = useState("booting");
  const [error, setError] = useState<string | null>(null);

  const [runLog, setRunLog] = useState<string[]>(["Waiting for run events..."]);
  const [authLog, setAuthLog] = useState<string[]>(["Waiting for auth events..."]);
  const [runPanelOpen, setRunPanelOpen] = useState(false);
  const [authPanelOpen, setAuthPanelOpen] = useState(false);

  const [chatPage, setChatPage] = useState(1);
  const [providerFilter, setProviderFilter] = useState<ProviderKind | "all">("all");
  const [modelFilter, setModelFilter] = useState("all");
  const [keywordFilter, setKeywordFilter] = useState("");

  const [newChatOpen, setNewChatOpen] = useState(false);
  const [newChatProvider, setNewChatProvider] = useState<ProviderKind>("codex");
  const [newChatModel, setNewChatModel] = useState(MODEL_OPTIONS.codex[0]);
  const [newChatEffort, setNewChatEffort] = useState<(typeof EFFORT_OPTIONS)[number]>("high");
  const [newChatTitle, setNewChatTitle] = useState("");

  useEffect(() => {
    let alive = true;

    void (async () => {
      try {
        await api.health();
        if (!alive) return;

        await Promise.all([reloadProviders(), reloadChats(), reloadUsage(), reloadLlamaModels()]);
        if (!alive) return;

        setStatus("ready");
      } catch (err) {
        if (!alive) return;
        setError(asError(err));
        setStatus("offline");
      }
    })();

    return () => {
      alive = false;
    };
  }, []);

  const deferredKeywordFilter = useDeferredValue(keywordFilter);

  const activeChat = useMemo(
    () => chats.find((chat) => chat.id === activeChatId) ?? null,
    [activeChatId, chats],
  );

  const filteredChats = useMemo(() => {
    const keyword = deferredKeywordFilter.trim().toLowerCase();

    return chats.filter((chat) => {
      const providerMatches =
        providerFilter === "all" || chat.provider === providerFilter;
      const modelMatches =
        modelFilter === "all" || (chat.last_model ?? "unknown") === modelFilter;
      const keywordMatches =
        keyword.length === 0 ||
        chat.title.toLowerCase().includes(keyword) ||
        (chat.last_model ?? "").toLowerCase().includes(keyword) ||
        chat.id.toLowerCase().includes(keyword);

      return providerMatches && modelMatches && keywordMatches;
    });
  }, [chats, deferredKeywordFilter, modelFilter, providerFilter]);

  const totalChatPages = Math.max(1, Math.ceil(filteredChats.length / PAGE_SIZE));

  const paginatedChats = useMemo(() => {
    const start = (chatPage - 1) * PAGE_SIZE;
    return filteredChats.slice(start, start + PAGE_SIZE);
  }, [chatPage, filteredChats]);

  const knownModels = useMemo(() => {
    const modelsFromChats = chats
      .map((chat) => chat.last_model)
      .filter((value): value is string => Boolean(value));

    return Array.from(
      new Set([
        ...modelsFromChats,
        ...getModelOptions("codex", llamaModels),
        ...getModelOptions("claude", llamaModels),
        ...getModelOptions("ollama", llamaModels),
        ...getModelOptions("llama_cpp", llamaModels),
      ]),
    );
  }, [chats, llamaModels]);

  const timeline = useMemo(() => {
    if (!sending) return messages;

    return [
      ...messages,
      {
        id: "__pending__",
        session_id: activeChatId ?? "",
        role: "assistant",
        content: pendingText,
        created_at: new Date().toISOString(),
        source_run_id: null,
        usage: runUsage,
      } satisfies ChatMessage,
    ];
  }, [activeChatId, messages, pendingText, runUsage, sending]);

  const authenticatedProviders = useMemo(
    () =>
      providers.filter((provider) => provider.auth_status === "authenticated").length,
    [providers],
  );

  async function reloadProviders() {
    const data = await api.listProviders();
    setProviders(data);
  }

  async function reloadLlamaModels() {
    setLlamaModels(await api.listLlamaCppModels());
  }

  async function reloadChats() {
    const data = await api.listChats();
    setChats(data);

    if (!activeChatId && data.length > 0) {
      setActiveChatId(data[0].id);
    }

    if (activeChatId && !data.some((chat) => chat.id === activeChatId)) {
      setActiveChatId(data[0]?.id ?? null);
      setMessages([]);
    }
  }

  async function reloadUsage() {
    setUsage(await api.usageSummary());
  }

  async function loadMessages(chatId: string) {
    setMessages(await api.getChatMessages(chatId));
  }

  async function openChat(chatId: string) {
    setActiveChatId(chatId);
    await loadMessages(chatId);
    setPage("chat");
  }

  async function createChat() {
    await api.updateProviderPreferences(newChatProvider, {
      model: newChatModel,
      effort: newChatEffort,
    });

    const created = await api.createChat({
      provider: newChatProvider,
      title: newChatTitle.trim() || undefined,
    });

    setNewChatOpen(false);
    setNewChatTitle("");
    setNewChatEffort("high");

    await Promise.all([reloadProviders(), reloadChats()]);
    await openChat(created.id);
  }

  async function sendMessage() {
    if (!activeChatId || (!composer.trim() && attachments.length === 0)) return;

    setSending(true);
    setPendingText("");
    setRunUsage(null);
    setStatus("running");
    setError(null);

    const prompt = buildPromptWithAttachments(composer.trim(), attachments);
    setComposer("");
    setAttachments([]);

    try {
      const launch = await api.sendMessage(activeChatId, { content: prompt });
      appendRunLog(`run ${launch.run_id} started`);
      subscribeRun(launch.run_id, activeChatId);
    } catch (err) {
      setError(asError(err));
      setStatus("error");
      setSending(false);
      setPendingText("");
    }
  }

  async function handleAttachmentChange(fileList: FileList | null) {
    if (!fileList || fileList.length === 0) return;

    const nextAttachments = await Promise.all(
      Array.from(fileList).map(async (file) => {
        const textLike = isTextLikeFile(file);
        const inlineContent = textLike ? await file.text() : null;

        return {
          id: `${file.name}-${file.size}-${file.lastModified}`,
          name: file.name,
          mime: file.type || "application/octet-stream",
          size: file.size,
          inlineContent,
        } satisfies ComposerAttachment;
      }),
    );

    setAttachments((previous) => {
      const known = new Set(previous.map((attachment) => attachment.id));
      const unique = nextAttachments.filter((attachment) => !known.has(attachment.id));
      return [...previous, ...unique];
    });
  }

  function removeAttachment(attachmentId: string) {
    setAttachments((previous) =>
      previous.filter((attachment) => attachment.id !== attachmentId),
    );
  }

  function subscribeRun(runId: string, chatId: string) {
    const source = new EventSource(`/api/runs/${runId}/stream`);
    let finalized = false;

    const finalize = (nextStatus: "ready" | "error") => {
      if (finalized) return;
      finalized = true;
      source.close();
      setSending(false);
      setPendingText("");
      setStatus(nextStatus);

      void (async () => {
        for (let attempt = 0; attempt < 6; attempt += 1) {
          const loaded = await api.getChatMessages(chatId);
          setMessages(loaded);

          if (
            loaded.some(
              (message) =>
                message.role === "assistant" && message.source_run_id === runId,
            )
          ) {
            break;
          }

          await sleep(220 + attempt * 140);
        }

        await Promise.all([reloadChats(), reloadUsage()]);
      })();
    };

    source.onmessage = (event) => {
      const payload = safeParseEvent(event.data);
      if (!payload) return;

      appendRunLog(`${payload.event_kind}${payload.text ? `: ${payload.text}` : ""}`);

      if (payload.event_kind === "assistant_delta" || payload.event_kind === "assistant_final") {
        const nextText = sanitizeAssistantContent(String(payload.text ?? ""));
        setPendingText((previous) =>
          payload.event_kind === "assistant_final"
            ? nextText
            : mergeText(previous, nextText),
        );
      }

      if (payload.event_kind === "usage_updated") {
        setRunUsage(payload.usage ?? null);
      }

      if (payload.event_kind === "run_completed" || payload.event_kind === "run_failed") {
        finalize(payload.event_kind === "run_completed" ? "ready" : "error");
      }
    };

    source.onerror = () => {
      appendRunLog(`run ${runId} stream disconnected`);
      finalize("ready");
    };
  }

  async function handleProviderAuth(
    provider: ProviderKind,
    action: "login" | "logout",
  ) {
    const launch: AuthLaunchResponse =
      action === "login"
        ? await api.loginProvider(provider)
        : await api.logoutProvider(provider);

    appendAuthLog(`${provider} ${action} started`);

    const source = new EventSource(
      `/api/providers/${provider}/auth-stream/${launch.auth_id}`,
    );

    source.onmessage = (event) => {
      const payload = safeParseEvent(event.data);
      if (!payload) return;

      appendAuthLog(
        `${provider} ${payload.event_kind}${payload.text ? `: ${payload.text}` : ""}`,
      );

      if (payload.event_kind === "run_completed" || payload.event_kind === "run_failed") {
        source.close();
        void reloadProviders();
      }
    };
  }

  async function saveProviderPreferences(
    provider: ProviderKind,
    model: string,
    effort: string,
  ) {
    const saved: ProviderPreferences = await api.updateProviderPreferences(provider, {
      model,
      effort,
    });
    appendAuthLog(
      `${provider} actual: ${saved.model ?? "default"} ${saved.effort ?? "default"}`,
    );
    await reloadProviders();
  }

  function appendRunLog(line: string) {
    setRunLog((previous) =>
      previous.length === 1 && previous[0].startsWith("Waiting")
        ? [line]
        : [...previous, line],
    );
  }

  function appendAuthLog(line: string) {
    setAuthLog((previous) =>
      previous.length === 1 && previous[0].startsWith("Waiting")
        ? [line]
        : [...previous, line],
    );
  }

  function resetChatFilters() {
    setProviderFilter("all");
    setModelFilter("all");
    setKeywordFilter("");
    setChatPage(1);
  }

  return (
    <div className={sidebarOpen ? "app" : "app sidebar-collapsed"}>
      <aside className="sidebar">
        <div className="sidebar-top">
          <div className="brand-row">
            <button
              className="sidebar-toggle"
              onClick={() => setSidebarOpen((value) => !value)}
              aria-label={sidebarOpen ? "Collapse sidebar" : "Expand sidebar"}
            >
              {sidebarOpen ? "<" : ">"}
            </button>

            <div className="brand">
              <div className="brand-dot" />
              <div className="brand-copy">
                <h1>Ferrum AI</h1>
                <p>Local CLI orchestration lab</p>
              </div>
            </div>
          </div>

          <nav className="nav">
            <button
              className={page === "providers" ? "nav-item active" : "nav-item"}
              onClick={() => setPage("providers")}
            >
              <IconProviders />
              <span className="nav-label">Providers</span>
            </button>

            <button
              className={page === "agents" ? "nav-item active" : "nav-item"}
              onClick={() => setPage("agents")}
            >
              <IconAgents />
              <span className="nav-label">Agents</span>
            </button>

            <button
              className={page === "skills" ? "nav-item active" : "nav-item"}
              onClick={() => setPage("skills")}
            >
              <IconSkills />
              <span className="nav-label">Skills</span>
            </button>

            <button
              className={page === "mcps" ? "nav-item active" : "nav-item"}
              onClick={() => setPage("mcps")}
            >
              <IconMcp />
              <span className="nav-label">MCPs</span>
            </button>

            <button
              className={page === "chats" ? "nav-item active" : "nav-item"}
              onClick={() => setPage("chats")}
            >
              <IconChats />
              <span className="nav-label">Chats</span>
            </button>
          </nav>
        </div>

        <div className="sidebar-bottom">
          <div className="sidebar-meta">
            <span className={`status-pill ${status}`}>{status}</span>
            {error ? <p className="error-text">{error}</p> : null}
          </div>

          {sidebarOpen ? (
            <div className="sidebar-panels">
              <Panel
                title="Run events"
                open={runPanelOpen}
                onToggle={() => setRunPanelOpen((value) => !value)}
                onClear={() => setRunLog(["Waiting for run events..."])}
                content={runLog}
              />
              <Panel
                title="Auth events"
                open={authPanelOpen}
                onToggle={() => setAuthPanelOpen((value) => !value)}
                onClear={() => setAuthLog(["Waiting for auth events..."])}
                content={authLog}
              />
            </div>
          ) : null}
        </div>
      </aside>

      <main className="main">
        <header className="topbar">
          <div className="topbar-main">
            <span className="eyebrow">
              {page === "providers"
                ? "Provider setup"
                : page === "agents"
                  ? "Workflow orchestration"
                  : page === "skills"
                    ? "Shared expert context"
                    : page === "mcps"
                      ? "Shared tooling"
                : page === "chats"
                  ? "Conversation index"
                  : "Active conversation"}
            </span>
            <h2>
              {page === "providers"
                ? "Providers"
                : page === "agents"
                  ? "Agent Mode"
                  : page === "skills"
                    ? "Skills"
                    : page === "mcps"
                      ? "MCPs"
                : page === "chats"
                  ? "Chats"
                  : activeChat?.title ?? "Chat"}
            </h2>
            <p>
              {page === "providers"
                ? "Operate closed providers and local model policy from one secure control plane."
                : page === "agents"
                  ? "Run coordinated workflows without mixing them with shared tooling management."
                  : page === "skills"
                    ? "Prepare reusable expert context that future agents and providers can share."
                    : page === "mcps"
                      ? "Manage local MCP connectivity without mixing it with model distribution."
                : page === "chats"
                  ? "Filter by provider, model, or keyword and open any conversation."
                  : activeChat
                    ? `${labelProvider(activeChat.provider)} | ${
                        activeChat.last_model ?? "model pending"
                      }`
                    : "Select a chat from the list"}
            </p>
          </div>

          <div className="topbar-actions">
            {page === "chat" ? (
              <button className="ghost" onClick={() => setPage("chats")}>
                View chats
              </button>
            ) : null}
            <button className="new-chat-btn" onClick={() => setNewChatOpen(true)}>
              <span>+</span>
              <span>New Chat</span>
            </button>
          </div>
        </header>

        {page === "providers" ? (
          <ProvidersControlPlane
            providers={providers}
            llamaModels={llamaModels}
            usage={usage}
            authenticatedProviders={authenticatedProviders}
            onRefresh={async () => Promise.all([reloadProviders(), reloadUsage(), reloadLlamaModels()])}
            onAuth={handleProviderAuth}
            onSave={saveProviderPreferences}
          />
        ) : null}

        {page === "agents" ? <AgentMode providers={providers} /> : null}

        {page === "skills" ? <SkillsScreen /> : null}

        {page === "mcps" ? <McpRegistryScreen providers={providers} /> : null}

        {page === "chats" ? (
          <ChatsScreen
            chats={paginatedChats}
            filteredCount={filteredChats.length}
            totalCount={chats.length}
            currentPage={chatPage}
            totalPages={totalChatPages}
            keywordFilter={keywordFilter}
            providerFilter={providerFilter}
            modelFilter={modelFilter}
            knownModels={knownModels}
            onKeywordFilterChange={(value) => {
              setKeywordFilter(value);
              setChatPage(1);
            }}
            onProviderFilterChange={(value) => {
              setProviderFilter(value);
              setChatPage(1);
            }}
            onModelFilterChange={(value) => {
              setModelFilter(value);
              setChatPage(1);
            }}
            onResetFilters={resetChatFilters}
            onOpenChat={openChat}
            onPageChange={setChatPage}
          />
        ) : null}

        {page === "chat" ? (
          <ConversationScreen
            chat={activeChat}
            timeline={timeline}
            sending={sending}
            composer={composer}
            attachments={attachments}
            onComposerChange={setComposer}
            onAttachmentChange={handleAttachmentChange}
            onRemoveAttachment={removeAttachment}
            onSendMessage={sendMessage}
          />
        ) : null}
      </main>

      {newChatOpen ? (
        <NewChatModal
          provider={newChatProvider}
          model={newChatModel}
          llamaModels={llamaModels}
          effort={newChatEffort}
          title={newChatTitle}
          onProviderChange={(provider) => {
            setNewChatProvider(provider);
            setNewChatModel(getModelOptions(provider, llamaModels)[0]);
          }}
          onModelChange={setNewChatModel}
          onEffortChange={setNewChatEffort}
          onTitleChange={setNewChatTitle}
          onClose={() => setNewChatOpen(false)}
          onCreate={() => void createChat()}
        />
      ) : null}
    </div>
  );
}

export function ProvidersScreenLegacy(_: {
  providers: ProviderView[];
  llamaModels: LlamaCppModel[];
  usage: UsageSummary;
  todayUsage: { input: number; output: number; total: number };
  authenticatedProviders: number;
  onRefresh: () => Promise<unknown>;
  onAuth: (provider: ProviderKind, action: "login" | "logout") => Promise<void>;
  onSave: (provider: ProviderKind, model: string, effort: string) => Promise<void>;
}) {
  const props = _;
  const today = new Date().toISOString().slice(0, 10);
  const todayRows = props.usage.daily.filter((row) => row.day === today);

  return (
    <section className="providers-screen">
      <section className="providers-hero card">
        <div className="providers-hero-copy">
          <span className="eyebrow">Workspace routing</span>
          <h3>Provider controls built for local orchestration</h3>
          <p>
            Keep login, model, and effort configuration in one place while the chat
            surface stays focused on prompting.
          </p>
        </div>

        <div className="hero-stats">
          <MetricCard
            label="Providers ready"
            value={`${props.authenticatedProviders}/${props.providers.length}`}
          />
          <MetricCard label="Tokens today" value={formatNum(props.todayUsage.total)} />
          <MetricCard
            label="Input / output"
            value={`${formatNum(props.todayUsage.input)} / ${formatNum(props.todayUsage.output)}`}
          />
        </div>

        <button className="ghost refresh-button" onClick={() => void props.onRefresh()}>
          Refresh diagnostics
        </button>
      </section>

      <div className="provider-grid">
        {props.providers.map((provider) => (
          <ProviderCard
            key={provider.provider}
            provider={provider}
            llamaModels={props.llamaModels}
            usage={todayRows.find((row) => row.provider === provider.provider)}
            onAuth={props.onAuth}
            onSave={props.onSave}
          />
        ))}
      </div>
    </section>
  );
}

function ProviderCard(_: {
  provider: ProviderView;
  llamaModels: LlamaCppModel[];
  usage: UsageSummary["daily"][number] | undefined;
  onAuth: (provider: ProviderKind, action: "login" | "logout") => Promise<void>;
  onSave: (provider: ProviderKind, model: string, effort: string) => Promise<void>;
}) {
  const props = _;
  const models = getModelOptions(props.provider.provider, props.llamaModels);
  const [model, setModel] = useState(
    props.provider.selected_model && models.includes(props.provider.selected_model)
      ? props.provider.selected_model
      : models[0],
  );
  const [effort, setEffort] = useState(props.provider.selected_effort ?? "medium");

  useEffect(() => {
    if (props.provider.selected_model && models.includes(props.provider.selected_model)) {
      setModel(props.provider.selected_model);
    }
    setEffort(props.provider.selected_effort ?? "medium");
  }, [models, props.provider.selected_effort, props.provider.selected_model]);

  return (
    <article className="provider-card card">
      <div className="provider-card-header">
        <div>
          <span className="provider-kicker">{labelProvider(props.provider.provider)}</span>
          <h3>{props.provider.version ?? "Version unknown"}</h3>
        </div>

        <span className={`badge ${props.provider.auth_status}`}>
          {props.provider.auth_status}
        </span>
      </div>

      <div className="provider-current">
        <span className="chip chip-blue">{model}</span>
        <span className="chip">{effort}</span>
        <span className="chip">{props.provider.data_boundary}</span>
      </div>

      <div className="form-row">
        <label>Model</label>
        <select value={model} onChange={(event) => setModel(event.target.value)}>
          {models.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
      </div>

      <div className="form-row">
        <label>Effort</label>
        <select value={effort} onChange={(event) => setEffort(event.target.value)}>
          {EFFORT_OPTIONS.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
      </div>

      <div className="provider-actions">
        <button
          className="primary"
          onClick={() => void props.onSave(props.provider.provider, model, effort)}
        >
          Save profile
        </button>
        {props.provider.auth_required ? (
          <>
            <button
              className="ghost"
              onClick={() => void props.onAuth(props.provider.provider, "login")}
            >
              Login
            </button>
            <button
              className="ghost"
              onClick={() => void props.onAuth(props.provider.provider, "logout")}
            >
              Logout
            </button>
          </>
        ) : null}
      </div>

      <p className="provider-detail">{props.provider.detail ?? "No extra detail reported."}</p>

      {props.usage ? (
        <div className="provider-usage">
          <MetricInline label="Total today" value={formatNum(props.usage.total_tokens)} />
          <MetricInline
            label="Input / output"
            value={`${formatNum(props.usage.input_tokens)} / ${formatNum(props.usage.output_tokens)}`}
          />
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

function ChatsScreen(_: {
  chats: ChatSummary[];
  filteredCount: number;
  totalCount: number;
  currentPage: number;
  totalPages: number;
  keywordFilter: string;
  providerFilter: ProviderKind | "all";
  modelFilter: string;
  knownModels: string[];
  onKeywordFilterChange: (value: string) => void;
  onProviderFilterChange: (value: ProviderKind | "all") => void;
  onModelFilterChange: (value: string) => void;
  onResetFilters: () => void;
  onOpenChat: (chatId: string) => Promise<void>;
  onPageChange: (page: number) => void;
}) {
  const props = _;
  const rows = [...props.chats];
  while (rows.length < PAGE_SIZE) {
    rows.push({
      id: `placeholder-${rows.length}`,
      provider: "codex",
      title: "",
      provider_session_ref: null,
      last_model: null,
      created_at: "",
      last_message_at: null,
    });
  }

  return (
    <section className="card chats-screen">
      <header className="page-header">
        <div>
          <span className="eyebrow">All conversations</span>
          <h3>Chat index</h3>
          <p>
            {props.filteredCount} / {props.totalCount} chats match the current filters.
          </p>
        </div>

        <button className="ghost" onClick={props.onResetFilters}>
          Reset filters
        </button>
      </header>

      <div className="filters">
        <select
          value={props.providerFilter}
          onChange={(event) =>
            props.onProviderFilterChange(event.target.value as ProviderKind | "all")
          }
        >
          <option value="all">All providers</option>
          <option value="codex">Codex</option>
          <option value="claude">Claude</option>
          <option value="ollama">Ollama</option>
          <option value="llama_cpp">llama.cpp</option>
        </select>

        <select
          value={props.modelFilter}
          onChange={(event) => props.onModelFilterChange(event.target.value)}
        >
          <option value="all">All models</option>
          {props.knownModels.map((model) => (
            <option key={model} value={model}>
              {model}
            </option>
          ))}
        </select>

        <input
          value={props.keywordFilter}
          onChange={(event) => props.onKeywordFilterChange(event.target.value)}
          placeholder="Search by title, model, or id"
        />
      </div>

      <div className="chat-index-grid">
        {rows.map((chat) =>
          chat.title ? (
            <button
              key={chat.id}
              className="chat-index-item"
              onClick={() => void props.onOpenChat(chat.id)}
            >
              <div className="chat-index-left">
                <strong>{chat.title}</strong>
                <div className="chat-index-tags">
                  <span className="chip">{labelProvider(chat.provider)}</span>
                  <span className="chip chip-blue">
                    {chat.last_model ?? "model pending"}
                  </span>
                </div>
              </div>

              <div className="chat-index-right">
                <span>{formatDate(chat.last_message_at ?? chat.created_at)}</span>
              </div>
            </button>
          ) : (
            <div key={chat.id} className="chat-index-placeholder" />
          ),
        )}
      </div>

      <footer className="pagination">
        <button
          className="ghost"
          disabled={props.currentPage <= 1}
          onClick={() => props.onPageChange(props.currentPage - 1)}
        >
          Prev
        </button>
        <span>
          {props.currentPage} / {props.totalPages}
        </span>
        <button
          className="ghost"
          disabled={props.currentPage >= props.totalPages}
          onClick={() => props.onPageChange(props.currentPage + 1)}
        >
          Next
        </button>
      </footer>
    </section>
  );
}

function ConversationScreen(_: {
  chat: ChatSummary | null;
  timeline: ChatMessage[];
  sending: boolean;
  composer: string;
  attachments: ComposerAttachment[];
  onComposerChange: (value: string) => void;
  onAttachmentChange: (files: FileList | null) => void;
  onRemoveAttachment: (attachmentId: string) => void;
  onSendMessage: () => Promise<void>;
}) {
  const props = _;

  function handleComposerKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing) {
      return;
    }

    event.preventDefault();

    if (props.sending || (!props.composer.trim() && props.attachments.length === 0)) {
      return;
    }

    void props.onSendMessage();
  }

  return (
    <section className="conversation-screen card">
      <header className="conversation-header">
        <div>
          <span className="eyebrow">Conversation</span>
          <h3>{props.chat?.title ?? "Select a chat"}</h3>
          <p>
            {props.chat
              ? `${labelProvider(props.chat.provider)} | ${
                  props.chat.last_model ?? "model pending"
                }`
              : "Open a chat from the list or create a new one."}
          </p>
        </div>
      </header>

      <div className="message-stream">
        {props.timeline.length === 0 ? (
          <div className="empty-chat-state">
            <span className="eyebrow">No messages yet</span>
            <h3>Start with a prompt that is worth keeping</h3>
            <p>
              This surface is optimized for long answers, markdown formatting, and
              local run telemetry.
            </p>
          </div>
        ) : (
          props.timeline.map((message) => (
            <MessageBubble
              key={message.id}
              message={message}
              pending={message.id === "__pending__"}
            />
          ))
        )}
      </div>

      <div className="composer-shell">
        {props.attachments.length > 0 ? (
          <div className="attachment-list">
            {props.attachments.map((attachment) => (
              <div key={attachment.id} className="attachment-chip">
                <div>
                  <strong>{attachment.name}</strong>
                  <span>
                    {attachment.mime || "file"} · {formatFileSize(attachment.size)}
                  </span>
                </div>

                <button
                  type="button"
                  className="attachment-remove"
                  onClick={() => props.onRemoveAttachment(attachment.id)}
                  aria-label={`Remove ${attachment.name}`}
                >
                  ×
                </button>
              </div>
            ))}
          </div>
        ) : null}

        <div className="composer-note">
          {props.sending
            ? "Streaming response from the local provider..."
            : "Enter sends. Shift+Enter adds a new line. Markdown is rendered after persistence."}
        </div>

        <div className="composer">
          <label className="attach-button" aria-label="Attach files">
            <input
              type="file"
              multiple
              onChange={(event: ChangeEvent<HTMLInputElement>) => {
                props.onAttachmentChange(event.target.files);
                event.target.value = "";
              }}
            />
            <span>+</span>
          </label>

          <textarea
            value={props.composer}
            onChange={(event) => props.onComposerChange(event.target.value)}
            onKeyDown={handleComposerKeyDown}
            placeholder="Ask for a summary, a design explanation, or attach files for analysis..."
            rows={4}
          />
        </div>
      </div>
    </section>
  );
}

function MessageBubble(_: { message: ChatMessage; pending: boolean }) {
  const props = _;
  const isUser = props.message.role === "user";
  const cleanContent = isUser
    ? props.message.content
    : sanitizeAssistantContent(props.message.content);
  const blocks = !isUser ? parseMarkdownBlocks(cleanContent) : [];

  return (
    <article className={isUser ? "message-row user" : "message-row assistant"}>
      {!isUser ? <div className="avatar assistant">AI</div> : null}

      <div className={props.pending ? "message-card pending" : "message-card"}>
        <div className="message-meta">
          <span className="message-author">
            {isUser ? "You" : props.pending ? "Thinking" : "Assistant"}
          </span>
          <span>{formatDate(props.message.created_at)}</span>
        </div>

        {props.pending ? (
          <div className="thinking-block">
            <div className="thinking-status">
              <span className="live-pill">LIVE</span>
              <TypingDots />
            </div>

            {cleanContent ? (
              <div className="markdown">{renderMarkdownBlocks(blocks)}</div>
            ) : (
              <div className="thinking-placeholder">
                <span />
                <span />
                <span />
              </div>
            )}
          </div>
        ) : isUser ? (
          <p className="plain-text">{cleanContent}</p>
        ) : (
          <div className="markdown">{renderMarkdownBlocks(blocks)}</div>
        )}

        {props.message.usage ? (
          <p className="usage-line">
            {props.message.usage.model ? `${props.message.usage.model} | ` : ""}
            in/out {formatNum(props.message.usage.input_tokens ?? 0)}/
            {formatNum(props.message.usage.output_tokens ?? 0)} | total{" "}
            {formatNum(props.message.usage.total_tokens ?? 0)}
          </p>
        ) : null}
      </div>

      {isUser ? <div className="avatar user">You</div> : null}
    </article>
  );
}

function NewChatModal(_: {
  provider: ProviderKind;
  model: string;
  llamaModels: LlamaCppModel[];
  effort: (typeof EFFORT_OPTIONS)[number];
  title: string;
  onProviderChange: (provider: ProviderKind) => void;
  onModelChange: (value: string) => void;
  onEffortChange: (value: (typeof EFFORT_OPTIONS)[number]) => void;
  onTitleChange: (value: string) => void;
  onClose: () => void;
  onCreate: () => void;
}) {
  const props = _;

  return (
    <div className="modal-backdrop" onClick={props.onClose}>
      <div
        className="modal"
        onClick={(event) => {
          event.stopPropagation();
        }}
      >
        <span className="eyebrow">New conversation</span>
        <h3>Start with provider and model already selected</h3>
        <p>
          The chosen profile becomes the default for the first run of this chat.
        </p>

        <div className="form-row">
          <label>Provider</label>
          <select
            value={props.provider}
            onChange={(event) =>
              props.onProviderChange(event.target.value as ProviderKind)
            }
          >
            <option value="codex">Codex</option>
            <option value="claude">Claude</option>
            <option value="ollama">Ollama</option>
            <option value="llama_cpp">llama.cpp</option>
          </select>
        </div>

        <div className="form-row">
          <label>Model</label>
          <select
            value={props.model}
            onChange={(event) => props.onModelChange(event.target.value)}
          >
            {getModelOptions(props.provider, props.llamaModels).map((model) => (
              <option key={model} value={model}>
                {model}
              </option>
            ))}
          </select>
        </div>

        <div className="form-row">
          <label>Effort</label>
          <select
            value={props.effort}
            onChange={(event) =>
              props.onEffortChange(event.target.value as (typeof EFFORT_OPTIONS)[number])
            }
          >
            {EFFORT_OPTIONS.map((effort) => (
              <option key={effort} value={effort}>
                {effort}
              </option>
            ))}
          </select>
        </div>

        <div className="form-row">
          <label>Title</label>
          <input
            value={props.title}
            onChange={(event) => props.onTitleChange(event.target.value)}
            placeholder="Optional chat title"
          />
        </div>

        <div className="modal-actions">
          <button className="ghost" onClick={props.onClose}>
            Cancel
          </button>
          <button className="primary" onClick={props.onCreate}>
            Create chat
          </button>
        </div>
      </div>
    </div>
  );
}

function MetricCard(_: { label: string; value: string }) {
  const props = _;
  return (
    <article className="metric-card">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </article>
  );
}

function MetricInline(_: { label: string; value: string }) {
  const props = _;
  return (
    <div className="metric-inline">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  );
}

function TypingDots() {
  return (
    <span className="typing-dots" aria-hidden="true">
      <span />
      <span />
      <span />
    </span>
  );
}

function Panel(_: {
  title: string;
  open: boolean;
  onToggle: () => void;
  onClear: () => void;
  content: string[];
}) {
  const props = _;

  return (
    <article className="panel">
      <header className="panel-head" onClick={props.onToggle}>
        <div className="panel-title">
          <span className={props.open ? "chevron open" : "chevron"}>v</span>
          <strong>{props.title}</strong>
        </div>

        <button
          className="ghost panel-clear"
          onClick={(event) => {
            event.stopPropagation();
            props.onClear();
          }}
        >
          Clear
        </button>
      </header>

      {props.open ? (
        <pre className="panel-body">
          {props.content.length ? props.content.join("\n") : "No events yet."}
        </pre>
      ) : null}
    </article>
  );
}

function IconProviders() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M4 6h16M4 12h16M4 18h10" />
    </svg>
  );
}

function IconAgents() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M5 6h14" />
      <path d="M5 12h6" />
      <path d="M5 18h10" />
      <path d="M16 10l3 2-3 2" />
    </svg>
  );
}

function IconSkills() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M6 5h12v14l-6-3.8L6 19z" />
      <path d="M9 8h6" />
      <path d="M9 11h4" />
    </svg>
  );
}

function IconMcp() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M7 7h2v2H7z" />
      <path d="M15 7h2v2h-2z" />
      <path d="M7 15h2v2H7z" />
      <path d="M9 8h6" />
      <path d="M8 9v6" />
      <path d="M9 16h6" />
      <path d="M16 8v8" />
    </svg>
  );
}

function IconChats() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M4 5h16v11H8l-4 4z" />
    </svg>
  );
}

function renderMarkdownBlocks(_: MarkdownBlock[]) {
  const blocks = _;

  return blocks.map((block, index) => {
    const key = `block-${index}`;

    if (block.type === "heading") {
      if (block.level === 1) return <h1 key={key}>{renderInline(block.text, key)}</h1>;
      if (block.level === 2) return <h2 key={key}>{renderInline(block.text, key)}</h2>;
      if (block.level === 3) return <h3 key={key}>{renderInline(block.text, key)}</h3>;
      return <h4 key={key}>{renderInline(block.text, key)}</h4>;
    }

    if (block.type === "unordered-list") {
      return (
        <ul key={key}>
          {block.items.map((item, itemIndex) => (
            <li key={`${key}-item-${itemIndex}`}>
              {renderInline(item, `${key}-item-${itemIndex}`)}
            </li>
          ))}
        </ul>
      );
    }

    if (block.type === "ordered-list") {
      return (
        <ol key={key}>
          {block.items.map((item, itemIndex) => (
            <li key={`${key}-item-${itemIndex}`}>
              {renderInline(item, `${key}-item-${itemIndex}`)}
            </li>
          ))}
        </ol>
      );
    }

    if (block.type === "code") {
      return (
        <pre key={key}>
          <code>{block.text}</code>
        </pre>
      );
    }

    return <p key={key}>{renderInline(block.text, key)}</p>;
  });
}

function renderInline(_: string, __: string) {
  const text = _;
  const keyPrefix = __;
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*]+\*)/g;
  const segments = text.split(pattern).filter(Boolean);

  return segments.flatMap((segment, index) => {
    const key = `${keyPrefix}-${index}`;

    if (segment.startsWith("`") && segment.endsWith("`")) {
      return [<code key={key}>{segment.slice(1, -1)}</code>];
    }

    if (segment.startsWith("**") && segment.endsWith("**")) {
      return [<strong key={key}>{segment.slice(2, -2)}</strong>];
    }

    if (segment.startsWith("*") && segment.endsWith("*")) {
      return [<em key={key}>{segment.slice(1, -1)}</em>];
    }

    return segment.split("\n").flatMap((line, lineIndex, lines) => {
      const lineKey = `${key}-line-${lineIndex}`;
      if (lineIndex === lines.length - 1) {
        return [<Fragment key={lineKey}>{line}</Fragment>];
      }
      return [
        <Fragment key={lineKey}>{line}</Fragment>,
        <br key={`${lineKey}-break`} />,
      ];
    });
  });
}

function parseMarkdownBlocks(_: string): MarkdownBlock[] {
  const text = _;
  const lines = text.replace(/\r/g, "").split("\n");
  const blocks: MarkdownBlock[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];

    if (!line.trim()) {
      index += 1;
      continue;
    }

    if (line.startsWith("```")) {
      index += 1;
      const codeLines: string[] = [];
      while (index < lines.length && !lines[index].startsWith("```")) {
        codeLines.push(lines[index]);
        index += 1;
      }
      index += 1;
      blocks.push({ type: "code", text: codeLines.join("\n") });
      continue;
    }

    const heading = line.match(/^(#{1,4})\s+(.+)$/);
    if (heading) {
      blocks.push({
        type: "heading",
        level: heading[1].length as 1 | 2 | 3 | 4,
        text: heading[2],
      });
      index += 1;
      continue;
    }

    if (/^[-*]\s+/.test(line)) {
      const items: string[] = [];
      while (index < lines.length && /^[-*]\s+/.test(lines[index])) {
        items.push(lines[index].replace(/^[-*]\s+/, ""));
        index += 1;
      }
      blocks.push({ type: "unordered-list", items });
      continue;
    }

    if (/^\d+\.\s+/.test(line)) {
      const items: string[] = [];
      while (index < lines.length && /^\d+\.\s+/.test(lines[index])) {
        items.push(lines[index].replace(/^\d+\.\s+/, ""));
        index += 1;
      }
      blocks.push({ type: "ordered-list", items });
      continue;
    }

    const paragraph: string[] = [line];
    index += 1;

    while (
      index < lines.length &&
      lines[index].trim() &&
      !lines[index].startsWith("```") &&
      !/^(#{1,4})\s+/.test(lines[index]) &&
      !/^[-*]\s+/.test(lines[index]) &&
      !/^\d+\.\s+/.test(lines[index])
    ) {
      paragraph.push(lines[index]);
      index += 1;
    }

    blocks.push({ type: "paragraph", text: paragraph.join("\n") });
  }

  return blocks;
}

function sanitizeAssistantContent(content: string) {
  return content
    .replace(/[0-9a-f]{8}-[0-9a-f-]{27,}/gi, "")
    .replace(/\*\*Preparing[^*]+\*\*/gi, "")
    .replace(/Preparing [A-Za-z][^\n]*(?=(Hola|En\s`|Me\s|#|\n|$))/g, "")
    .replace(/\bturn\.(started|completed)\b/gi, "")
    .replace(/\s{2,}/g, " ")
    .trim();
}

function safeParseEvent(raw: string): StreamEvent | null {
  try {
    return JSON.parse(raw) as StreamEvent;
  } catch {
    return null;
  }
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

function getModelOptions(provider: ProviderKind, llamaModels: LlamaCppModel[]) {
  if (provider === "llama_cpp") {
    const registered = llamaModels
      .filter((model) => model.enabled)
      .map((model) => model.file_path);
    return registered.length > 0 ? registered : MODEL_OPTIONS.llama_cpp;
  }
  return MODEL_OPTIONS[provider];
}

function asError(value: unknown): string {
  if (value instanceof Error) return value.message;
  return String(value);
}

function formatDate(value: string) {
  if (!value) return "-";
  const date = new Date(value);
  return `${date.toLocaleDateString()} ${date.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  })}`;
}

function formatNum(value: number) {
  return new Intl.NumberFormat().format(value);
}

function formatFileSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function isTextLikeFile(file: File) {
  if (file.type.startsWith("text/")) return true;

  const textExtensions = [
    ".md",
    ".txt",
    ".json",
    ".csv",
    ".ts",
    ".tsx",
    ".js",
    ".jsx",
    ".rs",
    ".py",
    ".html",
    ".css",
    ".sql",
    ".toml",
    ".yaml",
    ".yml",
    ".xml",
  ];

  const lowercaseName = file.name.toLowerCase();
  return textExtensions.some((extension) => lowercaseName.endsWith(extension));
}

function buildPromptWithAttachments(prompt: string, attachments: ComposerAttachment[]) {
  if (attachments.length === 0) return prompt;

  const metadata = attachments
    .map(
      (attachment, index) =>
        `${index + 1}. ${attachment.name} (${attachment.mime || "file"}, ${formatFileSize(
          attachment.size,
        )})`,
    )
    .join("\n");

  const embeddedText = attachments
    .filter((attachment) => attachment.inlineContent)
    .map(
      (attachment) =>
        `<attachment name="${attachment.name}">\n${attachment.inlineContent}\n</attachment>`,
    )
    .join("\n\n");

  if (!prompt) {
    return embeddedText
      ? `Attached files:\n${metadata}\n\n${embeddedText}`
      : `Attached files:\n${metadata}`;
  }

  if (!embeddedText) {
    return `${prompt}\n\nAttached files:\n${metadata}`;
  }

  return `${prompt}\n\nAttached files:\n${metadata}\n\n${embeddedText}`;
}

function mergeText(previous: string, next: string) {
  if (!previous) return next;
  if (!next) return previous;
  if (next.startsWith(previous)) return next;
  return `${previous}${next}`;
}

function sleep(ms: number) {
  return new Promise<void>((resolve) => {
    setTimeout(resolve, ms);
  });
}
