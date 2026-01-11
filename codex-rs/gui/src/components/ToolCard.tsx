import {
    Terminal,
    FileText,
    FilePen,
    Search,
    ChevronDown,
    ChevronRight,
    CheckCircle2,
    XCircle,
    Loader2,
} from 'lucide-react';
import { useState, useEffect, useRef } from 'react';
import { ToolExecution, ToolType, ShellInput } from '../types';

interface ToolCardProps {
    tool: ToolExecution;
}

const toolIcons: Record<ToolType, React.ElementType> = {
    shell: Terminal,
    read: FileText,
    write: FilePen,
    edit: FilePen,
    search: Search,
    mcp: Terminal,
    memory: FileText,
};

function StatusIndicator({ status }: { status: ToolExecution['status'] }) {
    switch (status) {
        case 'pending':
            return <span className="w-1.5 h-1.5 rounded-full bg-muted-foreground" />;
        case 'running':
            return <Loader2 className="w-3 h-3 text-primary animate-spin" />;
        case 'success':
            return <CheckCircle2 className="w-3 h-3 text-primary" />;
        case 'error':
            return <XCircle className="w-3 h-3 text-destructive" />;
    }
}

function formatDuration(start: Date, end?: Date): string {
    const endTime = end || new Date();
    const ms = endTime.getTime() - start.getTime();
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(1)}s`;
}

export function ToolCard({ tool }: ToolCardProps) {
    // Auto-expand when running or when there's output
    const [expanded, setExpanded] = useState(tool.status === 'running' || !!tool.output);
    const outputRef = useRef<HTMLPreElement>(null);
    const Icon = toolIcons[tool.type] || Terminal;

    // Auto-expand and scroll when output changes
    useEffect(() => {
        if (tool.output && tool.status === 'running') {
            setExpanded(true);
            // Auto-scroll to bottom of output
            if (outputRef.current) {
                outputRef.current.scrollTop = outputRef.current.scrollHeight;
            }
        }
    }, [tool.output, tool.status]);

    const getDisplayName = () => {
        if (tool.type === 'shell' && tool.input && 'command' in tool.input) {
            const shellInput = tool.input as ShellInput;
            return `$ ${shellInput.command.join(' ')}`;
        }
        return tool.name;
    };

    return (
        <div className="border border-border rounded bg-muted/20">
            {/* Header */}
            <button
                onClick={() => setExpanded(!expanded)}
                className="w-full flex items-center gap-2 px-2 py-1.5 hover:bg-muted/30 transition-colors text-left"
            >
                {expanded ? (
                    <ChevronDown className="w-3 h-3 text-muted-foreground" />
                ) : (
                    <ChevronRight className="w-3 h-3 text-muted-foreground" />
                )}
                <Icon className="w-3 h-3 text-muted-foreground" />
                <code className="flex-1 text-xs truncate text-foreground">
                    {getDisplayName()}
                </code>
                <StatusIndicator status={tool.status} />
                <span className="text-xs text-muted-foreground tabular-nums">
                    {formatDuration(tool.startTime, tool.endTime)}
                </span>
            </button>

            {/* Expanded content */}
            {expanded && (
                <div className="border-t border-border">
                    {/* Output */}
                    {tool.output && (
                        <pre ref={outputRef} className="p-2 text-xs overflow-x-auto bg-background/50 max-h-40 overflow-y-auto text-muted-foreground">
                            <code>{tool.output}</code>
                        </pre>
                    )}

                    {/* Error */}
                    {tool.error && (
                        <div className="p-2 text-xs text-destructive bg-destructive/5">
                            {tool.error}
                        </div>
                    )}

                    {/* Running indicator */}
                    {tool.status === 'running' && !tool.output && (
                        <div className="p-2 flex items-center gap-2 text-xs text-muted-foreground">
                            <Loader2 className="w-3 h-3 animate-spin" />
                            running...
                        </div>
                    )}
                </div>
            )}
        </div>
    );
}
