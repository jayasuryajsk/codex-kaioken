import { useState, useEffect } from 'react';
import { X, GitBranch, FolderOpen } from 'lucide-react';
import { Repository } from '../types';

interface CreateWorktreeModalProps {
    repository: Repository;
    onClose: () => void;
    onCreate: (branchName: string, worktreePath: string) => Promise<void>;
}

export function CreateWorktreeModal({
    repository,
    onClose,
    onCreate,
}: CreateWorktreeModalProps) {
    const [branchName, setBranchName] = useState('');
    const [worktreePath, setWorktreePath] = useState('');
    const [isCreating, setIsCreating] = useState(false);
    const [error, setError] = useState<string | null>(null);

    // Auto-generate worktree path based on branch name
    useEffect(() => {
        if (branchName) {
            // Create sibling directory: /path/to/repo -> /path/to/repo-branch-name
            const repoDir = repository.rootPath;
            const parentDir = repoDir.substring(0, repoDir.lastIndexOf('/'));
            const repoName = repoDir.substring(repoDir.lastIndexOf('/') + 1);
            const safeBranchName = branchName.replace(/[^a-zA-Z0-9-_]/g, '-');
            setWorktreePath(`${parentDir}/${repoName}-${safeBranchName}`);
        } else {
            setWorktreePath('');
        }
    }, [branchName, repository.rootPath]);

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        if (!branchName.trim() || !worktreePath.trim()) return;

        setIsCreating(true);
        setError(null);

        try {
            await onCreate(branchName.trim(), worktreePath.trim());
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
                        <GitBranch className="w-4 h-4 text-muted-foreground" />
                        <h2 className="text-sm font-medium">New Worktree</h2>
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
                    <div className="text-xs text-muted-foreground mb-3">
                        Creating worktree for <span className="font-medium text-foreground">{repository.name}</span>
                    </div>

                    {/* Branch Name */}
                    <div className="space-y-1.5">
                        <label className="text-xs font-medium text-muted-foreground">
                            Branch Name
                        </label>
                        <div className="relative">
                            <GitBranch className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground/50" />
                            <input
                                type="text"
                                value={branchName}
                                onChange={(e) => setBranchName(e.target.value)}
                                placeholder="feature/my-feature"
                                className="w-full pl-9 pr-3 py-2 text-sm bg-muted/30 border border-border rounded-md focus:outline-none focus:ring-1 focus:ring-accent placeholder:text-muted-foreground/50"
                                autoFocus
                            />
                        </div>
                        <p className="text-[10px] text-muted-foreground/70">
                            Enter existing branch or new branch name
                        </p>
                    </div>

                    {/* Worktree Path */}
                    <div className="space-y-1.5">
                        <label className="text-xs font-medium text-muted-foreground">
                            Worktree Path
                        </label>
                        <div className="relative">
                            <FolderOpen className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground/50" />
                            <input
                                type="text"
                                value={worktreePath}
                                onChange={(e) => setWorktreePath(e.target.value)}
                                placeholder="/path/to/worktree"
                                className="w-full pl-9 pr-3 py-2 text-sm bg-muted/30 border border-border rounded-md focus:outline-none focus:ring-1 focus:ring-accent placeholder:text-muted-foreground/50 font-mono text-xs"
                            />
                        </div>
                        <p className="text-[10px] text-muted-foreground/70">
                            Directory will be created if it doesn't exist
                        </p>
                    </div>

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
                            disabled={!branchName.trim() || !worktreePath.trim() || isCreating}
                            className="px-3 py-1.5 text-xs bg-accent text-accent-foreground rounded-md hover:bg-accent/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                        >
                            {isCreating ? 'Creating...' : 'Create Worktree'}
                        </button>
                    </div>
                </form>
            </div>
        </div>
    );
}
