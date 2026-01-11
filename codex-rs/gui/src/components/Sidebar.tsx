import { useEffect, useMemo, useState } from "react";
import { Plus } from "lucide-react";
import { Repository, WorktreeSession } from "../types";
import { RepositoryGroup } from "./RepositoryGroup";

interface SidebarProps {
  repositories: Repository[];
  sessions: WorktreeSession[];
  activeSessionId: string | null;
  onSelectSession: (sessionId: string, worktreePath?: string) => void;
  onAddRepository: () => void;
  onCreateSession: (repositoryId: string) => void;
  onRemoveRepository: (repositoryId: string) => void;
  onRemoveWorktree?: (repositoryId: string, worktreePath: string) => void;
}

export function Sidebar({
  repositories,
  sessions,
  activeSessionId,
  onSelectSession,
  onAddRepository,
  onCreateSession,
  onRemoveRepository,
  onRemoveWorktree,
}: SidebarProps) {
  const [expandedRepos, setExpandedRepos] = useState<Record<string, boolean>>(
    {},
  );

  // Default new repos to expanded unless explicitly set
  useEffect(() => {
    setExpandedRepos((prev) => {
      const next = { ...prev };
      repositories.forEach((repo) => {
        if (next[repo.id] === undefined) {
          next[repo.id] = repo.expanded ?? true;
        }
      });
      return next;
    });
  }, [repositories]);

  const groupedSessions = useMemo(() => {
    const map = new Map<string, WorktreeSession[]>();
    sessions.forEach((session) => {
      const list = map.get(session.repositoryId) ?? [];
      list.push(session);
      map.set(session.repositoryId, list);
    });
    return map;
  }, [sessions]);

  return (
    <div className="flex flex-col h-full bg-sidebar">
      <div className="px-3 h-9 flex items-center justify-between border-b border-border bg-muted/30">
        <span className="text-sidebar-foreground/70 text-xs font-semibold tracking-wide uppercase">
          Workspaces
        </span>
        <button
          onClick={onAddRepository}
          className="p-1 text-sidebar-foreground/50 hover:text-sidebar-foreground hover:bg-sidebar-accent/60 rounded transition-colors"
          title="Add repository"
        >
          <Plus className="w-4 h-4" strokeWidth={2} />
        </button>
      </div>

      <div className="flex-1 overflow-y-auto px-2 pb-2">
        {repositories.length === 0 ? (
          <EmptyState message="Add a folder to get started" />
        ) : (
          <div className="space-y-1">
            {repositories.map((repo) => (
              <RepositoryGroup
                key={repo.id}
                repository={{
                  ...repo,
                  expanded: expandedRepos[repo.id] ?? true,
                }}
                sessions={groupedSessions.get(repo.id) ?? []}
                activeSessionId={activeSessionId}
                onSelectSession={onSelectSession}
                onCreateWorktree={() => onCreateSession(repo.id)}
                onRemoveRepository={() => onRemoveRepository(repo.id)}
                onRemoveWorktree={onRemoveWorktree ? (path) => onRemoveWorktree(repo.id, path) : undefined}
                onToggleExpand={() =>
                  setExpandedRepos((prev) => ({
                    ...prev,
                    [repo.id]: !(prev[repo.id] ?? true),
                  }))
                }
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function EmptyState({ message }: { message: string }) {
  return (
    <div className="flex items-center justify-center h-24">
      <p className="text-[12px] text-sidebar-foreground/30">{message}</p>
    </div>
  );
}
