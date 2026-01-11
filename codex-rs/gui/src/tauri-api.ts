import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Message, ReasoningEffort, ToolExecution, PlanUpdate } from './types';

// Types matching Rust backend
interface SendMessageResponse {
    message: {
        id: string;
        role: 'user' | 'assistant';
        content: string;
        timestamp: number;
        toolCalls: ToolExecution[] | null;
    };
    tokenUsage: { input: number; output: number; total: number } | null;
}

interface GuiEvent {
    eventType?: 'message' | 'toolStart' | 'toolEnd' | 'contentDelta' | 'approvalNeeded' | 'connected' | 'tokenUsage' | 'error';
    type?: string; // For backward compat with JSON events
    [key: string]: unknown;
}

interface ThreadResponse {
    thread: {
        id: string;
        preview: string;
        createdAt: number;
    };
    model: string;
}

// Convert backend timestamp (ms) to Date
function timestampToDate(ts: number): Date {
    return new Date(ts);
}

// Convert backend tool execution format to frontend
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function convertToolExecution(tool: any): ToolExecution {
    return {
        id: tool.id as string,
        // Handle both 'toolType' (new) and 'type' (old) field names
        type: (tool.toolType || tool.type) as ToolExecution['type'],
        name: tool.name as string,
        status: tool.status as ToolExecution['status'],
        input: tool.input,
        output: tool.output as string | undefined,
        error: tool.error as string | undefined,
        startTime: timestampToDate(tool.startTime as number),
        endTime: tool.endTime ? timestampToDate(tool.endTime as number) : undefined,
    };
}

// Convert backend message format to frontend
function convertMessage(msg: SendMessageResponse['message']): Message {
    return {
        id: msg.id,
        role: msg.role,
        content: msg.content,
        timestamp: timestampToDate(msg.timestamp),
        toolCalls: msg.toolCalls?.map(convertToolExecution) ?? undefined,
    };
}

/**
 * A restored message from rollout file
 */
export interface RestoredMessage {
    role: 'user' | 'assistant';
    content: string;
}

/**
 * Response from init_session containing conversation info for persistence
 */
export interface InitSessionResponse {
    success: boolean;
    /** Backend conversation ID (UUID) - store this for resuming */
    conversationId: string | null;
    /** Path to rollout file - store this for resuming */
    rolloutPath: string | null;
    /** Restored messages from rollout (only when resuming) */
    messages: RestoredMessage[];
}

/**
 * Initialize a session with specific sessionId and working directory
 * If rolloutPath is provided, resumes from that session instead of creating new
 */
export async function initSession(sessionId: string, cwd: string, rolloutPath?: string): Promise<InitSessionResponse> {
    return invoke<InitSessionResponse>('init_session', { sessionId, cwd, rolloutPath: rolloutPath ?? null });
}

/**
 * Initialize the default conversation with codex-core (backward compat)
 */
export async function initAppServer(): Promise<boolean> {
    return invoke<boolean>('init_conversation');
}

/**
 * Start a new thread/conversation
 */
export async function startThread(model?: string, reasoningEffort?: string): Promise<ThreadResponse> {
    return invoke<ThreadResponse>('start_thread', { model, reasoningEffort });
}

/**
 * Send a message to Kaioken backend
 */
export async function sendMessage(message: string, sessionId?: string): Promise<Message> {
    const response = await invoke<SendMessageResponse>('send_message', { sessionId, message });
    return convertMessage(response.message);
}

/**
 * Get all messages in a session
 */
export async function getMessages(sessionId?: string): Promise<Message[]> {
    const messages = await invoke<SendMessageResponse['message'][]>('get_messages', { sessionId });
    return messages.map(convertMessage);
}

/**
 * Check if session exists/is connected
 */
export async function getStatus(sessionId?: string): Promise<boolean> {
    return invoke<boolean>('get_status', { sessionId });
}

/**
 * Clear conversation history for a session
 */
export async function clearConversation(sessionId?: string): Promise<void> {
    await invoke('clear_conversation', { sessionId });
}

/**
 * Set the active model for the backend
 */
export async function setModel(model: string): Promise<void> {
    await invoke('set_model', { model });
}

/**
 * Set the active reasoning effort for the backend
 */
export async function setReasoningEffort(reasoningEffort: ReasoningEffort): Promise<void> {
    await invoke('set_reasoning_effort', { reasoningEffort });
}

/**
 * Set the approval mode preset (matches TUI)
 * - "read-only" -> Requires approval to edit files and run commands
 * - "auto" -> Read and edit files, and run commands (Agent mode)
 * - "full-access" -> Full access with no sandbox restrictions
 */
export async function setApprovalMode(presetId: 'read-only' | 'auto' | 'full-access'): Promise<void> {
    await invoke('set_approval_mode', { presetId });
}

/**
 * Send approval decision for exec or patch
 */
export async function sendApproval(id: string, kind: 'exec' | 'patch', approved: boolean, sessionId?: string): Promise<void> {
    await invoke('send_approval', { sessionId, id, kind, approved });
}

/**
 * Interrupt a running session
 */
export async function interrupt(sessionId?: string): Promise<void> {
    await invoke('interrupt', { sessionId });
}

export interface ExecOutputDelta {
    callId: string;
    stream: 'stdout' | 'stderr';
    chunk: string;
}

export interface TokenUsageEvent {
    total: { input: number; output: number; cached: number; reasoning: number; total: number };
    last: { input: number; output: number; total: number };
    contextWindow: number | null;
}

export interface ApprovalRequestEvent {
    kind: 'exec' | 'patch';
    id: string;
    command?: string[];
    cwd?: string;
    files?: string[];
    reasoning?: string;
}

export interface SubagentEvent {
    callId: string;
    task: string;
    status?: string;
    summary?: string;
    line?: string;
    agentIndex?: number;
}

export interface EventCallbacks {
    onMessage?: (msg: Message) => void;
    onToolStart?: (tool: ToolExecution) => void;
    onToolEnd?: (tool: ToolExecution) => void;
    onError?: (error: string) => void;
    onContentDelta?: (delta: string) => void;
    onTaskComplete?: () => void;
    onReasoningDelta?: (delta: string) => void;
    onExecOutputDelta?: (delta: ExecOutputDelta) => void;
    onTokenUsage?: (usage: TokenUsageEvent) => void;
    onApprovalRequest?: (req: ApprovalRequestEvent) => void;
    onWarning?: (message: string) => void;
    onBackground?: (message: string) => void;
    onSubagentUpdate?: (event: SubagentEvent) => void;
    onSubagentLog?: (event: SubagentEvent) => void;
    onTaskStarted?: () => void;
    onContextCompacted?: () => void;
    onPlanUpdate?: (plan: PlanUpdate) => void;
}

/**
 * Subscribe to Kaioken events
 */
export function subscribeToEvents(
    onMessage: (msg: Message) => void,
    onToolStart?: (tool: ToolExecution) => void,
    onToolEnd?: (tool: ToolExecution) => void,
    onError?: (error: string) => void,
    onContentDelta?: (delta: string) => void,
    onTaskComplete?: () => void,
    onReasoningDelta?: (delta: string) => void,
    onExecOutputDelta?: (delta: ExecOutputDelta) => void,
    callbacks?: EventCallbacks,
): () => void {
    console.log('[tauri-api] Setting up listener for kaioken-event');
    const unlisten = listen<GuiEvent>('kaioken-event', (event) => {
        console.log('[tauri-api] RAW EVENT:', event);
        console.log('[tauri-api] RAW PAYLOAD:', event.payload);
        const normalized = normalizePayload(event.payload);
        console.log('[tauri-api] normalized:', normalized);
        if (!normalized) {
            console.warn('[tauri-api] normalized is null, skipping');
            return;
        }
        const payload =
            normalized.payload && typeof normalized.payload === 'object'
                ? normalized.payload
                : normalized;
        // Look for eventType (from tagged enum) or type (from JSON objects)
        const type =
            typeof normalized.eventType === 'string'
                ? normalized.eventType
                : typeof normalized.type === 'string'
                    ? normalized.type
                    : typeof (payload as GuiEvent).eventType === 'string'
                        ? (payload as GuiEvent).eventType
                        : typeof (payload as GuiEvent).type === 'string'
                            ? (payload as GuiEvent).type
                            : undefined;
        if (!type) {
            console.warn('[tauri-api] no type found in:', normalized);
            return;
        }

        console.log('%c[tauri-api] DISPATCHING: ' + type, 'color: lime; font-weight: bold', payload);
        switch (type) {
            case 'message':
                onMessage(convertMessage(extractMessagePayload(payload)));
                break;
            case 'toolStart':
                console.log('[tauri-api] toolStart payload:', JSON.stringify(payload));
                if (onToolStart) {
                    const tool = convertToolExecution(payload);
                    console.log('[tauri-api] converted tool:', JSON.stringify(tool));
                    console.log('[tauri-api] tool.id:', tool.id, 'tool.type:', tool.type, 'tool.name:', tool.name);
                    onToolStart(tool);
                } else {
                    console.warn('[tauri-api] onToolStart callback not provided');
                }
                break;
            case 'toolEnd':
                console.log('[tauri-api] toolEnd payload:', payload);
                if (onToolEnd) {
                    const tool = convertToolExecution(payload);
                    console.log('[tauri-api] calling onToolEnd with:', tool);
                    onToolEnd(tool);
                } else {
                    console.warn('[tauri-api] onToolEnd callback not provided');
                }
                break;
            case 'error':
                if ('message' in payload && typeof payload.message === 'string') {
                    onError?.(payload.message);
                }
                break;
            case 'contentDelta':
                if ('delta' in payload && typeof payload.delta === 'string') {
                    onContentDelta?.(payload.delta);
                }
                break;
            case 'reasoningDelta':
                console.log('[tauri-api] reasoningDelta payload:', payload);
                if ('delta' in payload && typeof payload.delta === 'string') {
                    console.log('[tauri-api] calling onReasoningDelta with:', payload.delta.slice(0, 50));
                    onReasoningDelta?.(payload.delta);
                } else {
                    console.warn('[tauri-api] reasoningDelta missing delta field:', payload);
                }
                break;
            case 'execOutputDelta':
                if ('callId' in payload && 'stream' in payload && 'chunk' in payload) {
                    onExecOutputDelta?.({
                        callId: payload.callId as string,
                        stream: payload.stream as 'stdout' | 'stderr',
                        chunk: payload.chunk as string,
                    });
                }
                break;
            case 'taskComplete':
                onTaskComplete?.();
                break;
            case 'taskStarted':
                callbacks?.onTaskStarted?.();
                break;
            case 'tokenUsage':
                if ('total' in payload && 'last' in payload) {
                    callbacks?.onTokenUsage?.(payload as unknown as TokenUsageEvent);
                }
                break;
            case 'approvalRequest':
                callbacks?.onApprovalRequest?.(payload as unknown as ApprovalRequestEvent);
                break;
            case 'warning':
                if ('message' in payload && typeof payload.message === 'string') {
                    callbacks?.onWarning?.(payload.message);
                }
                break;
            case 'background':
                if ('message' in payload && typeof payload.message === 'string') {
                    callbacks?.onBackground?.(payload.message);
                }
                break;
            case 'subagentUpdate':
                callbacks?.onSubagentUpdate?.(payload as unknown as SubagentEvent);
                break;
            case 'subagentLog':
                callbacks?.onSubagentLog?.(payload as unknown as SubagentEvent);
                break;
            case 'subagentHistory':
                callbacks?.onSubagentUpdate?.(payload as unknown as SubagentEvent);
                break;
            case 'contextCompacted':
                callbacks?.onContextCompacted?.();
                break;
            case 'planUpdate':
                // payload.plan contains { explanation, plan: [...] }
                const planData = (payload as { plan?: { explanation?: string; plan?: unknown[] } }).plan;
                if (planData && Array.isArray(planData.plan)) {
                    callbacks?.onPlanUpdate?.({
                        explanation: planData.explanation,
                        plan: planData.plan as PlanUpdate['plan'],
                    });
                }
                break;
            case 'streamError':
                if ('message' in payload && typeof payload.message === 'string') {
                    onError?.(payload.message);
                }
                break;
        }
    });

    // Return cleanup function
    return () => {
        unlisten.then((fn) => fn());
    };
}

function normalizePayload(payload: GuiEvent | string | null | undefined): GuiEvent | null {
    if (!payload) {
        return null;
    }
    if (typeof payload === 'string') {
        try {
            return JSON.parse(payload) as GuiEvent;
        } catch {
            return null;
        }
    }
    return payload as GuiEvent;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function extractMessagePayload(payload: any): SendMessageResponse['message'] {
    if ('message' in payload) {
        return payload.message as SendMessageResponse['message'];
    }
    if ('payload' in payload && typeof payload.payload === 'object' && payload.payload) {
        return payload.payload as SendMessageResponse['message'];
    }
    if ('0' in payload) {
        return payload['0'] as SendMessageResponse['message'];
    }
    return payload as SendMessageResponse['message'];
}

// ============================================================================
// Workspace Management API
// ============================================================================

import {
    GuiWorkspacesConfig,
    Repository,
    WorktreeInfo,
    DetectedGitInfo,
} from './types';

/**
 * Get the current workspace configuration
 */
export async function getWorkspaceConfig(): Promise<GuiWorkspacesConfig> {
    return invoke<GuiWorkspacesConfig>('get_workspace_config');
}

/**
 * Add a repository to the workspace
 */
export async function addRepository(path: string): Promise<Repository> {
    return invoke<Repository>('add_repository', { path });
}

/**
 * Remove a repository from the workspace
 */
export async function removeRepository(repositoryId: string): Promise<void> {
    await invoke('remove_repository', { repositoryId });
}

/**
 * List git worktrees for a repository
 */
export async function listGitWorktrees(repoPath: string): Promise<WorktreeInfo[]> {
    return invoke<WorktreeInfo[]>('list_git_worktrees', { repoPath });
}

/**
 * Create a new git worktree
 */
export async function createWorktree(
    repositoryId: string,
    branchName: string,
    worktreePath: string
): Promise<WorktreeInfo> {
    return invoke<WorktreeInfo>('create_worktree', { repositoryId, branchName, worktreePath });
}

/**
 * Detect git info for a path
 */
export async function detectGitInfo(path: string): Promise<DetectedGitInfo> {
    return invoke<DetectedGitInfo>('detect_git_info', { path });
}

/**
 * Add or update a worktree session entry (metadata + rollout) in the workspace config
 */
export async function upsertWorktreeSession(params: {
    id: string;
    repositoryId: string;
    worktreePath: string;
    worktreeName: string;
    rolloutPath?: string | null;
    conversationId?: string | null;
}): Promise<void> {
    const { id, repositoryId, worktreePath, worktreeName, rolloutPath, conversationId } = params;
    await invoke('upsert_worktree_session', {
        sessionId: id,
        repositoryId,
        worktreePath,
        worktreeName,
        rolloutPath: rolloutPath ?? null,
        conversationId: conversationId ?? null,
    });
}

/**
 * Start a worktree session (spawns codex backend process)
 */
export async function startWorktreeSession(sessionId: string, worktreePath: string): Promise<void> {
    await invoke('start_worktree_session', { sessionId, worktreePath });
}

/**
 * Stop a worktree session (kills codex backend process)
 */
export async function stopWorktreeSession(sessionId: string): Promise<void> {
    await invoke('stop_worktree_session', { sessionId });
}

/**
 * Update session with rollout path and conversation ID for persistence
 */
export async function updateSessionRollout(
    sessionId: string,
    rolloutPath: string | null,
    conversationId: string | null
): Promise<void> {
    await invoke('update_session_rollout', { sessionId, rolloutPath, conversationId });
}

/**
 * Get session rollout path for resuming
 */
export async function getSessionRollout(sessionId: string): Promise<string | null> {
    return invoke<string | null>('get_session_rollout', { sessionId });
}

/**
 * Tab config for persistence
 */
export interface TabConfigPersist {
    id: string;
    name: string;
    rolloutPath?: string;
    conversationId?: string;
    createdAt: string;
}

/**
 * Update session tabs (save open tabs and active tab)
 */
export async function updateSessionTabs(
    sessionId: string,
    tabs: TabConfigPersist[],
    activeTabId?: string,
): Promise<void> {
    await invoke('update_session_tabs', { sessionId, tabs, activeTabId });
}

/**
 * Add a tab to session history (when closing a tab with content)
 */
export async function addTabToHistory(sessionId: string, tab: TabConfigPersist): Promise<void> {
    await invoke('add_tab_to_history', { sessionId, tab });
}

/**
 * Get session chat history (closed tabs)
 */
export async function getSessionHistory(sessionId: string): Promise<TabConfigPersist[]> {
    return invoke<TabConfigPersist[]>('get_session_history', { sessionId });
}

/**
 * Rate limit window info
 */
export interface RateLimitWindow {
    usedPercent: number;
    windowMinutes: number | null;
    resetsAt: number | null;
}

/**
 * Rate limits response
 */
export interface RateLimitsResponse {
    primary: RateLimitWindow | null;
    secondary: RateLimitWindow | null;
    hasCredits: boolean | null;
    creditsBalance: string | null;
}

/**
 * Fetch rate limits from the API
 */
export async function getRateLimits(): Promise<RateLimitsResponse> {
    return invoke<RateLimitsResponse>('get_rate_limits');
}

/**
 * List files in a directory for @-mention autocomplete
 */
export async function listFiles(sessionId: string, query?: string): Promise<string[]> {
    return invoke<string[]>('list_files', { sessionId, query: query ?? '' });
}

/**
 * Memory entry from the database
 */
export interface MemoryEntry {
    id: string;
    memory_type: 'fact' | 'pattern' | 'decision' | 'lesson' | 'preference' | 'location';
    content: string;
    importance: number;
    created_at: number;
}

/**
 * Get memories for a project
 */
export async function getMemories(cwd: string): Promise<MemoryEntry[]> {
    return invoke<MemoryEntry[]>('get_memories', { cwd });
}

/**
 * Session event with sessionId at top level
 */
export interface SessionEvent extends GuiEvent {
    sessionId: string;
}

/**
 * Session-aware callbacks (same as EventCallbacks but receives sessionId)
 */
export interface SessionEventCallbacks {
    onMessage?: (sessionId: string, msg: Message) => void;
    onToolStart?: (sessionId: string, tool: ToolExecution) => void;
    onToolEnd?: (sessionId: string, tool: ToolExecution) => void;
    onError?: (sessionId: string, error: string) => void;
    onContentDelta?: (sessionId: string, delta: string) => void;
    onTaskComplete?: (sessionId: string) => void;
    onReasoningDelta?: (sessionId: string, delta: string) => void;
    onExecOutputDelta?: (sessionId: string, delta: ExecOutputDelta) => void;
    onTokenUsage?: (sessionId: string, usage: TokenUsageEvent) => void;
    onApprovalRequest?: (sessionId: string, req: ApprovalRequestEvent) => void;
    onWarning?: (sessionId: string, message: string) => void;
    onBackground?: (sessionId: string, message: string) => void;
    onSubagentUpdate?: (sessionId: string, event: SubagentEvent) => void;
    onSubagentLog?: (sessionId: string, event: SubagentEvent) => void;
    onTaskStarted?: (sessionId: string) => void;
    onContextCompacted?: (sessionId: string) => void;
    onPlanUpdate?: (sessionId: string, plan: PlanUpdate) => void;
    onMemoryResponse?: (sessionId: string, response: { success: boolean; memoryId?: string; error?: string }) => void;
}

/**
 * Subscribe to session events (events tagged with sessionId)
 * All events now include sessionId at the top level
 */
export function subscribeToSessionEvents(
    callbacks: SessionEventCallbacks
): () => void {
    console.log('[tauri-api] Setting up listener for kaioken-session-event');
    const unlisten = listen<SessionEvent>('kaioken-session-event', (event) => {
        console.log('[tauri-api] SESSION EVENT:', event);
        const payload = event.payload;
        if (!payload) {
            console.warn('[tauri-api] session event payload is null');
            return;
        }

        const sessionId = payload.sessionId;
        if (!sessionId) {
            console.warn('[tauri-api] session event missing sessionId:', payload);
            return;
        }

        const type = payload.type;
        if (!type) {
            console.warn('[tauri-api] session event missing type:', payload);
            return;
        }

        console.log(`%c[tauri-api] SESSION[${sessionId}] ${type}`, 'color: cyan; font-weight: bold', payload);

        switch (type) {
            case 'message':
                callbacks.onMessage?.(sessionId, convertMessage(extractMessagePayload(payload)));
                break;
            case 'toolStart':
                callbacks.onToolStart?.(sessionId, convertToolExecution(payload));
                break;
            case 'toolEnd':
                callbacks.onToolEnd?.(sessionId, convertToolExecution(payload));
                break;
            case 'error':
                if ('message' in payload && typeof payload.message === 'string') {
                    callbacks.onError?.(sessionId, payload.message);
                }
                break;
            case 'contentDelta':
                if ('delta' in payload && typeof payload.delta === 'string') {
                    callbacks.onContentDelta?.(sessionId, payload.delta);
                }
                break;
            case 'reasoningDelta':
                if ('delta' in payload && typeof payload.delta === 'string') {
                    callbacks.onReasoningDelta?.(sessionId, payload.delta);
                }
                break;
            case 'execOutputDelta':
                if ('callId' in payload && 'stream' in payload && 'chunk' in payload) {
                    callbacks.onExecOutputDelta?.(sessionId, {
                        callId: payload.callId as string,
                        stream: payload.stream as 'stdout' | 'stderr',
                        chunk: payload.chunk as string,
                    });
                }
                break;
            case 'taskComplete':
                callbacks.onTaskComplete?.(sessionId);
                break;
            case 'taskStarted':
                callbacks.onTaskStarted?.(sessionId);
                break;
            case 'tokenUsage':
                if ('total' in payload && 'last' in payload) {
                    callbacks.onTokenUsage?.(sessionId, payload as unknown as TokenUsageEvent);
                }
                break;
            case 'approvalRequest':
                callbacks.onApprovalRequest?.(sessionId, payload as unknown as ApprovalRequestEvent);
                break;
            case 'warning':
                if ('message' in payload && typeof payload.message === 'string') {
                    callbacks.onWarning?.(sessionId, payload.message);
                }
                break;
            case 'background':
                if ('message' in payload && typeof payload.message === 'string') {
                    callbacks.onBackground?.(sessionId, payload.message);
                }
                break;
            case 'subagentUpdate':
                callbacks.onSubagentUpdate?.(sessionId, payload as unknown as SubagentEvent);
                break;
            case 'subagentLog':
                callbacks.onSubagentLog?.(sessionId, payload as unknown as SubagentEvent);
                break;
            case 'contextCompacted':
                callbacks.onContextCompacted?.(sessionId);
                break;
            case 'planUpdate':
                const planData = (payload as { plan?: { explanation?: string; plan?: unknown[] } }).plan;
                if (planData && Array.isArray(planData.plan)) {
                    callbacks.onPlanUpdate?.(sessionId, {
                        explanation: planData.explanation,
                        plan: planData.plan as PlanUpdate['plan'],
                    });
                }
                break;
            case 'streamError':
                if ('message' in payload && typeof payload.message === 'string') {
                    callbacks.onError?.(sessionId, payload.message);
                }
                break;
            case 'memoryResponse':
                callbacks.onMemoryResponse?.(sessionId, {
                    success: (payload as { success?: boolean }).success ?? false,
                    memoryId: (payload as { memoryId?: string }).memoryId,
                    error: (payload as { error?: string }).error,
                });
                break;
        }
    });

    return () => {
        unlisten.then((fn) => fn());
    };
}
