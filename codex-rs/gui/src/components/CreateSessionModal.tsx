import { useState, useEffect } from 'react';
import { X, Sparkles, ChevronDown, ChevronUp } from 'lucide-react';
import { Repository } from '../types';

// Slugify task name to valid git branch name
function slugify(text: string): string {
    return text
        .toLowerCase()
        .trim()
        .replace(/[^a-z0-9\s-]/g, '')  // Remove special chars
        .replace(/\s+/g, '-')           // Spaces to hyphens
        .replace(/-+/g, '-')            // Collapse multiple hyphens
        .replace(/^-|-$/g, '')          // Trim hyphens from ends
        .slice(0, 50);                  // Limit length
}

interface CreateSessionModalProps {
    repository: Repository;
    onClose: () => void;
    onCreate: (sessionName: string, branchName: string, worktreePath: string) => Promise<void>;
}

export function CreateSessionModal({
    repository,
    onClose,
    onCreate,
}: CreateSessionModalProps) {
    const [taskName, setTaskName] = useState('');
    const [branchName, setBranchName] = useState('');
    const [worktreePath, setWorktreePath] = useState('');
    const [showAdvanced, setShowAdvanced] = useState(false);
    const [isCreating, setIsCreating] = useState(false);
    const [error, setError] = useState<string | null>(null);

    // Auto-generate branch name and worktree path from task name
    useEffect(() => {
        if (taskName) {
            const slug = slugify(taskName);
            const generatedBranch = `session/${slug}`;
            setBranchName(generatedBranch);

            // Worktree path: sibling directory
            const repoDir = repository.rootPath;
            const parentDir = repoDir.substring(0, repoDir.lastIndexOf('/'));
            const repoName = repoDir.substring(repoDir.lastIndexOf('/') + 1);
            setWorktreePath(`${parentDir}/${repoName}-${slug}`);
        } else {
            setBranchName('');
            setWorktreePath('');
        }
    }, [taskName, repository.rootPath]);

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        if (!taskName.trim() || !branchName.trim() || !worktreePath.trim()) return;

        setIsCreating(true);
        setError(null);

        try {
            await onCreate(taskName.trim(), branchName.trim(), worktreePath.trim());
            onClose();
        } catch (err) {
            setError(err instanceof Error ? err.message : String(err));
        } finally {
            setIsCreating(false);
        }
    };

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
            {/* Backdrop */}
            <div
                className="absolute inset-0 bg-black/50 backdrop-blur-sm"
                onClick={onClose}
            />

            {/* Modal */}
            <div className="relative bg-background border border-border rounded-lg shadow-xl w-full max-w-md mx-4">
                {/* Header */}
                <div className="flex items-center justify-between px-4 py-3 border-b border-border">
                    <div className="flex items-center gap-2">
                        <Sparkles className="w-4 h-4 text-accent" />
                        <h2 className="text-sm font-medium">New Session</h2>
                    </div>
                    <button
                        onClick={onClose}
                        className="p-1 text-muted-foreground hover:text-foreground rounded"
                    >
                        <X className="w-4 h-4" />
                    </button>
                </div>

                {/* Content */}
                <form onSubmit={handleSubmit} className="p-4 space-y-4">
                    {/* Task Name - Primary Input */}
                    <div className="space-y-1.5">
                        <label className="text-xs font-medium text-foreground">
                            What are you working on?
                        </label>
                        <input
                            type="text"
                            value={taskName}
                            onChange={(e) => setTaskName(e.target.value)}
                            placeholder="Fix login bug, Add payment flow, Refactor auth..."
                            className="w-full px-3 py-2.5 text-sm bg-muted/30 border border-border rounded-md focus:outline-none focus:ring-2 focus:ring-accent/50 placeholder:text-muted-foreground/50"
                            autoFocus
                        />
                    </div>

                    {/* Generated branch preview */}
                    {branchName && (
                        <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
                            <span>Branch:</span>
                            <code className="px-1.5 py-0.5 bg-muted/50 rounded font-mono">
                                {branchName}
                            </code>
                        </div>
                    )}

                    {/* Advanced Options Toggle */}
                    <button
                        type="button"
                        onClick={() => setShowAdvanced(!showAdvanced)}
                        className="flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground transition-colors"
                    >
                        {showAdvanced ? (
                            <ChevronUp className="w-3 h-3" />
                        ) : (
                            <ChevronDown className="w-3 h-3" />
                        )}
                        <span>Advanced options</span>
                    </button>

                    {/* Advanced Options */}
                    {showAdvanced && (
                        <div className="space-y-3 pt-1 border-t border-border/50">
                            {/* Custom Branch Name */}
                            <div className="space-y-1">
                                <label className="text-[10px] font-medium text-muted-foreground uppercase tracking-wide">
                                    Branch Name
                                </label>
                                <input
                                    type="text"
                                    value={branchName}
                                    onChange={(e) => setBranchName(e.target.value)}
                                    className="w-full px-2 py-1.5 text-xs bg-muted/30 border border-border rounded focus:outline-none focus:ring-1 focus:ring-accent font-mono"
                                />
                            </div>

                            {/* Worktree Path */}
                            <div className="space-y-1">
                                <label className="text-[10px] font-medium text-muted-foreground uppercase tracking-wide">
                                    Worktree Path
                                </label>
                                <input
                                    type="text"
                                    value={worktreePath}
                                    onChange={(e) => setWorktreePath(e.target.value)}
                                    className="w-full px-2 py-1.5 text-xs bg-muted/30 border border-border rounded focus:outline-none focus:ring-1 focus:ring-accent font-mono"
                                />
                            </div>
                        </div>
                    )}

                    {/* Error */}
                    {error && (
                        <div className="text-xs text-red-500 bg-red-500/10 px-3 py-2 rounded-md">
                            {error}
                        </div>
                    )}

                    {/* Actions */}
                    <div className="flex justify-end gap-2 pt-2">
                        <button
                            type="button"
                            onClick={onClose}
                            className="px-3 py-1.5 text-xs text-muted-foreground hover:text-foreground rounded-md transition-colors"
                        >
                            Cancel
                        </button>
                        <button
                            type="submit"
                            disabled={!taskName.trim() || !branchName.trim() || !worktreePath.trim() || isCreating}
                            className="px-4 py-1.5 text-xs bg-accent text-accent-foreground rounded-md hover:bg-accent/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                        >
                            {isCreating ? 'Creating...' : 'Start Session'}
                        </button>
                    </div>
                </form>
            </div>
        </div>
    );
}
