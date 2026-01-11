// Plan step status
export type PlanStepStatus = 'pending' | 'in_progress' | 'completed';

// Plan step
export interface PlanStep {
  step: string;
  status: PlanStepStatus;
}

// Plan update from backend
export interface PlanUpdate {
  explanation?: string;
  plan: PlanStep[];
}

// Plan workflow status
export type PlanWorkflowStatus = 'awaiting_plan' | 'awaiting_approval' | 'executing' | 'idle';

// Stream event types for interleaved rendering (TUI-style)
export type StreamEvent =
  | { type: 'reasoning'; text: string }
  | { type: 'tool'; execution: ToolExecution }
  | { type: 'content'; text: string }
  | { type: 'plan'; update: PlanUpdate }
  | { type: 'subagent'; task: SubagentTask };

export interface Message {
  id: string;
  role: 'user' | 'assistant';
  content: string;  // For user messages, or legacy support
  timestamp: Date;
  events?: StreamEvent[];  // Interleaved stream events for assistant messages
  // Task completion tracking (for "Worked for X" separator)
  completedAt?: Date;
  inputTokens?: number;
  outputTokens?: number;
  // Legacy fields (kept for backward compat, can be derived from events)
  toolCalls?: ToolExecution[];
  reasoning?: string;
}

export type ToolType = 'shell' | 'read' | 'write' | 'edit' | 'search' | 'mcp' | 'memory';

export type ToolStatus = 'pending' | 'running' | 'success' | 'error';

export interface ToolExecution {
  id: string;
  type: ToolType;
  name: string;
  status: ToolStatus;
  input: any;
  output?: string;
  error?: string;
  startTime: Date;
  endTime?: Date;
}

export interface ShellInput {
  command: string[];
  cwd?: string;
  env?: Record<string, string>;
}

export interface DiffLine {
  content: string;
  type: 'add' | 'remove' | 'context';
  oldLineNo?: number;
  newLineNo?: number;
}

export interface DiffHunk {
  header: string;
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: DiffLine[];
}

export interface ConversationState {
  messages: Message[];
  status: 'idle' | 'busy' | 'waiting-for-approval';
}

export interface ApprovalRequest {
  id: string;
  type: 'tool' | 'edit';
  description: string;
  command?: string;
  risk: 'low' | 'medium' | 'high';
}

export type ReasoningEffort = 'low' | 'medium' | 'high' | 'xhigh';

// Approval mode presets (matches TUI)
export type ApprovalMode = 'read-only' | 'auto' | 'full-access';

// Plan detail preference
export type PlanDetail = 'auto' | 'coarse' | 'detailed';

// Send key preference
export type SendKey = 'enter' | 'cmd-enter';

// App settings - defaults that persist
export interface AppSettings {
  // Chat settings
  model: string;
  reasoningEffort: ReasoningEffort;
  planMode: boolean;
  planDetail: PlanDetail;
  sendKey: SendKey;

  // Approval settings
  approvalMode: ApprovalMode;

  // Performance settings
  subagentConcurrency: number;

  // Display settings
  theme: ThemeName;
  showRateLimits: boolean;
  showTokenCost: boolean;
  animations: boolean;

  // Notification settings
  desktopNotifications: boolean;
  soundEffects: boolean;

  // Experimental features
  experimentalFeatures: Record<string, boolean>;
}

// Available themes
export type ThemeName =
  | 'snow' | 'cream' | 'sage' | 'quiet-light' | 'solarized-light' | 'github-light'
  | 'midnight' | 'obsidian' | 'one-dark' | 'nord' | 'dracula';

export const THEME_INFO: Record<ThemeName, { name: string; isDark: boolean; color: string }> = {
  // Light themes
  snow: { name: 'Snow', isDark: false, color: '#ffffff' },
  cream: { name: 'Cream', isDark: false, color: '#faf8f5' },
  sage: { name: 'Sage', isDark: false, color: '#f0f4f0' },
  'quiet-light': { name: 'Quiet Light', isDark: false, color: '#f9f8f5' },
  'solarized-light': { name: 'Solarized', isDark: false, color: '#fdf6e3' },
  'github-light': { name: 'GitHub', isDark: false, color: '#f6f8fa' },
  // Dark themes
  midnight: { name: 'Midnight', isDark: true, color: '#0f172a' },
  obsidian: { name: 'Obsidian', isDark: true, color: '#09090b' },
  'one-dark': { name: 'One Dark', isDark: true, color: '#282c34' },
  nord: { name: 'Nord', isDark: true, color: '#2e3440' },
  dracula: { name: 'Dracula', isDark: true, color: '#282a36' },
};

export interface TokenUsage {
  input: number;
  output: number;
  total: number;
  cached?: number;
  reasoning?: number;
}

// Approval request from backend
export interface ApprovalRequestEvent {
  kind: 'exec' | 'patch';
  id: string;
  command?: string[];
  cwd?: string;
  files?: string[];
  reasoning?: string;
}

// System event for feed
export interface SystemEvent {
  id: string;
  type: 'warning' | 'error' | 'info' | 'background';
  message: string;
  timestamp: Date;
}

// Subagent task
export interface SubagentTask {
  callId: string;
  agentIndex: number;
  task: string;
  status: 'Running' | 'Done' | 'Timeout' | 'Failed';
  summary?: string;
  logs: string[];
  startedAt: Date;
  finishedAt?: Date;
}

// Legacy AgentSession (kept for backward compat during migration)
export interface AgentSession {
  id: string;
  name: string;
  status: 'running' | 'idle' | 'error';
  branch: string;
  stats: {
    added: number;
    removed: number;
  };
  messages: Message[];
  tokenUsage: TokenUsage;
}

// ============================================================================
// Workspace Management Types (Multi-repo, Multi-worktree)
// ============================================================================

// Status of a worktree session
export type WorkspaceStatus =
  | 'idle'           // Ready for input
  | 'thinking'       // LLM is processing
  | 'working'        // Executing tools
  | 'waiting'        // Awaiting approval
  | 'error'          // Error state
  | 'disconnected';  // Backend process not running

// Git worktree information
export interface WorktreeInfo {
  name: string;        // Display name (usually branch name)
  path: string;        // Absolute path to worktree
  branch: string;      // Current git branch
  isMain: boolean;     // Whether this is the main worktree (repo root)
}

// Repository configuration
export interface Repository {
  id: string;
  name: string;
  rootPath: string;
  worktrees: WorktreeInfo[];
  expanded: boolean;   // Whether expanded in sidebar
}

// Chat tab within a worktree session
export interface ChatTab {
  id: string;
  name: string;
  conversationId?: string;
  rolloutPath?: string;
  messages: Message[];
  tokenUsage: TokenUsage;
  createdAt: string;
}

// Worktree session (one per backend process)
export interface WorktreeSession {
  id: string;
  repositoryId: string;
  worktreePath: string;
  worktreeName: string;
  status: WorkspaceStatus;
  lastActivity: string;       // ISO timestamp
  currentTask?: string;       // Brief description of current activity
  /** @deprecated Use tabs instead */
  messages: Message[];
  tokenUsage: TokenUsage;
  stats: { added: number; removed: number };
  /** Path to rollout file for session persistence/resume */
  rolloutPath?: string;
  /** Backend conversation ID for reference */
  conversationId?: string;
  /** Chat tabs - multiple conversations within same worktree */
  tabs?: ChatTab[];
  /** Active tab ID */
  activeTabId?: string;
}

// Full workspace state
export interface WorkspaceState {
  repositories: Repository[];
  sessions: Map<string, WorktreeSession>;
  activeSessionId: string | null;
}

// Persisted tab config
export interface TabConfig {
  id: string;
  name: string;
  rolloutPath?: string;
  conversationId?: string;
  createdAt: string;
}

// Persisted workspace config (from ~/.codex/gui-workspaces.json)
export interface GuiWorkspacesConfig {
  version: number;
  repositories: Repository[];
  sessions: Array<{
    id: string;
    repositoryId: string;
    worktreePath: string;
    worktreeName: string;
    createdAt: string;
    lastActivity: string;
    /** Path to rollout file for resuming session (deprecated) */
    rolloutPath?: string;
    /** Backend conversation ID (deprecated) */
    conversationId?: string;
    /** Open tabs */
    tabs?: TabConfig[];
    /** Active tab ID */
    activeTabId?: string;
    /** Chat history (closed tabs) */
    history?: TabConfig[];
  }>;
  activeSessionId: string | null;
}

// Git info detected from a path
export interface DetectedGitInfo {
  isGitRepo: boolean;
  repoRoot: string | null;
  branch: string | null;
  isWorktree: boolean;
  mainRepoPath: string | null;
}
