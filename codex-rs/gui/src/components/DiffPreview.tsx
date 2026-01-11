import { DiffHunk, DiffLine } from '../types';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { useState } from 'react';

interface DiffPreviewProps {
    filename: string;
    hunks: DiffHunk[];
}

function DiffLineView({ line }: { line: DiffLine }) {
    const bgColor = {
        add: 'bg-green-500/10',
        remove: 'bg-red-500/10',
        context: 'bg-transparent',
    }[line.type];

    const textColor = {
        add: 'text-green-500',
        remove: 'text-red-500',
        context: 'text-foreground',
    }[line.type];

    const prefix = {
        add: '+',
        remove: '-',
        context: ' ',
    }[line.type];

    return (
        <div className={`flex font-mono text-xs ${bgColor}`}>
            {/* Line numbers */}
            <span className="w-10 px-2 text-right text-muted-foreground select-none border-r border-border">
                {line.oldLineNo || ''}
            </span>
            <span className="w-10 px-2 text-right text-muted-foreground select-none border-r border-border">
                {line.newLineNo || ''}
            </span>
            {/* Content */}
            <span className={`flex-1 px-2 ${textColor}`}>
                <span className="select-none">{prefix}</span>
                {line.content}
            </span>
        </div>
    );
}

export function DiffPreview({ filename, hunks }: DiffPreviewProps) {
    const [collapsed, setCollapsed] = useState(false);

    const additions = hunks.reduce(
        (sum, h) => sum + h.lines.filter((l) => l.type === 'add').length,
        0
    );
    const deletions = hunks.reduce(
        (sum, h) => sum + h.lines.filter((l) => l.type === 'remove').length,
        0
    );

    return (
        <div className="border border-border rounded-lg overflow-hidden">
            {/* Header */}
            <button
                onClick={() => setCollapsed(!collapsed)}
                className="w-full flex items-center gap-2 px-3 py-2 bg-muted/50 hover:bg-muted transition-colors"
            >
                {collapsed ? (
                    <ChevronRight className="w-4 h-4 text-muted-foreground" />
                ) : (
                    <ChevronDown className="w-4 h-4 text-muted-foreground" />
                )}
                <span className="text-sm font-mono text-foreground flex-1 text-left truncate">
                    {filename}
                </span>
                <span className="text-xs">
                    <span className="text-green-500">+{additions}</span>
                    <span className="text-muted-foreground mx-1">/</span>
                    <span className="text-red-500">-{deletions}</span>
                </span>
            </button>

            {/* Diff content */}
            {!collapsed && (
                <div className="bg-background overflow-x-auto">
                    {hunks.map((hunk, i) => (
                        <div key={i}>
                            {/* Hunk header */}
                            <div className="px-3 py-1 bg-muted/30 text-xs text-muted-foreground font-mono">
                                @@ -{hunk.oldStart},{hunk.oldLines} +{hunk.newStart},
                                {hunk.newLines} @@
                            </div>
                            {/* Lines */}
                            {hunk.lines.map((line, j) => (
                                <DiffLineView key={j} line={line} />
                            ))}
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
}
