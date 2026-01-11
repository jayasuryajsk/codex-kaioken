import { useState, useEffect } from 'react';
import { ChevronDown, ChevronRight, Check, X, Clock, Loader2 } from 'lucide-react';
import { SubagentTask } from '../types';

interface SubagentCardProps {
    task: SubagentTask;
}

function formatElapsed(startedAt: Date, finishedAt?: Date): string {
    const end = finishedAt || new Date();
    const ms = end.getTime() - startedAt.getTime();
    const seconds = Math.floor(ms / 1000);
    if (seconds < 60) return `${seconds}s`;
    const minutes = Math.floor(seconds / 60);
    const remainingSec = seconds % 60;
    return `${minutes}m ${remainingSec}s`;
}

export function SubagentCard({ task }: SubagentCardProps) {
    const [expanded, setExpanded] = useState(true);
    const [elapsed, setElapsed] = useState(() => formatElapsed(task.startedAt, task.finishedAt));

    // Update elapsed time every second while running
    useEffect(() => {
        if (task.status !== 'Running') {
            setElapsed(formatElapsed(task.startedAt, task.finishedAt));
            return;
        }

        const interval = setInterval(() => {
            setElapsed(formatElapsed(task.startedAt));
        }, 1000);

        return () => clearInterval(interval);
    }, [task.status, task.startedAt, task.finishedAt]);

    const isRunning = task.status === 'Running';
    const isDone = task.status === 'Done';
    const isFailed = task.status === 'Failed';
    const isTimeout = task.status === 'Timeout';

    // Status icon and color
    const getStatusIcon = () => {
        if (isRunning) return <Loader2 className="w-3.5 h-3.5 animate-spin text-primary" />;
        if (isDone) return <Check className="w-3.5 h-3.5 text-green-500" />;
        if (isFailed) return <X className="w-3.5 h-3.5 text-red-500" />;
        if (isTimeout) return <Clock className="w-3.5 h-3.5 text-amber-500" />;
        return null;
    };

    const getBorderColor = () => {
        if (isRunning) return 'border-primary/30';
        if (isDone) return 'border-green-500/30';
        if (isFailed) return 'border-red-500/30';
        if (isTimeout) return 'border-amber-500/30';
        return 'border-border';
    };

    // Show last 5 log lines
    const visibleLogs = task.logs.slice(-5);
    const hiddenCount = Math.max(0, task.logs.length - 5);

    return (
        <div className={`my-2 border rounded-lg ${getBorderColor()} bg-muted/20 overflow-hidden`}>
            {/* Header */}
            <button
                onClick={() => setExpanded(!expanded)}
                className="w-full flex items-center gap-2 px-3 py-2 hover:bg-muted/30 transition-colors"
            >
                {expanded ? (
                    <ChevronDown className="w-3.5 h-3.5 text-muted-foreground" />
                ) : (
                    <ChevronRight className="w-3.5 h-3.5 text-muted-foreground" />
                )}

                {getStatusIcon()}

                <span className="flex-1 text-left text-sm font-medium text-foreground truncate">
                    {task.task}
                </span>

                <span className="text-xs text-muted-foreground tabular-nums">
                    {elapsed}
                </span>
            </button>

            {/* Content */}
            {expanded && (
                <div className="px-3 pb-2 space-y-1">
                    {/* Log lines */}
                    {task.logs.length > 0 && (
                        <div className="font-mono text-[11px] leading-relaxed space-y-0.5 max-h-32 overflow-y-auto">
                            {hiddenCount > 0 && (
                                <div className="text-muted-foreground/50 italic">
                                    ... {hiddenCount} more
                                </div>
                            )}
                            {visibleLogs.map((line, i) => (
                                <div key={i} className="text-muted-foreground whitespace-pre-wrap break-words">
                                    {line}
                                </div>
                            ))}
                        </div>
                    )}

                    {/* Summary - formatted with line breaks */}
                    {task.summary && !isRunning && (
                        <div className={`text-xs leading-relaxed mt-1 pt-1 border-t border-border/50 whitespace-pre-wrap break-words ${
                            isDone ? 'text-foreground' :
                            isFailed ? 'text-red-600 dark:text-red-400' :
                            isTimeout ? 'text-amber-600 dark:text-amber-400' :
                            'text-muted-foreground'
                        }`}>
                            {task.summary.length > 200
                                ? task.summary.slice(0, 200) + '...'
                                : task.summary}
                        </div>
                    )}

                    {/* Empty state */}
                    {task.logs.length === 0 && isRunning && !task.summary && (
                        <div className="text-xs text-muted-foreground/50 italic">
                            Starting...
                        </div>
                    )}
                </div>
            )}
        </div>
    );
}
