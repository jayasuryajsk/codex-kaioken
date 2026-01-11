import { AlertTriangle, Check, X, Terminal, FilePen } from 'lucide-react';

interface ApprovalRequest {
    kind: 'exec' | 'patch';
    id: string;
    command?: string[];
    cwd?: string;
    files?: string[];
    reasoning?: string;
}

interface ApprovalModalProps {
    request: ApprovalRequest;
    onApprove: (id: string) => void;
    onDeny: (id: string) => void;
}

export function ApprovalModal({ request, onApprove, onDeny }: ApprovalModalProps) {
    const isExec = request.kind === 'exec';

    return (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
            <div className="bg-background border border-border rounded-lg shadow-xl max-w-lg w-full mx-4">
                {/* Header */}
                <div className="flex items-center gap-3 px-4 py-3 border-b border-border">
                    <div className="p-2 rounded-full bg-yellow-500/10">
                        <AlertTriangle className="w-5 h-5 text-yellow-500" />
                    </div>
                    <div>
                        <h3 className="text-sm font-semibold text-foreground">
                            {isExec ? 'Command Approval Required' : 'File Changes Approval Required'}
                        </h3>
                    </div>
                </div>

                {/* Content */}
                <div className="p-4 space-y-3">
                    {isExec && request.command && (
                        <div className="space-y-2">
                            <div className="flex items-center gap-2 text-xs text-muted-foreground">
                                <Terminal className="w-3.5 h-3.5" />
                                <span>Command</span>
                            </div>
                            <pre className="p-3 bg-muted rounded text-xs font-mono overflow-x-auto">
                                {request.command.join(' ')}
                            </pre>
                            {request.cwd && (
                                <div className="text-xs text-muted-foreground">
                                    Working directory: <code className="bg-muted px-1 rounded">{request.cwd}</code>
                                </div>
                            )}
                        </div>
                    )}

                    {!isExec && request.files && request.files.length > 0 && (
                        <div className="space-y-2">
                            <div className="flex items-center gap-2 text-xs text-muted-foreground">
                                <FilePen className="w-3.5 h-3.5" />
                                <span>Files to modify</span>
                            </div>
                            <div className="p-3 bg-muted rounded space-y-1">
                                {request.files.map((file, i) => (
                                    <div key={i} className="text-xs font-mono text-foreground">
                                        {file}
                                    </div>
                                ))}
                            </div>
                        </div>
                    )}

                    {request.reasoning && (
                        <div className="text-xs text-muted-foreground">
                            <span className="font-medium">Reason:</span> {request.reasoning}
                        </div>
                    )}
                </div>

                {/* Actions */}
                <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-border bg-muted/30">
                    <button
                        onClick={() => onDeny(request.id)}
                        className="flex items-center gap-2 px-3 py-1.5 text-xs rounded bg-muted hover:bg-muted/80 text-foreground transition-colors"
                    >
                        <X className="w-3.5 h-3.5" />
                        Deny
                    </button>
                    <button
                        onClick={() => onApprove(request.id)}
                        className="flex items-center gap-2 px-3 py-1.5 text-xs rounded bg-primary hover:bg-primary/90 text-primary-foreground transition-colors"
                    >
                        <Check className="w-3.5 h-3.5" />
                        Approve
                    </button>
                </div>
            </div>
        </div>
    );
}
