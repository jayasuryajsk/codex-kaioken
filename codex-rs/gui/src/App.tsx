import { useState, useRef, useEffect, useCallback } from "react";
import {
  Header,
  ChatMessage,
  ChatTabs,
  InputArea,
  Sidebar,
  RightPanel,
  ApprovalModal,
  CreateSessionModal,
  WelcomeCard,
  HomePage,
  SettingsView,
} from "./components";
import {
  Message,
  AppSettings,
  AgentSession,
  ReasoningEffort,
  ToolExecution,
  StreamEvent,
  PlanUpdate,
  PlanWorkflowStatus,
  ThemeName,
  THEME_INFO,
  Repository,
  WorktreeSession,
  SubagentTask,
  ChatTab,
} from "./types";
import {
  sendMessage as tauriSendMessage,
  getStatus,
  initSession,
  setModel,
  setReasoningEffort,
  setApprovalMode,
  subscribeToSessionEvents,
  sendApproval,
  interrupt,
  TokenUsageEvent,
  ApprovalRequestEvent,
  getWorkspaceConfig,
  updateSessionTabs,
  addTabToHistory,
  getSessionHistory,
  TabConfigPersist,
  addRepository as tauriAddRepository,
  createWorktree as tauriCreateWorktree,
  removeRepository as tauriRemoveRepository,
  getSessionRollout,
  upsertWorktreeSession,
} from "./tauri-api";
import { open } from "@tauri-apps/plugin-dialog";
// Note: Node.js fs/path modules don't work in browser/Tauri frontend
// Session messages are restored through backend event stream on resume

// Panel size constraints
const LEFT_PANEL_MIN = 180;
const LEFT_PANEL_MAX = 400;
const LEFT_PANEL_DEFAULT = 256;
const RIGHT_PANEL_MIN = 280;
const RIGHT_PANEL_MAX = 600;
const RIGHT_PANEL_DEFAULT = 400;

function App() {
  // Single source of truth: worktreeSessions (defined below)
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [, setIsConnected] = useState(false);

  // App view state: 'home' shows homepage, 'workspace' shows the full workspace UI
  const [appView, setAppView] = useState<"home" | "workspace">("home");

  // Workspace state for multi-repo, multi-worktree sidebar
  const [repositories, setRepositories] = useState<Repository[]>([]);
  const [worktreeSessions, setWorktreeSessions] = useState<WorktreeSession[]>(
    [],
  );
  const [worktreeModalRepo, setWorktreeModalRepo] = useState<Repository | null>(
    null,
  );
  const [leftPanelWidth, setLeftPanelWidth] = useState(LEFT_PANEL_DEFAULT);
  const [rightPanelWidth, setRightPanelWidth] = useState(RIGHT_PANEL_DEFAULT);
  const [resizing, setResizing] = useState<'left' | 'right' | null>(null);
  const lastMouseX = useRef(0);
  const defaultSessionSettings: Omit<AppSettings, "theme"> = {
    // Chat settings
    model: "gpt-5.2-codex",
    reasoningEffort: "medium" as ReasoningEffort,
    planMode: false,
    planDetail: "auto",
    sendKey: "enter",
    // Approval settings
    approvalMode: "auto",
    // Performance settings
    subagentConcurrency: 4,
    // Display settings
    showRateLimits: true,
    showTokenCost: false,
    animations: true,
    // Notification settings
    desktopNotifications: true,
    soundEffects: false,
    // Experimental features
    experimentalFeatures: {},
  };
  const [sessionSettings, setSessionSettings] = useState<
    Record<string, typeof defaultSessionSettings>
  >({});
  const [theme, setTheme] = useState<ThemeName>("cream");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [tokenUsageBySession, setTokenUsageBySession] = useState<
    Map<string, TokenUsageEvent>
  >(new Map());
  const [approvalRequest, setApprovalRequest] =
    useState<ApprovalRequestEvent | null>(null);
  const [systemMessages, setSystemMessages] = useState<
    Array<{ id: string; type: string; message: string; timestamp: Date }>
  >([]);
  const [planWorkflowStatus, setPlanWorkflowStatus] =
    useState<PlanWorkflowStatus>("idle");
  const [currentPlan, setCurrentPlan] = useState<PlanUpdate | null>(null);
  const [planMessageId, setPlanMessageId] = useState<string | null>(null);
  const [originalPlanRequest, setOriginalPlanRequest] = useState<string>("");
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const activeSessionIdRef = useRef(activeSessionId);
  const activeToolCallsRef = useRef<Map<string, ToolExecution>>(new Map());
  const activeSubagentsRef = useRef<Map<string, SubagentTask>>(new Map());
  const subagentMessageIdsRef = useRef<Map<string, Map<string, string>>>(
    new Map(),
  );
  const [sessionHistory, setSessionHistory] = useState<TabConfigPersist[]>([]);

  // Derive activeSession from worktreeSessions (single source of truth)
  const activeWorktreeSession = activeSessionId
    ? worktreeSessions.find((s) => s.id === activeSessionId)
    : null;

  // Get active tab for the current worktree session
  const getActiveTab = (session: WorktreeSession | null | undefined): ChatTab | null => {
    if (!session) return null;
    const tabs = session.tabs || [];
    if (tabs.length === 0) return null;
    const activeTabId = session.activeTabId || tabs[0]?.id;
    return tabs.find(t => t.id === activeTabId) || tabs[0] || null;
  };

  const activeTab = getActiveTab(activeWorktreeSession);

  // Get messages from active tab, fallback to session messages for backward compat
  const activeMessages = activeTab?.messages || activeWorktreeSession?.messages || [];

  // Convert to AgentSession shape for components that expect it
  const activeSession: AgentSession | null = activeWorktreeSession
    ? {
        id: activeWorktreeSession.id,
        name: activeWorktreeSession.worktreeName,
        status:
          activeWorktreeSession.status === "idle"
            ? "idle"
            : activeWorktreeSession.status === "disconnected"
              ? "idle"
              : "running",
        branch: activeWorktreeSession.worktreeName,
        stats: activeWorktreeSession.stats,
        tokenUsage: activeTab?.tokenUsage || activeWorktreeSession.tokenUsage,
        messages: activeMessages,
      }
    : null;

  const settingsKeyFor = (sessionId: string | null) => sessionId ?? "default";

  const activeSettingsForSession = (sessionId: string | null): AppSettings => {
    const key = settingsKeyFor(sessionId);
    const session = sessionSettings[key] ?? defaultSessionSettings;
    return { ...session, theme };
  };

  const activeSettings = activeSettingsForSession(activeSessionId);

  const updateSessionSettings = (
    sessionId: string | null,
    updater: (
      prev: typeof defaultSessionSettings,
    ) => typeof defaultSessionSettings,
  ) => {
    const key = settingsKeyFor(sessionId);
    setSessionSettings((prev) => {
      const current = prev[key] ?? defaultSessionSettings;
      const next = updater(current);
      return { ...prev, [key]: next };
    });
  };
  const activeTokenUsage = activeSessionId
    ? tokenUsageBySession.get(activeSessionId) || null
    : null;

  // Resize handlers - edge-based resizing
  const startResize = useCallback((edge: 'left' | 'right', e: React.MouseEvent) => {
    e.preventDefault();
    lastMouseX.current = e.clientX;
    setResizing(edge);
  }, []);

  useEffect(() => {
    if (!resizing) return;

    const handleMouseMove = (e: MouseEvent) => {
      const delta = e.clientX - lastMouseX.current;
      lastMouseX.current = e.clientX;

      if (resizing === 'left') {
        setLeftPanelWidth((prev) =>
          Math.min(LEFT_PANEL_MAX, Math.max(LEFT_PANEL_MIN, prev + delta)),
        );
      } else {
        setRightPanelWidth((prev) =>
          Math.min(RIGHT_PANEL_MAX, Math.max(RIGHT_PANEL_MIN, prev - delta)),
        );
      }
    };

    const handleMouseUp = () => {
      setResizing(null);
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [resizing]);

  // Scroll to bottom on new messages
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [activeSession?.messages]);

  // Apply theme class to document
  useEffect(() => {
    activeSessionIdRef.current = activeSessionId;
  }, [activeSessionId]);

  useEffect(() => {
    const key = settingsKeyFor(activeSessionId);
    setSessionSettings((prev) => {
      if (prev[key]) return prev;
      return { ...prev, [key]: defaultSessionSettings };
    });
  }, [activeSessionId]);

  useEffect(() => {
    const root = document.documentElement;
    root.classList.remove(
      "dark",
      "theme-snow",
      "theme-cream",
      "theme-sage",
      "theme-midnight",
      "theme-obsidian",
    );
    root.classList.add(`theme-${theme}`);
    if (THEME_INFO[theme].isDark) {
      root.classList.add("dark");
    }
  }, [theme]);

  // Per-session streaming message refs (maps sessionId to streamingMessageId)
  const streamingMessageIdsRef = useRef<Map<string, string>>(new Map());

  useEffect(() => {
    console.log("[App] Setting up session-aware event subscription...");

    // Helper to update messages in a tab (sessionId can be tab ID or worktree session ID)
    const updateTabMessages = (
      sessionId: string,
      updater: (messages: Message[]) => Message[]
    ) => {
      setWorktreeSessions((prev) => {
        // First try to find a tab with this ID
        for (const session of prev) {
          const tab = session.tabs?.find((t) => t.id === sessionId);
          if (tab) {
            return prev.map((s) => {
              if (s.id !== session.id) return s;
              return {
                ...s,
                tabs: (s.tabs || []).map((t) =>
                  t.id === sessionId
                    ? { ...t, messages: updater(t.messages) }
                    : t
                ),
              };
            });
          }
        }
        // Fallback: try to match worktree session ID (for backward compat)
        return prev.map((session) => {
          if (session.id !== sessionId) return session;
          // Update first tab if exists, otherwise update session messages
          if (session.tabs && session.tabs.length > 0) {
            const activeTabId = session.activeTabId || session.tabs[0].id;
            return {
              ...session,
              tabs: session.tabs.map((t) =>
                t.id === activeTabId
                  ? { ...t, messages: updater(t.messages) }
                  : t
              ),
            };
          }
          return { ...session, messages: updater(session.messages) };
        });
      });
    };

    const upsertSubagentEvent = (
      sessionId: string,
      task: SubagentTask,
      opts?: { allowCreate?: boolean },
    ) => {
      const allowCreate = opts?.allowCreate ?? true;

      // Per-call subagent host message tracking
      let perSession = subagentMessageIdsRef.current.get(sessionId);
      if (!perSession) {
        perSession = new Map();
        subagentMessageIdsRef.current.set(sessionId, perSession);
      }

      updateTabMessages(sessionId, (messages) => {
        let messageId = perSession!.get(task.callId);
        const hasMessage =
          messageId && messages.some((message) => message.id === messageId);

        let updatedMessages = messages;

        if (!hasMessage) {
          if (!allowCreate) return messages;
          messageId = `subagent-${task.callId}-${Date.now()}`;
          perSession!.set(task.callId, messageId);
          updatedMessages = [
            ...messages,
            {
              id: messageId,
              role: "assistant" as const,
              content: "",
              timestamp: new Date(),
              events: [],
            },
          ];
        }

        return updatedMessages.map((msg) => {
          if (msg.id !== messageId) return msg;
          const events = msg.events || [];
          const existingIdx = events.findIndex(
            (e) =>
              e.type === "subagent" &&
              e.task.callId === task.callId &&
              e.task.agentIndex === task.agentIndex,
          );
          if (existingIdx >= 0) {
            const updated = [...events];
            updated[existingIdx] = { type: "subagent", task: { ...task } };
            return { ...msg, events: updated };
          }
          return {
            ...msg,
            events: [...events, { type: "subagent", task: { ...task } }],
          };
        });
      });
    };

    // Helper to get streaming message ID for a session
    const getStreamingId = (sessionId: string) =>
      streamingMessageIdsRef.current.get(sessionId);
    const setStreamingId = (sessionId: string, messageId: string | null) => {
      if (messageId) {
        streamingMessageIdsRef.current.set(sessionId, messageId);
      } else {
        streamingMessageIdsRef.current.delete(sessionId);
      }
    };

    const unsubscribe = subscribeToSessionEvents({
      // onMessage - skip, we use streaming deltas instead
      onMessage: (sessionId) => {
        console.log("[App] onMessage for session:", sessionId);
      },

      // onToolStart - append tool event to the correct session/tab
      onToolStart: (sessionId, tool) => {
        console.log(
          "[App] toolStart[%s]:",
          sessionId,
          tool.id,
          tool.type,
          tool.name,
        );
        activeToolCallsRef.current.set(tool.id, tool);
        setIsLoading(false);

        updateTabMessages(sessionId, (messages) => {
          const streamingId = getStreamingId(sessionId);
          const event: StreamEvent = { type: "tool", execution: tool };

          // Find or create streaming message
          const existingMsg = streamingId ? messages.find(m => m.id === streamingId) : null;

          if (existingMsg) {
            // Append event to existing message
            return messages.map(m =>
              m.id === streamingId
                ? {
                    ...m,
                    events: [...(m.events || []), event],
                    toolCalls: [...(m.toolCalls || []), tool]
                  }
                : m
            );
          }

          // Create new streaming message with this event
          const newId = `stream-${Date.now()}`;
          setStreamingId(sessionId, newId);
          return [
            ...messages,
            {
              id: newId,
              role: "assistant" as const,
              content: "",
              timestamp: new Date(),
              events: [event],
              toolCalls: [tool],
            },
          ];
        });
      },

      // onToolEnd - update tool in events for the correct session/tab
      onToolEnd: (sessionId, tool) => {
        console.log("[App] toolEnd[%s]:", sessionId, tool.id, tool.status);
        activeToolCallsRef.current.set(tool.id, tool);

        updateTabMessages(sessionId, (messages) => {
          const streamingId = getStreamingId(sessionId);
          if (!streamingId) return messages;

          return messages.map(m => {
            if (m.id !== streamingId) return m;
            const events = (m.events || []).map(e =>
              e.type === "tool" && e.execution.id === tool.id
                ? { ...e, execution: tool }
                : e
            );
            const toolCalls = (m.toolCalls || []).map(t =>
              t.id === tool.id ? tool : t
            );
            return { ...m, events, toolCalls };
          });
        });
      },

      // onError
      onError: (sessionId, error) => {
        console.error("Kaioken event error[%s]:", sessionId, error);
        setIsLoading(false);
      },

      // onContentDelta - append content event to the correct session/tab
      onContentDelta: (sessionId, delta) => {
        setIsLoading(false);

        updateTabMessages(sessionId, (messages) => {
          const streamingId = getStreamingId(sessionId);
          const existingMsg = streamingId
            ? messages.find((m) => m.id === streamingId)
            : null;

          if (existingMsg) {
            const events = existingMsg.events || [];
            const lastEvent = events[events.length - 1];

            if (lastEvent?.type === "content") {
              return messages.map((m) =>
                m.id === streamingId
                  ? {
                      ...m,
                      content: m.content + delta,
                      events: [
                        ...events.slice(0, -1),
                        {
                          type: "content" as const,
                          text: lastEvent.text + delta,
                        },
                      ],
                    }
                  : m,
              );
            }
            return messages.map((m) =>
              m.id === streamingId
                ? {
                    ...m,
                    content: m.content + delta,
                    events: [
                      ...events,
                      { type: "content" as const, text: delta },
                    ],
                  }
                : m,
            );
          }

          // Create new streaming message
          const messageId = `stream-${Date.now()}`;
          setStreamingId(sessionId, messageId);
          return [
            ...messages,
            {
              id: messageId,
              role: "assistant" as const,
              content: delta,
              timestamp: new Date(),
              events: [{ type: "content" as const, text: delta }],
            },
          ];
        });
      },

      // onTaskComplete - mark message as complete with timestamp
      onTaskComplete: (sessionId) => {
        setIsLoading(false);
        const streamingId = getStreamingId(sessionId);
        if (streamingId) {
          updateTabMessages(sessionId, (messages) =>
            messages.map((msg) =>
              msg.id === streamingId
                ? { ...msg, completedAt: new Date() }
                : msg,
            ),
          );
        }
        setStreamingId(sessionId, null);
        activeToolCallsRef.current.clear();
      },

      // onReasoningDelta - append reasoning event
      onReasoningDelta: (sessionId, delta) => {
        updateTabMessages(sessionId, (messages) => {
          const streamingId = getStreamingId(sessionId);
          const existingMsg = streamingId
            ? messages.find((m) => m.id === streamingId)
            : null;

          if (existingMsg) {
            const events = existingMsg.events || [];
            const lastEvent = events[events.length - 1];

            // If last event is reasoning, append to it
            if (lastEvent?.type === "reasoning") {
              return messages.map((m) =>
                m.id === streamingId
                  ? {
                      ...m,
                      reasoning: (m.reasoning || "") + delta,
                      events: [
                        ...events.slice(0, -1),
                        {
                          type: "reasoning" as const,
                          text: lastEvent.text + delta,
                        },
                      ],
                    }
                  : m,
              );
            }
            // Otherwise, add new reasoning event
            return messages.map((m) =>
              m.id === streamingId
                ? {
                    ...m,
                    reasoning: (m.reasoning || "") + delta,
                    events: [
                      ...events,
                      { type: "reasoning" as const, text: delta },
                    ],
                  }
                : m,
            );
          }

          // Create new streaming message
          const newId = `stream-${Date.now()}`;
          setStreamingId(sessionId, newId);
          return [
            ...messages,
            {
              id: newId,
              role: "assistant" as const,
              content: "",
              timestamp: new Date(),
              reasoning: delta,
              events: [{ type: "reasoning" as const, text: delta }],
            },
          ];
        });
      },

      // onExecOutputDelta - update tool output in events
      onExecOutputDelta: (sessionId, delta) => {
        updateTabMessages(sessionId, (messages) => {
          const streamingId = getStreamingId(sessionId);
          if (!streamingId) return messages;

          return messages.map((msg) => {
            if (msg.id !== streamingId) return msg;

            // Update in events
            const events = msg.events || [];
            const updatedEvents = events.map((e) => {
              if (e.type === "tool" && e.execution.id === delta.callId) {
                return {
                  ...e,
                  execution: {
                    ...e.execution,
                    output: (e.execution.output || "") + delta.chunk,
                  },
                };
              }
              return e;
            });

            // Also update legacy toolCalls
            const tools = msg.toolCalls || [];
            const updatedTools = tools.map((t) => {
              if (t.id !== delta.callId) return t;
              return { ...t, output: (t.output || "") + delta.chunk };
            });

            return {
              ...msg,
              events: updatedEvents,
              toolCalls: updatedTools,
            };
          });
        });
      },

      // Token usage - store per session
      onTokenUsage: (sessionId, usage) => {
        setTokenUsageBySession((prev) => {
          const next = new Map(prev);
          next.set(sessionId, usage);
          return next;
        });
      },

      // Approval request
      onApprovalRequest: (sessionId, req) => {
        // Store sessionId with the request for approval handling
        setApprovalRequest({ ...req, sessionId } as ApprovalRequestEvent & {
          sessionId: string;
        });
      },

      // Warning
      onWarning: (sessionId, message) => {
        setSystemMessages((prev) => [
          ...prev.slice(-9),
          {
            id: `warn-${Date.now()}`,
            type: "warning",
            message: `[${sessionId}] ${message}`,
            timestamp: new Date(),
          },
        ]);
      },

      // Background
      onBackground: (sessionId, message) => {
        setSystemMessages((prev) => [
          ...prev.slice(-9),
          {
            id: `bg-${Date.now()}`,
            type: "background",
            message: `[${sessionId}] ${message}`,
            timestamp: new Date(),
          },
        ]);
      },

      // Task started
      onTaskStarted: (sessionId) => {
        if (sessionId === activeSessionIdRef.current) {
          setIsLoading(true);
        }
      },

      // Context compacted
      onContextCompacted: (sessionId) => {
        setSystemMessages((prev) => [
          ...prev.slice(-9),
          {
            id: `compact-${Date.now()}`,
            type: "info",
            message: `[${sessionId}] Context compacted`,
            timestamp: new Date(),
          },
        ]);
      },

      // Plan update
      onPlanUpdate: (sessionId, plan) => {
        if (sessionId !== activeSessionIdRef.current) return;

        setCurrentPlan(plan);
        // Only set to awaiting_approval if not already executing
        setPlanWorkflowStatus((prev) =>
          prev === "executing" ? "executing" : "awaiting_approval",
        );

        // Track which message should show the plan card
        let streamingId = getStreamingId(sessionId);
        if (!streamingId) {
          streamingId = `stream-${Date.now()}`;
          setStreamingId(sessionId, streamingId);
          // Create the message for this plan
          updateTabMessages(sessionId, (messages) => [
            ...messages,
            {
              id: streamingId!,
              role: "assistant" as const,
              content: "",
              timestamp: new Date(),
              events: [],
            },
          ]);
        }

        // Set the plan message ID (only the first time)
        setPlanMessageId((prev) => prev ?? streamingId);
      },

      // Subagent update - status changes
      onSubagentUpdate: (sessionId, event) => {
        const agentIndex = event.agentIndex ?? 0;
        const taskKey = `${event.callId}-${agentIndex}`;
        console.log(
          "[App] subagentUpdate[%s]:",
          sessionId,
          taskKey,
          event.status,
          event.task,
        );

        // Get or create the subagent task
        let task = activeSubagentsRef.current.get(taskKey);
        if (!task) {
          task = {
            callId: event.callId,
            agentIndex: agentIndex,
            task: event.task,
            status: (event.status as SubagentTask["status"]) || "Running",
            logs: [],
            startedAt: new Date(),
          };
          activeSubagentsRef.current.set(taskKey, task);
        }

        // Update task status
        task.status = (event.status as SubagentTask["status"]) || task.status;
        if (event.summary) task.summary = event.summary;
        if (task.status !== "Running") {
          task.finishedAt = new Date();
        }

        upsertSubagentEvent(sessionId, task!, { allowCreate: true });

        // Clean up finished subagents after a delay
        if (task.status !== "Running") {
          setTimeout(() => {
            activeSubagentsRef.current.delete(taskKey);
          }, 5000);
        }
      },

      // Subagent log - streaming log lines
      onSubagentLog: (sessionId, event) => {
        if (!event.line) return;

        const agentIndex = event.agentIndex ?? 0;
        const taskKey = `${event.callId}-${agentIndex}`;

        // Get or create the subagent task
        let task = activeSubagentsRef.current.get(taskKey);
        if (!task) {
          task = {
            callId: event.callId,
            agentIndex: agentIndex,
            task: event.task,
            status: "Running",
            logs: [],
            startedAt: new Date(),
          };
          activeSubagentsRef.current.set(taskKey, task);
        }

        // Append log line (keep last 50)
        task.logs = [...task.logs.slice(-49), event.line];

        upsertSubagentEvent(sessionId, task!, { allowCreate: true });
      },

      // Memory response - triggers sidebar refresh when memory is stored
      onMemoryResponse: (sessionId, response) => {
        console.log("[App] memoryResponse[%s]:", sessionId, response);
        // Memory stored successfully - the RightPanel will auto-refresh via its interval
        // But we can trigger an immediate refresh by updating session messages count
        // (the RightPanel depends on session.messages.length to trigger refresh)
        if (response.success) {
          // Add a system message to notify about memory stored
          setSystemMessages((prev) => [
            ...prev.slice(-9),
            {
              id: `memory-${Date.now()}`,
              type: "info",
              message: `Memory stored: ${response.memoryId?.slice(0, 8) ?? "saved"}`,
              timestamp: new Date(),
            },
          ]);
        }
      },
    });

    return () => {
      unsubscribe();
    };
  }, []);

  // Check connection status on mount
  useEffect(() => {
    getStatus()
      .then(setIsConnected)
      .catch(() => setIsConnected(false));
  }, []);

  // Load workspace config on mount (for sidebar display only)
  // Note: activeSessionId is NOT changed here - the current conversation system
  // uses 'default' session. Multi-worktree sessions will be handled separately.
  useEffect(() => {
    getWorkspaceConfig()
      .then((config) => {
        // Messages are restored through backend event stream on session resume
        // No preloading needed - backend sends history when session is resumed
        const preloadMessagesFromRollout = (
          _rolloutPath?: string | null,
        ): Message[] => {
          return [];
        };

        setRepositories(
          config.repositories.map((repo) => ({
            id: repo.id,
            name: repo.name,
            rootPath: repo.rootPath,
            worktrees: repo.worktrees,
            expanded: repo.expanded,
          })),
        );
        setWorktreeSessions(
          config.sessions.map((s) => {
            // Restore tabs from persisted config
            const restoredTabs: ChatTab[] = (s.tabs || []).map((t) => ({
              id: t.id,
              name: t.name || "",
              conversationId: t.conversationId,
              rolloutPath: t.rolloutPath,
              messages: [], // Messages restored on demand when tab becomes active
              tokenUsage: { input: 0, output: 0, total: 0 },
              createdAt: t.createdAt,
            }));

            return {
              id: s.id,
              repositoryId: s.repositoryId,
              worktreePath: s.worktreePath,
              worktreeName: s.worktreeName,
              status: "disconnected" as const, // Runtime state, default to disconnected
              lastActivity: s.lastActivity,
              currentTask: undefined, // Runtime state
              stats: { added: 0, removed: 0 },
              messages: preloadMessagesFromRollout(s.rolloutPath),
              tokenUsage: { input: 0, output: 0, total: 0 },
              rolloutPath: s.rolloutPath, // For session persistence/resume (legacy)
              conversationId: s.conversationId, // Backend conversation ID (legacy)
              tabs: restoredTabs.length > 0 ? restoredTabs : undefined,
              activeTabId: s.activeTabId,
            };
          }),
        );
        // Don't auto-start sessions on launch; user selects to resume.
      })
      .catch((error) => {
        console.warn("Failed to load workspace config:", error);
      });
  }, []);

  // Tab management handlers
  const handleSelectTab = useCallback((tabId: string) => {
    if (!activeSessionId) return;
    setWorktreeSessions(prev => prev.map(session => {
      if (session.id !== activeSessionId) return session;
      return { ...session, activeTabId: tabId };
    }));
  }, [activeSessionId]);

  const handleNewTab = useCallback(() => {
    if (!activeSessionId || !activeWorktreeSession) return;

    const newTabId = `tab-${Date.now()}`;

    // Create new tab without initializing backend session
    // Session will be initialized lazily when user sends first message
    const newTab: ChatTab = {
      id: newTabId,
      name: "", // Empty name - will be set from first message
      messages: [],
      tokenUsage: { input: 0, output: 0, total: 0 },
      createdAt: new Date().toISOString(),
      // No conversationId or rolloutPath - will be set on first message
    };

    // Add tab and switch to it
    setWorktreeSessions(prev => prev.map(session => {
      if (session.id !== activeSessionId) return session;
      const currentTabs = session.tabs || [];
      return {
        ...session,
        tabs: [...currentTabs, newTab],
        activeTabId: newTabId,
      };
    }));
  }, [activeSessionId, activeWorktreeSession]);

  const handleCloseTab = useCallback((tabId: string) => {
    if (!activeSessionId) return;

    setWorktreeSessions(prev => prev.map(session => {
      if (session.id !== activeSessionId) return session;
      const tabs = session.tabs || [];
      if (tabs.length <= 1) return session; // Don't close last tab

      const closingTab = tabs.find(t => t.id === tabId);
      const newTabs = tabs.filter(t => t.id !== tabId);
      const wasActive = session.activeTabId === tabId;

      // Save to history if tab had content (messages or conversation)
      if (closingTab && (closingTab.messages.length > 0 || closingTab.conversationId)) {
        const historyTab: TabConfigPersist = {
          id: closingTab.id,
          name: closingTab.name,
          rolloutPath: closingTab.rolloutPath,
          conversationId: closingTab.conversationId,
          createdAt: closingTab.createdAt,
        };
        addTabToHistory(session.id, historyTab).catch(console.error);
      }

      return {
        ...session,
        tabs: newTabs,
        activeTabId: wasActive ? newTabs[0]?.id : session.activeTabId,
      };
    }));
  }, [activeSessionId]);

  // Ensure session has at least one tab (migration helper)
  useEffect(() => {
    if (!activeWorktreeSession || !activeSessionId) return;
    const tabs = activeWorktreeSession.tabs || [];
    if (tabs.length === 0) {
      // Use session ID as tab ID so events route correctly
      const tabId = activeSessionId;
      const existingMessages = activeWorktreeSession.messages || [];

      // Generate tab name from first user message if exists
      const firstUserMessage = existingMessages.find(m => m.role === 'user');
      let tabName = "";
      if (firstUserMessage) {
        const cleaned = firstUserMessage.content
          .replace(/[#*`_~\[\]]/g, '')
          .replace(/\s+/g, ' ')
          .trim();
        if (cleaned.length <= 30) {
          tabName = cleaned;
        } else {
          const truncated = cleaned.slice(0, 30);
          const lastSpace = truncated.lastIndexOf(' ');
          tabName = (lastSpace > 15 ? truncated.slice(0, lastSpace) : truncated) + '…';
        }
      }

      const defaultTab: ChatTab = {
        id: tabId,
        name: tabName,
        conversationId: activeWorktreeSession.conversationId,
        rolloutPath: activeWorktreeSession.rolloutPath,
        messages: existingMessages,
        tokenUsage: activeWorktreeSession.tokenUsage,
        createdAt: new Date().toISOString(),
      };
      setWorktreeSessions(prev => prev.map(session => {
        if (session.id !== activeSessionId) return session;
        if (session.tabs && session.tabs.length > 0) return session; // Already has tabs
        return {
          ...session,
          tabs: [defaultTab],
          activeTabId: defaultTab.id,
        };
      }));
    }
  }, [activeSessionId, activeWorktreeSession?.tabs?.length]);

  // Save tabs to backend when they change (debounced)
  useEffect(() => {
    if (!activeSessionId || !activeWorktreeSession?.tabs) return;

    const timeoutId = setTimeout(() => {
      const tabsToSave: TabConfigPersist[] = activeWorktreeSession.tabs!.map(tab => ({
        id: tab.id,
        name: tab.name,
        rolloutPath: tab.rolloutPath,
        conversationId: tab.conversationId,
        createdAt: tab.createdAt,
      }));

      updateSessionTabs(activeSessionId, tabsToSave, activeWorktreeSession.activeTabId)
        .catch(err => console.error('[App] Failed to save tabs:', err));
    }, 1000); // Debounce 1 second

    return () => clearTimeout(timeoutId);
  }, [activeSessionId, activeWorktreeSession?.tabs, activeWorktreeSession?.activeTabId]);

  // Load session history when session changes
  useEffect(() => {
    if (!activeSessionId) {
      setSessionHistory([]);
      return;
    }

    getSessionHistory(activeSessionId)
      .then(setSessionHistory)
      .catch(err => {
        console.error('[App] Failed to load session history:', err);
        setSessionHistory([]);
      });
  }, [activeSessionId]);

  // Handler for restoring a tab from history
  const handleRestoreFromHistory = useCallback((historyTab: TabConfigPersist) => {
    if (!activeSessionId) return;

    // Create a new tab from the history item
    const newTab: ChatTab = {
      id: `tab-${Date.now()}`, // New ID to avoid conflicts
      name: historyTab.name,
      conversationId: historyTab.conversationId,
      rolloutPath: historyTab.rolloutPath,
      messages: [], // Messages will be restored when session is resumed
      tokenUsage: { input: 0, output: 0, total: 0 },
      createdAt: new Date().toISOString(),
    };

    // Add tab and switch to it
    setWorktreeSessions(prev => prev.map(session => {
      if (session.id !== activeSessionId) return session;
      const currentTabs = session.tabs || [];
      return {
        ...session,
        tabs: [...currentTabs, newTab],
        activeTabId: newTab.id,
      };
    }));
  }, [activeSessionId]);

  // Plan mode prompt instructions
  const getPlanPromptInstructions = () => {
    return `You are operating in Kaioken's plan-first workflow. Your next response must use the \`update_plan\` tool only.
Decide on the level of detail: keep the plan to roughly 3–4 steps for a narrow change, or expand to 6–10 focused steps when the request spans multiple files or systems.
Each step must mention the relevant files/modules or commands plus how you will verify the change (tests, linters, manual QA). Start every step with status \`pending\` unless progress is already made.`;
  };

  // Generate a short tab name from message content
  const generateTabName = (content: string): string => {
    // Remove markdown, trim whitespace
    const cleaned = content
      .replace(/[#*`_~\[\]]/g, '')
      .replace(/\s+/g, ' ')
      .trim();

    // Take first ~30 chars, break at word boundary
    if (cleaned.length <= 30) return cleaned;
    const truncated = cleaned.slice(0, 30);
    const lastSpace = truncated.lastIndexOf(' ');
    return (lastSpace > 15 ? truncated.slice(0, lastSpace) : truncated) + '…';
  };

  const handleSend = async (content: string) => {
    // Check if we have a valid worktree session and tab
    const session = worktreeSessions.find((s) => s.id === activeSessionId);
    if (!session) {
      console.error("[App] No valid session - add a repository first");
      return;
    }

    const currentTab = getActiveTab(session);
    if (!currentTab) {
      console.error("[App] No active tab");
      return;
    }

    // Check if this tab needs session initialization (lazy init)
    const needsInit = !currentTab.conversationId;
    const isFirstMessage = currentTab.messages.length === 0;

    // If plan mode is on, decorate the message
    let messageToSend = content;
    let displayContent = content;

    if (activeSettings.planMode && planWorkflowStatus === "idle") {
      // Start plan workflow
      setOriginalPlanRequest(content);
      setPlanWorkflowStatus("awaiting_plan");
      messageToSend = `${getPlanPromptInstructions()}\n\nGoal:\n${content}`;
    }

    const userMessage: Message = {
      id: Date.now().toString(),
      role: "user",
      content: displayContent,
      timestamp: new Date(),
    };

    // Generate tab name from first message
    const newTabName = isFirstMessage ? generateTabName(content) : undefined;

    // Add user message to active tab (and update name if first message)
    setWorktreeSessions((prev) =>
      prev.map((s) => {
        if (s.id !== activeSessionId) return s;
        return {
          ...s,
          tabs: (s.tabs || []).map((tab) =>
            tab.id === currentTab.id
              ? {
                  ...tab,
                  messages: [...tab.messages, userMessage],
                  name: newTabName ?? tab.name,
                }
              : tab
          ),
        };
      }),
    );

    setIsLoading(true);

    try {
      // Initialize session if needed (lazy init on first message)
      if (needsInit) {
        console.log("[App] Lazy init session for tab:", currentTab.id);
        const result = await initSession(currentTab.id, session.worktreePath);

        // Update tab with session info
        setWorktreeSessions((prev) =>
          prev.map((s) => {
            if (s.id !== activeSessionId) return s;
            return {
              ...s,
              tabs: (s.tabs || []).map((tab) =>
                tab.id === currentTab.id
                  ? {
                      ...tab,
                      conversationId: result.conversationId ?? undefined,
                      rolloutPath: result.rolloutPath ?? undefined,
                    }
                  : tab
              ),
            };
          }),
        );
      }

      // Send to backend with tab ID (each tab has its own backend session)
      await tauriSendMessage(messageToSend, currentTab.id);
    } catch (error) {
      console.error("Failed to send message:", error);
      setIsLoading(false);
    }
  };

  const handleCancel = async () => {
    if (!activeSessionId) return;
    try {
      const session = worktreeSessions.find((s) => s.id === activeSessionId);
      const currentTab = session?.tabs?.find((t) => t.id === session?.activeTabId);
      await interrupt(currentTab?.id ?? activeSessionId);

      // Add interrupted message to chat
      const interruptedMessage: Message = {
        id: `interrupted-${Date.now()}`,
        role: "assistant",
        content: "⏹ Interrupted",
        timestamp: new Date(),
      };

      if (currentTab) {
        setWorktreeSessions((prev) =>
          prev.map((s) => {
            if (s.id !== activeSessionId) return s;
            return {
              ...s,
              tabs: (s.tabs || []).map((t) =>
                t.id === currentTab.id
                  ? { ...t, messages: [...t.messages, interruptedMessage] }
                  : t
              ),
            };
          })
        );
      }

      setIsLoading(false);
    } catch (error) {
      console.error("Failed to interrupt:", error);
    }
  };

  const handleTogglePlanMode = () => {
    updateSessionSettings(activeSessionId, (prev) => ({
      ...prev,
      planMode: !prev.planMode,
    }));
    // Reset plan workflow when toggling off
    if (activeSettings.planMode) {
      setPlanWorkflowStatus("idle");
      setCurrentPlan(null);
      setPlanMessageId(null);
      setOriginalPlanRequest("");
    }
  };

  // Plan action handlers
  const handlePlanApprove = async () => {
    if (!currentPlan || !originalPlanRequest) return;

    // Turn OFF plan mode so model executes instead of creating another plan
    updateSessionSettings(activeSessionId, (prev) => ({
      ...prev,
      planMode: false,
    }));
    setPlanWorkflowStatus("executing");

    // Format the plan steps for the model
    const planSteps = currentPlan.plan
      .map((step, i) => `${i + 1}. ${step.step}`)
      .join("\n");
    const approvalMessage = `The user has approved the plan. Now execute each step in order. Use tools (read files, run commands, etc.) to complete each step. Update the plan status as you progress.

IMPORTANT: Do NOT create another plan. Execute the steps now using actual tools.

Original goal: ${originalPlanRequest}

Approved plan:
${planSteps}

Begin execution now.`;

    const userMessage: Message = {
      id: Date.now().toString(),
      role: "user",
      content: "Plan approved ✓",
      timestamp: new Date(),
    };

    setWorktreeSessions((prev) =>
      prev.map((s) =>
        s.id === activeSessionId
          ? { ...s, messages: [...s.messages, userMessage] }
          : s,
      ),
    );

    setIsLoading(true);
    try {
      await tauriSendMessage(approvalMessage, activeSessionId!);
    } catch (error) {
      console.error("Failed to send plan approval:", error);
      setIsLoading(false);
    }

    // Keep plan visible during execution (don't clear currentPlan or planMessageId)
    // The plan card will update as the model sends planUpdate events
    setOriginalPlanRequest("");
    setPlanWorkflowStatus("executing");
  };

  const handlePlanFeedback = async (feedback: string) => {
    if (!feedback.trim()) return;

    setPlanWorkflowStatus("awaiting_plan");
    const feedbackMessage = `Plan feedback:\n${feedback}\n\nPlease revise the plan and wait for approval.`;

    const userMessage: Message = {
      id: Date.now().toString(),
      role: "user",
      content: `Feedback: ${feedback}`,
      timestamp: new Date(),
    };

    setWorktreeSessions((prev) =>
      prev.map((s) =>
        s.id === activeSessionId
          ? { ...s, messages: [...s.messages, userMessage] }
          : s,
      ),
    );

    setIsLoading(true);
    try {
      await tauriSendMessage(feedbackMessage, activeSessionId!);
    } catch (error) {
      console.error("Failed to send plan feedback:", error);
      setIsLoading(false);
    }
  };

  const handlePlanCancel = () => {
    setPlanWorkflowStatus("idle");
    setCurrentPlan(null);
    setPlanMessageId(null);
    setOriginalPlanRequest("");

    // Add info message
    setSystemMessages((prev) => [
      ...prev.slice(-9),
      {
        id: `plan-cancel-${Date.now()}`,
        type: "info",
        message: "Plan cancelled",
        timestamp: new Date(),
      },
    ]);
  };

  const handleChangeTheme = (theme: ThemeName) => {
    setTheme(theme);
  };

  const handleCycleApproval = () => {
    const modes: AppSettings["approvalMode"][] = [
      "read-only",
      "auto",
      "full-access",
    ];
    const currentIndex = modes.indexOf(activeSettings.approvalMode);
    const nextIndex = (currentIndex + 1) % modes.length;
    const newMode = modes[nextIndex];
    updateSessionSettings(activeSessionId, (prev) => ({
      ...prev,
      approvalMode: newMode,
    }));
    void setApprovalMode(newMode).catch((error) => {
      console.warn("Failed to update approval mode:", error);
    });
  };

  const handleModelChange = (model: string) => {
    updateSessionSettings(activeSessionId, (prev) => ({ ...prev, model }));
    void setModel(model).catch((error) => {
      console.warn("Failed to update model:", error);
    });
  };

  const handleReasoningEffortChange = (reasoningEffort: ReasoningEffort) => {
    updateSessionSettings(activeSessionId, (prev) => ({
      ...prev,
      reasoningEffort,
    }));
    void setReasoningEffort(reasoningEffort).catch((error) => {
      console.warn("Failed to update reasoning effort:", error);
    });
  };

  // Unified settings change handler for SettingsView
  const handleSettingsChange = (updates: Partial<AppSettings>) => {
    // Handle theme separately since it's stored in its own state
    if (updates.theme !== undefined) {
      setTheme(updates.theme);
    }

    // Handle model change - call backend
    if (updates.model !== undefined) {
      void setModel(updates.model).catch((error) => {
        console.warn("Failed to update model:", error);
      });
    }

    // Handle reasoning effort change - call backend
    if (updates.reasoningEffort !== undefined) {
      void setReasoningEffort(updates.reasoningEffort).catch((error) => {
        console.warn("Failed to update reasoning effort:", error);
      });
    }

    // Handle approval mode change - call backend
    if (updates.approvalMode !== undefined) {
      void setApprovalMode(updates.approvalMode).catch((error) => {
        console.warn("Failed to update approval mode:", error);
      });
    }

    // Update session settings (excluding theme)
    const { theme: _, ...settingsUpdates } = updates;
    if (Object.keys(settingsUpdates).length > 0) {
      updateSessionSettings(activeSessionId, (prev) => ({
        ...prev,
        ...settingsUpdates,
      }));
    }
  };

  // Workspace handlers
  const handleAddRepository = useCallback(async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Select Repository Folder",
      });
      if (selected && typeof selected === "string") {
        const repo = await tauriAddRepository(selected);
        setRepositories((prev) => [...prev, repo]);

        // Auto-create a session for this repo
        const mainWorktree =
          repo.worktrees.find((w) => w.isMain) || repo.worktrees[0];
        const sessionPath = mainWorktree?.path || repo.rootPath;
        const sessionBranch = mainWorktree?.branch || "main";

        const newSessionId = `session-${Date.now()}`;
        const welcomeMessage: Message = {
          id: `welcome-${Date.now()}`,
          role: "assistant",
          content: `Ready to work on **${repo.name}**`,
          timestamp: new Date(),
        };

        const newSession: WorktreeSession = {
          id: newSessionId,
          repositoryId: repo.id,
          worktreePath: sessionPath,
          worktreeName: sessionBranch,
          status: "idle",
          lastActivity: new Date().toISOString(),
          messages: [welcomeMessage],
          tokenUsage: { input: 0, output: 0, total: 0 },
          stats: { added: 0, removed: 0 },
        };

        // Initialize backend session with the repo path FIRST
        try {
          console.log(
            "[App] Initializing session:",
            newSessionId,
            "at",
            sessionPath,
          );
          const result = await initSession(newSessionId, sessionPath);

          await upsertWorktreeSession({
            id: newSessionId,
            repositoryId: repo.id,
            worktreePath: sessionPath,
            worktreeName: sessionBranch,
            rolloutPath: result.rolloutPath ?? null,
            conversationId: result.conversationId ?? null,
          });
          newSession.rolloutPath = result.rolloutPath ?? undefined;
          newSession.conversationId = result.conversationId ?? undefined;

          // Only add session to state if backend init succeeds
          setWorktreeSessions((prev) => [...prev, newSession]);
          setActiveSessionId(newSessionId);

          // Switch to workspace view
          setAppView("workspace");
        } catch (err) {
          console.error("[App] Failed to init session:", err);
          // Don't create session in UI if backend failed
        }
      }
    } catch (error) {
      console.error("Failed to add repository:", error);
    }
  }, []);

  const handleRemoveRepository = useCallback(
    async (repositoryId: string) => {
      try {
        await tauriRemoveRepository(repositoryId);
        setRepositories((prev) => prev.filter((r) => r.id !== repositoryId));
        setWorktreeSessions((prev) => {
          const nextSessions = prev.filter(
            (s) => s.repositoryId !== repositoryId,
          );
          setActiveSessionId((current) => {
            if (current && nextSessions.some((s) => s.id === current)) {
              return current;
            }
            return nextSessions[0]?.id ?? null;
          });
          if (nextSessions.length === 0) {
            setAppView("home");
          }
          return nextSessions;
        });
      } catch (error) {
        console.error("Failed to remove repository:", error);
      }
    },
    [worktreeSessions],
  );

  // Handle cloning a repository from URL
  // Returns: undefined on success, throws on error, returns 'cancelled' if user cancelled
  const handleCloneRepo = useCallback(
    async (url: string): Promise<void | "cancelled"> => {
      // Ask user where to clone
      const destFolder = await open({
        directory: true,
        multiple: false,
        title: "Select destination folder for clone",
      });

      if (!destFolder || typeof destFolder !== "string") {
        // User cancelled the dialog - not an error
        return "cancelled";
      }

      // Extract repo name from URL
      const repoName = url.split("/").pop()?.replace(".git", "") || "repo";
      const clonePath = `${destFolder}/${repoName}`;

      // Run git clone
      console.log("[App] Cloning", url, "to", clonePath);

      // Use Tauri shell to run git clone
      const { Command } = await import("@tauri-apps/plugin-shell");
      const result = await Command.create("git", [
        "clone",
        url,
        clonePath,
      ]).execute();

      if (result.code !== 0) {
        console.error("[App] Git clone failed:", result.stderr, result.stdout);
        // Parse common git errors into user-friendly messages
        const stderr = result.stderr || "";
        let errorMsg = "Git clone failed";

        if (
          stderr.includes("Repository not found") ||
          stderr.includes("not found")
        ) {
          errorMsg = "Repository not found. Check the URL and try again.";
        } else if (
          stderr.includes("Authentication failed") ||
          stderr.includes("could not read Username")
        ) {
          errorMsg =
            "Authentication required. This may be a private repository.";
        } else if (stderr.includes("already exists")) {
          errorMsg =
            "A folder with this name already exists in the destination.";
        } else if (stderr.includes("Permission denied")) {
          errorMsg = "Permission denied. Check your access rights.";
        } else if (stderr.trim()) {
          // Show first line of stderr for other errors
          errorMsg =
            stderr
              .split("\n")[0]
              .replace(/^(fatal|error):\s*/i, "")
              .trim() || "Git clone failed";
        }

        throw new Error(errorMsg);
      }

      console.log("[App] Clone successful, adding repository");

      // Add the cloned repo
      const repo = await tauriAddRepository(clonePath);
      setRepositories((prev) => [...prev, repo]);

      // Auto-create a session for this repo
      const mainWorktree =
        repo.worktrees.find((w) => w.isMain) || repo.worktrees[0];
      const sessionPath = mainWorktree?.path || repo.rootPath;
      const sessionBranch = mainWorktree?.branch || "main";

      const newSessionId = `session-${Date.now()}`;
      const welcomeMessage: Message = {
        id: `welcome-${Date.now()}`,
        role: "assistant",
        content: `Cloned and ready to work on **${repo.name}**`,
        timestamp: new Date(),
      };

      const newSession: WorktreeSession = {
        id: newSessionId,
        repositoryId: repo.id,
        worktreePath: sessionPath,
        worktreeName: sessionBranch,
        status: "idle",
        lastActivity: new Date().toISOString(),
        messages: [welcomeMessage],
        tokenUsage: { input: 0, output: 0, total: 0 },
        stats: { added: 0, removed: 0 },
      };

      // Initialize backend session
      const initResult = await initSession(newSessionId, sessionPath);
      await upsertWorktreeSession({
        id: newSessionId,
        repositoryId: repo.id,
        worktreePath: sessionPath,
        worktreeName: sessionBranch,
        rolloutPath: initResult.rolloutPath ?? null,
        conversationId: initResult.conversationId ?? null,
      });
      newSession.rolloutPath = initResult.rolloutPath ?? undefined;
      newSession.conversationId = initResult.conversationId ?? undefined;
      setWorktreeSessions((prev) => [...prev, newSession]);
      setActiveSessionId(newSessionId);
      setAppView("workspace");
    },
    [],
  );

  // Handle selecting a worktree session - defined before handleSelectRepoFromHome since it's used there
  const handleSelectWorktreeSession = useCallback(
    async (sessionId: string, worktreePath?: string) => {
      console.log(
        "[App] handleSelectWorktreeSession:",
        sessionId,
        worktreePath,
      );

      // Check if session already exists in worktreeSessions
      const existingSession = worktreeSessions.find((s) => s.id === sessionId);

      if (existingSession) {
        // Session exists in state - check if it needs to be resumed
        if (existingSession.status === "disconnected") {
          console.log(
            "[App] Session exists but disconnected, resuming:",
            sessionId,
          );

          // Get rollout path from existing session or fetch from backend
          const rolloutPath =
            existingSession.rolloutPath || (await getSessionRollout(sessionId));
          const sessionPath = existingSession.worktreePath;

          try {
            console.log(
              "[App] Resuming session:",
              sessionId,
              sessionPath,
              "rollout:",
              rolloutPath,
            );
            const result = await initSession(
              sessionId,
              sessionPath,
              rolloutPath || undefined,
            );
            console.log("[App] Session resumed successfully");

            await upsertWorktreeSession({
              id: sessionId,
              repositoryId: existingSession.repositoryId,
              worktreePath: sessionPath,
              worktreeName: existingSession.worktreeName,
              rolloutPath: result.rolloutPath ?? rolloutPath ?? null,
              conversationId:
                result.conversationId ?? existingSession.conversationId ?? null,
            });

            // Update session state with restored data
            setWorktreeSessions((prev) =>
              prev.map((s) => {
                if (s.id !== sessionId) return s;

                // Restore messages from rollout if available
                let restoredMessages = s.messages;
                if (result.messages && result.messages.length > 0) {
                  console.log(
                    "[App] Restoring",
                    result.messages.length,
                    "messages from rollout",
                  );
                  restoredMessages = result.messages.map((msg, index) => ({
                    id: `restored-${index}-${Date.now()}`,
                    role: msg.role as "user" | "assistant",
                    content: msg.content,
                    timestamp: new Date(),
                  }));
                }

                return {
                  ...s,
                  status: "idle" as const,
                  messages: restoredMessages,
                  rolloutPath: result.rolloutPath ?? s.rolloutPath,
                  conversationId: result.conversationId ?? s.conversationId,
                };
              }),
            );

            setActiveSessionId(sessionId);
            setAppView("workspace");
            await upsertWorktreeSession({
              id: sessionId,
              repositoryId: existingSession.repositoryId,
              worktreePath: existingSession.worktreePath,
              worktreeName: existingSession.worktreeName,
              rolloutPath: existingSession.rolloutPath ?? null,
              conversationId: existingSession.conversationId ?? null,
            });
          } catch (error) {
            console.error("[App] Failed to resume session:", error);
          }
          return;
        }

        // Session exists and is connected - just switch to it
        console.log(
          "[App] Session exists and connected, switching to:",
          sessionId,
        );
        setActiveSessionId(sessionId);
        setAppView("workspace");
        await upsertWorktreeSession({
          id: sessionId,
          repositoryId: existingSession.repositoryId,
          worktreePath: existingSession.worktreePath,
          worktreeName: existingSession.worktreeName,
          rolloutPath: existingSession.rolloutPath ?? null,
          conversationId: existingSession.conversationId ?? null,
        });
        return;
      }

      // Create new session for this worktree
      if (!worktreePath) {
        console.warn(
          "[App] No worktreePath provided for new session:",
          sessionId,
        );
        return;
      }

      console.log("[App] Creating new session for worktree:", worktreePath);
      const worktreeName = worktreePath.split("/").pop() || "worktree";

      // Find the repository for this worktree
      const repo = repositories.find((r) =>
        r.worktrees.some((w) => w.path === worktreePath),
      );

      const newSession: WorktreeSession = {
        id: sessionId,
        repositoryId: repo?.id || "",
        worktreePath: worktreePath,
        worktreeName: worktreeName,
        status: "idle",
        lastActivity: new Date().toISOString(),
        messages: [],
        tokenUsage: { input: 0, output: 0, total: 0 },
        stats: { added: 0, removed: 0 },
      };

      // Check if there's a saved rollout path for this session (for resume)
      const savedRolloutPath = await getSessionRollout(sessionId);

      // Initialize backend session first (with rollout path for resume if available)
      try {
        console.log(
          "[App] Initializing backend session:",
          sessionId,
          worktreePath,
          "rollout:",
          savedRolloutPath,
        );
        const result = await initSession(
          sessionId,
          worktreePath,
          savedRolloutPath || undefined,
        );
        console.log("[App] Backend session initialized successfully");

        if (repo) {
          await upsertWorktreeSession({
            id: sessionId,
            repositoryId: repo.id,
            worktreePath,
            worktreeName,
            rolloutPath: result.rolloutPath ?? savedRolloutPath ?? null,
            conversationId: result.conversationId ?? null,
          });
        } else {
          console.warn(
            "[App] Skipping session persistence because repository was not found for worktree",
            worktreePath,
          );
        }
        newSession.rolloutPath = result.rolloutPath ?? undefined;
        newSession.conversationId = result.conversationId ?? undefined;

        // Restore messages from rollout if resuming
        if (result.messages && result.messages.length > 0) {
          console.log(
            "[App] Restoring",
            result.messages.length,
            "messages from rollout",
          );
          const restoredMessages: Message[] = result.messages.map(
            (msg, index) => ({
              id: `restored-${index}-${Date.now()}`,
              role: msg.role as "user" | "assistant",
              content: msg.content,
              timestamp: new Date(),
            }),
          );
          newSession.messages = restoredMessages;
        }

        // Only add session to state if backend init succeeds
        setWorktreeSessions((prev) => [...prev, newSession]);
        setActiveSessionId(sessionId);
        setAppView("workspace");
      } catch (error) {
        console.error("[App] Failed to initialize session:", error);
      }
    },
    [worktreeSessions, repositories],
  );

  const handleCreateSession = useCallback(
    (repositoryId: string) => {
      const repo = repositories.find((r) => r.id === repositoryId);
      if (repo) {
        setWorktreeModalRepo(repo);
      }
    },
    [repositories],
  );

  const handleSessionCreate = useCallback(
    async (sessionName: string, branchName: string, worktreePath: string) => {
      if (!worktreeModalRepo) return;
      console.log("[App] Creating session:", {
        repoId: worktreeModalRepo.id,
        sessionName,
        branchName,
        worktreePath,
      });

      // Create the git worktree
      const worktree = await tauriCreateWorktree(
        worktreeModalRepo.id,
        branchName,
        worktreePath,
      );
      console.log("[App] Worktree created:", worktree);

      // Add the new worktree to the repository
      setRepositories((prev) => {
        const updated = prev.map((repo) =>
          repo.id === worktreeModalRepo.id
            ? { ...repo, worktrees: [...repo.worktrees, worktree] }
            : repo,
        );
        return updated;
      });

      // Create a new session for this worktree with canned greeting
      const newSessionId = `session-${Date.now()}`;
      const greeting: Message = {
        id: `greeting-${Date.now()}`,
        role: "assistant",
        content: `Session started: **${sessionName}**\n\nI'm ready to help you work on this. What would you like to do?`,
        timestamp: new Date(),
      };

      const newSession: WorktreeSession = {
        id: newSessionId,
        repositoryId: worktreeModalRepo.id,
        worktreePath: worktreePath,
        worktreeName: sessionName, // Use task name as display name
        status: "idle",
        lastActivity: new Date().toISOString(),
        messages: [greeting],
        tokenUsage: { input: 0, output: 0, total: 0 },
        stats: { added: 0, removed: 0 },
      };

      // Initialize the backend session first
      try {
        const result = await initSession(newSessionId, worktreePath);

        await upsertWorktreeSession({
          id: newSessionId,
          repositoryId: worktreeModalRepo.id,
          worktreePath,
          worktreeName: sessionName,
          rolloutPath: result.rolloutPath ?? null,
          conversationId: result.conversationId ?? null,
        });
        newSession.rolloutPath = result.rolloutPath ?? undefined;
        newSession.conversationId = result.conversationId ?? undefined;

        setWorktreeSessions((prev) => [...prev, newSession]);
        setActiveSessionId(newSessionId);
        setAppView("workspace");
      } catch (err) {
        console.error("[App] Failed to init session:", err);
      }
    },
    [worktreeModalRepo],
  );

  const handleApprove = async (id: string) => {
    if (!approvalRequest) return;
    try {
      // Get sessionId from approval request (added in onApprovalRequest callback)
      const sessionId = (
        approvalRequest as ApprovalRequestEvent & { sessionId?: string }
      ).sessionId;
      await sendApproval(id, approvalRequest.kind, true, sessionId);
      setApprovalRequest(null);
    } catch (error) {
      console.error("Failed to send approval:", error);
    }
  };

  const handleDeny = async (id: string) => {
    if (!approvalRequest) return;
    try {
      // Get sessionId from approval request (added in onApprovalRequest callback)
      const sessionId = (
        approvalRequest as ApprovalRequestEvent & { sessionId?: string }
      ).sessionId;
      await sendApproval(id, approvalRequest.kind, false, sessionId);
      setApprovalRequest(null);
    } catch (error) {
      console.error("Failed to send denial:", error);
    }
  };

  // Homepage view - shown on app launch
  if (appView === "home") {
    return (
      <>
        {/* Modals need to stay available */}
        {approvalRequest && (
          <ApprovalModal
            request={approvalRequest}
            onApprove={handleApprove}
            onDeny={handleDeny}
          />
        )}
        <HomePage
          onOpenFolder={handleAddRepository}
          onCloneRepo={handleCloneRepo}
        />
      </>
    );
  }

  // Workspace view - shown after selecting a repo
  return (
    <div className="flex flex-col h-screen bg-background overflow-hidden">
      {/* Approval Modal */}
      {approvalRequest && (
        <ApprovalModal
          request={approvalRequest}
          onApprove={handleApprove}
          onDeny={handleDeny}
        />
      )}

      {/* Create Session Modal */}
      {worktreeModalRepo && (
        <CreateSessionModal
          repository={worktreeModalRepo}
          onClose={() => setWorktreeModalRepo(null)}
          onCreate={handleSessionCreate}
        />
      )}

      {/* Settings View */}
      {settingsOpen && (
        <SettingsView
          settings={activeSettings}
          onClose={() => setSettingsOpen(false)}
          onSettingsChange={handleSettingsChange}
        />
      )}

      {/* Unified Header */}
      <Header
        settings={activeSettings}
        onOpenSettings={() => setSettingsOpen(!settingsOpen)}
        onChangeTheme={handleChangeTheme}
      />

      {/* 3-Panel Grid Layout */}
      <div className="flex flex-1 overflow-hidden">
        {/* Left Panel: Sidebar */}
        <div style={{ width: leftPanelWidth }} className="flex-shrink-0 relative">
          <Sidebar
            repositories={repositories}
            sessions={worktreeSessions}
            activeSessionId={activeSessionId}
            onSelectSession={handleSelectWorktreeSession}
            onAddRepository={handleAddRepository}
            onCreateSession={handleCreateSession}
            onRemoveRepository={handleRemoveRepository}
          />
          {/* Right edge resize area */}
          <div
            onMouseDown={(e) => startResize('left', e)}
            className={`absolute top-0 right-0 w-1 h-full cursor-col-resize hover:bg-primary/30 transition-colors ${resizing === 'left' ? 'bg-primary/30' : ''}`}
          />
        </div>

        {/* Center Panel: Feed + Input */}
        <div className="flex-1 flex flex-col min-w-0">
          {/* Chat Tabs */}
          {activeWorktreeSession && (activeWorktreeSession.tabs?.length || 0) > 0 && (
            <ChatTabs
              tabs={activeWorktreeSession.tabs || []}
              activeTabId={activeWorktreeSession.activeTabId || activeWorktreeSession.tabs?.[0]?.id || ""}
              onSelectTab={handleSelectTab}
              onNewTab={handleNewTab}
              onCloseTab={handleCloseTab}
              history={sessionHistory}
              onRestoreFromHistory={handleRestoreFromHistory}
            />
          )}

          {/* Feed */}
          <div className="flex-1 overflow-y-auto px-6">
            <div className="w-full">
              {/* Show appropriate content based on state */}
              {(() => {
                const activeWorktreeSession = worktreeSessions.find(
                  (s) => s.id === activeSessionId,
                );
                const activeRepo = activeWorktreeSession
                  ? repositories.find(
                      (r) => r.id === activeWorktreeSession.repositoryId,
                    )
                  : null;

                // No repo/session - show get started
                if (!activeWorktreeSession || !activeRepo) {
                  return (
                    <div className="flex flex-col items-center justify-center py-20 px-4">
                      <div className="w-14 h-14 rounded-2xl bg-muted/50 flex items-center justify-center mb-5">
                        <span className="text-2xl">📁</span>
                      </div>
                      <h2 className="text-lg font-semibold text-foreground mb-2">
                        Get Started
                      </h2>
                      <p className="text-sm text-muted-foreground mb-6 text-center max-w-md">
                        Add a repository to start working with Kaioken.
                      </p>
                      <button
                        onClick={handleAddRepository}
                        className="px-4 py-2 text-sm font-medium bg-accent text-accent-foreground rounded-lg hover:bg-accent/90 transition-colors"
                      >
                        Open Folder
                      </button>
                    </div>
                  );
                }

                const hasUserMessages =
                  activeSession?.messages.some((m) => m.role === "user") ??
                  false;

                // Has session but no messages - show welcome
                if (!hasUserMessages) {
                  return (
                    <WelcomeCard
                      repoName={activeRepo.name}
                      branchName={activeWorktreeSession?.worktreeName}
                      worktreePath={activeWorktreeSession?.worktreePath}
                    />
                  );
                }

                // Has messages - render them
                return activeSession!.messages.map((message, index) => (
                  <ChatMessage
                    key={message.id}
                    message={message}
                    planWorkflowStatus={planWorkflowStatus}
                    currentPlan={currentPlan}
                    planMessageId={planMessageId}
                    onPlanApprove={handlePlanApprove}
                    onPlanFeedback={handlePlanFeedback}
                    onPlanCancel={handlePlanCancel}
                    isLastMessage={index === activeSession!.messages.length - 1}
                  />
                ));
              })()}

              {/* Waiting for AI response indicator */}
              {isLoading && (
                <div className="flex items-end gap-[3px] h-4 mt-2 mb-2">
                  <span className="w-[3px] bg-foreground/50 rounded-full animate-equalizer-1" />
                  <span className="w-[3px] bg-foreground/50 rounded-full animate-equalizer-2" />
                  <span className="w-[3px] bg-foreground/50 rounded-full animate-equalizer-3" />
                  <span className="w-[3px] bg-foreground/50 rounded-full animate-equalizer-4" />
                </div>
              )}

              <div ref={messagesEndRef} />
            </div>
          </div>

          {/* Input */}
          <InputArea
            onSend={handleSend}
            onCancel={handleCancel}
            isLoading={isLoading}
            settings={activeSettings}
            onTogglePlanMode={handleTogglePlanMode}
            onCycleApproval={handleCycleApproval}
            onModelChange={handleModelChange}
            onReasoningEffortChange={handleReasoningEffortChange}
            sessionId={activeTab?.id ?? activeSessionId ?? undefined}
          />
        </div>

        {/* Right Panel: Context */}
        <div style={{ width: rightPanelWidth }} className="flex-shrink-0 relative">
          {/* Left edge resize area */}
          <div
            onMouseDown={(e) => startResize('right', e)}
            className={`absolute top-0 left-0 w-1 h-full cursor-col-resize hover:bg-primary/30 transition-colors z-10 ${resizing === 'right' ? 'bg-primary/30' : ''}`}
          />
          <RightPanel
            session={activeSession!}
            tokenUsage={activeTokenUsage}
            systemMessages={systemMessages}
            worktreePath={worktreeSessions.find(s => s.id === activeSessionId)?.worktreePath}
          />
        </div>
      </div>
    </div>
  );
}

export default App;
