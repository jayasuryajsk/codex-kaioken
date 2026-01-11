import { Copy, Check, Loader2, ChevronRight, ChevronDown, CheckCircle2, Circle, PlayCircle, MessageSquare, X } from 'lucide-react';
import { useState, useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import AnsiToHtml from 'ansi-to-html';
import { Message, StreamEvent, ToolExecution, PlanUpdate, PlanWorkflowStatus } from '../types';
import { SubagentCard } from './SubagentCard';

// ANSI to HTML converter instance
const ansiConverter = new AnsiToHtml({
    fg: '#a1a1aa',      // muted foreground
    bg: 'transparent',
    colors: {
        0: '#71717a',   // black -> zinc-500
        1: '#ef4444',   // red
        2: '#22c55e',   // green
        3: '#eab308',   // yellow
        4: '#3b82f6',   // blue
        5: '#a855f7',   // magenta
        6: '#06b6d4',   // cyan
        7: '#e4e4e7',   // white -> zinc-200
        8: '#52525b',   // bright black -> zinc-600
        9: '#f87171',   // bright red
        10: '#4ade80',  // bright green
        11: '#facc15',  // bright yellow
        12: '#60a5fa',  // bright blue
        13: '#c084fc',  // bright magenta
        14: '#22d3ee',  // bright cyan
        15: '#fafafa',  // bright white
    }
});

interface ChatMessageProps {
    message: Message;
    planWorkflowStatus?: PlanWorkflowStatus;
    currentPlan?: PlanUpdate | null;
    planMessageId?: string | null;
    onPlanApprove?: () => void;
    onPlanFeedback?: (feedback: string) => void;
    onPlanCancel?: () => void;
    isLastMessage?: boolean;
}

// Extract **bold header** from reasoning text
function extractReasoningHeader(text: string): string {
    const match = text.match(/^\*\*(.+?)\*\*/);
    if (match) return match[1];
    // Fallback: first line truncated
    const firstLine = text.split('\n')[0];
    return firstLine.length > 80 ? firstLine.slice(0, 77) + '...' : firstLine;
}

// Get tool label
function getToolLabel(tool: ToolExecution): string {
    const type = tool.type;
    const name = tool.name?.toLowerCase() || '';
    const command = tool.input?.command;
    const firstCmd = Array.isArray(command) ? command[0] : command;

    if (type === 'read' || name.includes('read')) return 'Read';
    if (type === 'write' || name.includes('write')) return 'Write';
    if (type === 'edit' || name.includes('edit') || name.includes('patch')) return 'Edit';
    if (type === 'search' || name.includes('grep') || name.includes('search')) return 'Search';
    if (type === 'mcp') return tool.name.replace(/^mcp__[^_]+__/, '');

    if (type === 'shell' && firstCmd) {
        const cmd = firstCmd.toLowerCase();
        if (cmd.includes('grep') || cmd.includes('rg') || cmd.includes('ag')) return 'Search';
        if (cmd === 'ls' || cmd === 'find' || cmd === 'tree') return 'List';
        if (cmd === 'cat' || cmd === 'head' || cmd === 'tail') return 'Read';
        if (cmd === 'git') return 'Git';
        if (cmd.includes('npm') || cmd.includes('cargo') || cmd.includes('make')) return 'Build';
    }

    if (type === 'shell') return 'Run';
    return 'Tool';
}

// Get display text for tool
function getToolDisplay(tool: ToolExecution): string {
    const command = tool.input?.command;
    if (command) {
        const cmdStr = Array.isArray(command) ? command.join(' ') : command;
        return cmdStr.length > 70 ? cmdStr.slice(0, 67) + '...' : cmdStr;
    }
    if (tool.input?.file_path || tool.input?.path) {
        return tool.input.file_path || tool.input.path;
    }
    if (tool.input?.pattern) return `"${tool.input.pattern}"`;
    return tool.name || '';
}

// Truncate output TUI-style (head + ... + tail)
function truncateOutput(text: string, maxLines: number = 5): string[] {
    const lines = text.split('\n');
    if (lines.length <= maxLines) return lines;

    const head = 2;
    const tail = 2;
    const omitted = lines.length - head - tail;

    return [
        ...lines.slice(0, head),
        `… +${omitted} lines`,
        ...lines.slice(-tail)
    ];
}

// Convert ANSI codes to HTML
function formatAnsiOutput(text: string): string {
    try {
        return ansiConverter.toHtml(text);
    } catch {
        return text;
    }
}

// TUI-style tool display with │ └ prefixes and ANSI color support
function TUIToolCard({ tool }: { tool: ToolExecution }) {
    const isRunning = tool.status === 'running';
    const isError = tool.status === 'error';

    const label = getToolLabel(tool);
    const display = getToolDisplay(tool);
    const outputLines = tool.output ? truncateOutput(tool.output) : [];

    // Format duration
    const duration = tool.endTime
        ? `${((tool.endTime.getTime() - tool.startTime.getTime()) / 1000).toFixed(1)}s`
        : '';

    // Memoize ANSI-converted output
    const formattedLines = useMemo(() =>
        outputLines.map(line => ({
            html: formatAnsiOutput(line),
            isOmitted: line.startsWith('… +')
        })),
        [outputLines]
    );

    return (
        <div className="font-mono text-xs my-1">
            {/* Command line */}
            <div className="flex items-start gap-2">
                {isRunning ? (
                    <Loader2 className="w-3 h-3 mt-0.5 text-primary animate-spin flex-shrink-0" />
                ) : (
                    <span className={`flex-shrink-0 ${isError ? 'text-red-500' : 'text-green-600'}`}>•</span>
                )}
                <span className="font-semibold text-foreground">{label}</span>
                <span className="text-muted-foreground break-all flex-1">{display}</span>
                {duration && <span className="text-muted-foreground/50">{duration}</span>}
            </div>

            {/* Output lines with │ prefix and ANSI colors */}
            {formattedLines.length > 0 && (
                <div className="ml-3">
                    {formattedLines.map((line, i) => {
                        const isLast = i === formattedLines.length - 1;
                        return (
                            <div key={i} className="flex">
                                <span className="text-muted-foreground/40 select-none w-4 flex-shrink-0">
                                    {isLast ? '└' : '│'}
                                </span>
                                <span
                                    className={line.isOmitted ? 'italic text-muted-foreground/50' : 'text-muted-foreground'}
                                    dangerouslySetInnerHTML={{ __html: line.html }}
                                />
                            </div>
                        );
                    })}
                </div>
            )}

            {/* Error */}
            {tool.error && (
                <div className="ml-3 text-red-500 flex">
                    <span className="text-red-500/40 select-none w-4">└</span>
                    <span>{tool.error}</span>
                </div>
            )}
        </div>
    );
}

// Collapsible tools section with shimmer header
function ToolsSection({
    events,
    isComplete
}: {
    events: StreamEvent[];
    isComplete: boolean;
}) {
    const [expanded, setExpanded] = useState(false);

    const toolEvents = events.filter(e => e.type === 'tool');
    const reasoningEvents = events.filter(e => e.type === 'reasoning');

    // Get latest reasoning header for shimmer text (only if we have actual reasoning)
    const latestReasoning = reasoningEvents.length > 0
        ? reasoningEvents[reasoningEvents.length - 1]
        : null;
    const headerText = latestReasoning?.type === 'reasoning'
        ? extractReasoningHeader(latestReasoning.text)
        : null;

    const isRunning = toolEvents.some(e => e.type === 'tool' && e.execution.status === 'running');
    const showShimmer = (isRunning || !isComplete) && headerText;

    // If no tools and no reasoning, show nothing (TUI behavior)
    if (toolEvents.length === 0 && !headerText) {
        return null;
    }

    // If no tools but have reasoning, show shimmer only while running
    if (toolEvents.length === 0) {
        if (!showShimmer) return null;
        return (
            <div className="flex items-center gap-2 my-1">
                <Loader2 className="w-3 h-3 text-primary animate-spin" />
                <span className="text-sm shimmer-text">{headerText}</span>
            </div>
        );
    }

    return (
        <div className="my-1">
            {/* Collapsed header with shimmer */}
            <button
                onClick={() => setExpanded(!expanded)}
                className="flex items-center gap-2 text-sm hover:bg-muted/30 rounded px-1 -mx-1 transition-colors w-full text-left"
            >
                {expanded ? (
                    <ChevronDown className="w-3 h-3 text-muted-foreground" />
                ) : (
                    <ChevronRight className="w-3 h-3 text-muted-foreground" />
                )}

                {showShimmer ? (
                    <>
                        <Loader2 className="w-3 h-3 text-primary animate-spin" />
                        <span className="shimmer-text">{headerText}</span>
                    </>
                ) : (
                    <>
                        <span className="text-green-600">•</span>
                        <span className="text-muted-foreground">
                            {toolEvents.length} tool {toolEvents.length === 1 ? 'call' : 'calls'}
                        </span>
                    </>
                )}
            </button>

            {/* Expanded: TUI-style tool list */}
            {expanded && (
                <div className="ml-4 mt-1 border-l border-border/40 pl-3">
                    {events.map((event, i) => {
                        if (event.type === 'reasoning') {
                            return (
                                <div key={i} className="text-sm text-muted-foreground italic my-1">
                                    <ReactMarkdown remarkPlugins={[remarkGfm]}>
                                        {event.text.split('\n')[0].slice(0, 100)}
                                    </ReactMarkdown>
                                </div>
                            );
                        }
                        if (event.type === 'tool') {
                            return <TUIToolCard key={i} tool={event.execution} />;
                        }
                        return null;
                    })}
                </div>
            )}
        </div>
    );
}

// Markdown renderer component
function MarkdownContent({ content }: { content: string }) {
    return (
        <div className="prose prose-sm dark:prose-invert max-w-none leading-relaxed text-foreground">
            <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                components={{
                    code({ className, children, ...props }) {
                        const isInline = !className;
                        return isInline ? (
                            <code className="font-mono text-[13px]" style={{ color: 'hsl(var(--code-keyword))' }} {...props}>
                                {children}
                            </code>
                        ) : (
                            <code className={`${className} block bg-muted p-3 rounded-md text-sm font-mono overflow-x-auto`} {...props}>
                                {children}
                            </code>
                        );
                    },
                    pre({ children }) {
                        return <pre className="bg-muted rounded-md overflow-x-auto my-2">{children}</pre>;
                    },
                    a({ href, children }) {
                        return (
                            <a href={href} className="text-primary hover:underline" target="_blank" rel="noopener noreferrer">
                                {children}
                            </a>
                        );
                    },
                    ul({ children }) {
                        return <ul className="list-disc list-inside my-2 space-y-1">{children}</ul>;
                    },
                    ol({ children }) {
                        return <ol className="list-decimal list-inside my-2 space-y-1">{children}</ol>;
                    },
                    p({ children }) {
                        return <p className="my-1">{children}</p>;
                    },
                    blockquote({ children }) {
                        return <blockquote className="border-l-2 border-muted-foreground/30 pl-3 my-2 italic">{children}</blockquote>;
                    },
                    h1({ children }) {
                        return <h1 className="text-xl font-semibold mt-4 mb-2 text-foreground">{children}</h1>;
                    },
                    h2({ children }) {
                        return <h2 className="text-lg font-semibold mt-4 mb-2 text-foreground">{children}</h2>;
                    },
                    h3({ children }) {
                        return <h3 className="text-base font-semibold mt-3 mb-1.5 text-foreground">{children}</h3>;
                    },
                    h4({ children }) {
                        return <h4 className="text-[15px] font-semibold mt-3 mb-1 text-foreground">{children}</h4>;
                    },
                }}
            >
                {content}
            </ReactMarkdown>
        </div>
    );
}

// Plan card with action buttons
function PlanCard({
    plan,
    showActions,
    onApprove,
    onFeedback,
    onCancel,
}: {
    plan: PlanUpdate;
    showActions: boolean;
    onApprove?: () => void;
    onFeedback?: (feedback: string) => void;
    onCancel?: () => void;
}) {
    const [feedbackMode, setFeedbackMode] = useState(false);
    const [feedbackText, setFeedbackText] = useState('');

    const handleSubmitFeedback = () => {
        if (feedbackText.trim() && onFeedback) {
            onFeedback(feedbackText.trim());
            setFeedbackText('');
            setFeedbackMode(false);
        }
    };

    return (
        <div className="my-3 border border-border rounded-lg overflow-hidden bg-card">
            {/* Header */}
            <div className="px-4 py-2 bg-muted/30 border-b border-border flex items-center gap-2">
                <span className="text-sm font-medium text-foreground">📋 Plan</span>
                {plan.explanation && (
                    <span className="text-xs text-muted-foreground">— {plan.explanation}</span>
                )}
            </div>

            {/* Steps */}
            <div className="px-4 py-3 space-y-2">
                {plan.plan.map((step, index) => {
                    const StatusIcon = step.status === 'completed'
                        ? CheckCircle2
                        : step.status === 'in_progress'
                            ? PlayCircle
                            : Circle;
                    const statusColor = step.status === 'completed'
                        ? 'text-green-500'
                        : step.status === 'in_progress'
                            ? 'text-primary'
                            : 'text-muted-foreground/50';
                    const textStyle = step.status === 'completed'
                        ? 'line-through text-muted-foreground/70'
                        : 'text-foreground';

                    return (
                        <div key={index} className="flex items-start gap-2">
                            <StatusIcon className={`w-4 h-4 mt-0.5 flex-shrink-0 ${statusColor}`} />
                            <div className={`text-sm ${textStyle} prose prose-sm dark:prose-invert max-w-none [&_p]:my-0 [&_code]:text-xs [&_code]:bg-muted [&_code]:px-1 [&_code]:py-0.5 [&_code]:rounded`} style={{ '--tw-prose-code': 'hsl(var(--code-keyword))' } as React.CSSProperties}>
                                <ReactMarkdown remarkPlugins={[remarkGfm]}>
                                    {step.step}
                                </ReactMarkdown>
                            </div>
                        </div>
                    );
                })}
            </div>

            {/* Action buttons */}
            {showActions && (
                <div className="px-4 py-3 border-t border-border bg-muted/20">
                    {feedbackMode ? (
                        <div className="space-y-2">
                            <textarea
                                value={feedbackText}
                                onChange={(e) => setFeedbackText(e.target.value)}
                                placeholder="Enter your feedback..."
                                className="w-full px-3 py-2 text-sm bg-background border border-border rounded focus:outline-none focus:ring-1 focus:ring-primary resize-none"
                                rows={2}
                                autoFocus
                            />
                            <div className="flex gap-2">
                                <button
                                    onClick={handleSubmitFeedback}
                                    disabled={!feedbackText.trim()}
                                    className="px-3 py-1.5 text-xs font-medium bg-primary text-primary-foreground rounded hover:opacity-90 disabled:opacity-50"
                                >
                                    Send Feedback
                                </button>
                                <button
                                    onClick={() => { setFeedbackMode(false); setFeedbackText(''); }}
                                    className="px-3 py-1.5 text-xs font-medium text-muted-foreground hover:text-foreground"
                                >
                                    Cancel
                                </button>
                            </div>
                        </div>
                    ) : (
                        <div className="flex gap-2">
                            <button
                                onClick={onApprove}
                                className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium bg-green-600 text-white rounded hover:bg-green-700"
                            >
                                <Check className="w-3.5 h-3.5" />
                                Approve & Execute
                            </button>
                            <button
                                onClick={() => setFeedbackMode(true)}
                                className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium bg-muted text-foreground rounded hover:bg-muted/80"
                            >
                                <MessageSquare className="w-3.5 h-3.5" />
                                Give Feedback
                            </button>
                            <button
                                onClick={onCancel}
                                className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-muted-foreground hover:text-foreground"
                            >
                                <X className="w-3.5 h-3.5" />
                                Cancel
                            </button>
                        </div>
                    )}
                </div>
            )}
        </div>
    );
}

export function ChatMessage({
    message,
    planWorkflowStatus,
    currentPlan,
    planMessageId,
    onPlanApprove,
    onPlanFeedback,
    onPlanCancel,
    isLastMessage: _isLastMessage,
}: ChatMessageProps) {
    const [copied, setCopied] = useState(false);
    const isUser = message.role === 'user';

    const handleCopy = async () => {
        await navigator.clipboard.writeText(message.content);
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
    };
    const CopyIcon = copied ? Check : Copy;

    // User messages
    if (isUser) {
        // Special handling for plan approval - show as subtle separator instead of bubble
        if (message.content === 'Plan approved ✓') {
            return (
                <div className="flex items-center gap-2 text-xs text-muted-foreground/50 py-2 font-mono">
                    <div className="flex-1 border-t border-border/30" />
                    <span>Plan approved</span>
                    <div className="flex-1 border-t border-border/30" />
                </div>
            );
        }

        return (
            <div className="group relative py-2 animate-in">
                <div className="inline-block bg-card px-4 py-2 rounded border border-border text-foreground">
                    <p className="whitespace-pre-wrap text-sm">{message.content}</p>
                </div>
            </div>
        );
    }

    // Assistant messages
    const events = message.events || [];
    const isComplete = !!message.completedAt;

    // Separate tool/reasoning events from content (plan is handled globally)
    const workEvents = events.filter(e => e.type === 'tool' || e.type === 'reasoning');
    const toolEvents = events.filter(e => e.type === 'tool');
    const subagentEvents = events.filter((e): e is Extract<StreamEvent, { type: 'subagent' }> => e.type === 'subagent');
    const contentEvents = events.filter((e): e is Extract<StreamEvent, { type: 'content' }> => e.type === 'content');
    const contentText = contentEvents.map(e => e.text).join('');
    const hasToolCalls = toolEvents.length > 0 || (message.toolCalls?.length ?? 0) > 0 || subagentEvents.length > 0;

    // Show plan card only on the original plan message, using global currentPlan
    const shouldShowPlan = message.id === planMessageId && currentPlan;
    const showPlanActions = shouldShowPlan && planWorkflowStatus === 'awaiting_approval';

    // Format elapsed time (TUI style: 2m 57s)
    const elapsed = message.completedAt
        ? (() => {
            const ms = message.completedAt.getTime() - message.timestamp.getTime();
            const totalSecs = Math.floor(ms / 1000);
            if (ms < 1000) return `${ms}ms`;
            if (totalSecs < 60) return `${(ms / 1000).toFixed(1)}s`;
            const mins = Math.floor(totalSecs / 60);
            const secs = totalSecs % 60;
            return `${mins}m ${secs}s`;
        })()
        : null;

    return (
        <div className="group relative py-2 animate-in">
            <div className="text-sm">
                {/* Tools section - collapsible with shimmer */}
                {workEvents.length > 0 && (
                    <ToolsSection events={workEvents} isComplete={isComplete} />
                )}

                {/* Subagent cards - inline like TUI */}
                {subagentEvents.map((event) => (
                    <SubagentCard key={`${event.task.callId}-${event.task.agentIndex}`} task={event.task} />
                ))}

                {/* Plan card - rendered only on the original plan message */}
                {shouldShowPlan && currentPlan && (
                    <PlanCard
                        plan={currentPlan}
                        showActions={!!showPlanActions}
                        onApprove={onPlanApprove}
                        onFeedback={onPlanFeedback}
                        onCancel={onPlanCancel}
                    />
                )}

                {/* Content */}
                {contentText && <MarkdownContent content={contentText} />}

                {/* Legacy fallback */}
                {events.length === 0 && (
                    <>
                        {message.reasoning && (
                            <div className="text-sm text-muted-foreground italic my-1">
                                <ReactMarkdown remarkPlugins={[remarkGfm]}>
                                    {message.reasoning.split('\n')[0].slice(0, 100)}
                                </ReactMarkdown>
                            </div>
                        )}
                        {message.toolCalls?.map((tool) => (
                            <TUIToolCard key={tool.id} tool={tool} />
                        ))}
                        {message.content && <MarkdownContent content={message.content} />}
                    </>
                )}

                {/* Worked for X separator - only show if there were tool calls */}
                {elapsed && hasToolCalls && (
                    <div className="flex items-center gap-2 text-xs text-muted-foreground/40 mt-3 font-mono">
                        <div className="flex-1 border-t border-border/20" />
                        <span>Worked for {elapsed}</span>
                        <div className="flex-1 border-t border-border/20" />
                    </div>
                )}
            </div>

            {/* Copy button */}
            <div className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity">
                <button
                    onClick={handleCopy}
                    className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground"
                    title="Copy"
                >
                    <CopyIcon className="w-3 h-3" />
                </button>
            </div>
        </div>
    );
}
