import { Sparkles, GitBranch, FolderOpen } from 'lucide-react';

interface WelcomeCardProps {
    repoName: string;
    branchName?: string;
    worktreePath?: string;
}

export function WelcomeCard({ repoName, branchName, worktreePath }: WelcomeCardProps) {
    return (
        <div className="flex flex-col items-center justify-center py-16 px-4">
            {/* Icon */}
            <div className="w-14 h-14 rounded-2xl bg-accent/10 flex items-center justify-center mb-5">
                <Sparkles className="w-7 h-7 text-accent" strokeWidth={1.5} />
            </div>

            {/* Title */}
            <h2 className="text-lg font-semibold text-foreground mb-2">
                Ready to work on {repoName}
            </h2>

            {/* Subtitle */}
            <p className="text-sm text-muted-foreground mb-6 text-center max-w-md">
                Start a conversation to begin coding. I can help you write features, fix bugs, refactor code, and more.
            </p>

            {/* Info chips */}
            <div className="flex items-center gap-3 text-xs text-muted-foreground/70">
                {branchName && (
                    <div className="flex items-center gap-1.5 px-2.5 py-1 bg-muted/50 rounded-full">
                        <GitBranch className="w-3 h-3" strokeWidth={1.5} />
                        <span>{branchName}</span>
                    </div>
                )}
                {worktreePath && (
                    <div className="flex items-center gap-1.5 px-2.5 py-1 bg-muted/50 rounded-full">
                        <FolderOpen className="w-3 h-3" strokeWidth={1.5} />
                        <span className="truncate max-w-[200px]">{worktreePath.split('/').slice(-2).join('/')}</span>
                    </div>
                )}
            </div>
        </div>
    );
}
