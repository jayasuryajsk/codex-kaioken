import { ChevronRight, Plus, MoreHorizontal } from "lucide-react";
import { useState, useRef, useEffect } from "react";
import { Repository, WorktreeSession } from "../types";
import { WorktreeItem } from "./WorktreeItem";

interface RepositoryGroupProps {
  repository: Repository;
  sessions: WorktreeSession[];
  activeSessionId: string | null;
  onSelectSession: (sessionId: string, worktreePath?: string) => void;
  onCreateWorktree: () => void;
  onRemoveRepository: () => void;
  onRemoveWorktree?: (worktreePath: string) => void;
  onToggleExpand: () => void;
}

export function RepositoryGroup({
  repository,
  sessions,
  activeSessionId,
  onSelectSession,
  onCreateWorktree,
  onRemoveRepository,
  onRemoveWorktree,
  onToggleExpand,
}: RepositoryGroupProps) {
  const [showMenu, setShowMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setShowMenu(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  const getSessionForWorktree = (worktreePath: string) => {
    return sessions.find((s) => s.worktreePath === worktreePath);
  };

  return (
    <div>
      {/* Repository Header */}
      <div className="w-full flex items-center gap-2 px-3 py-1.5 rounded-md hover:bg-sidebar-accent/40 transition-colors">
        <button
          onClick={onToggleExpand}
          className="flex items-center gap-2 flex-1 group text-left"
        >
          <ChevronRight
            className={`w-3 h-3 text-sidebar-foreground/30 transition-transform duration-200 ${
              repository.expanded ? "rotate-90" : ""
            }`}
            strokeWidth={2}
          />
          <span className="text-[12px] font-semibold text-sidebar-foreground/75 uppercase tracking-[0.08em] leading-tight group-hover:text-sidebar-foreground transition-colors truncate">
            {repository.name}
          </span>
        </button>
        <div className="relative" ref={menuRef}>
          <button
            onClick={() => setShowMenu(!showMenu)}
            className="p-1.5 text-sidebar-foreground/40 hover:text-sidebar-foreground/80 hover:bg-sidebar-accent/50 rounded-md transition-colors"
          >
            <MoreHorizontal className="w-3.5 h-3.5" strokeWidth={2} />
          </button>
          {showMenu && (
            <div className="absolute right-0 top-full mt-1 w-36 bg-white dark:bg-zinc-900 border border-border rounded-md shadow-lg z-50 overflow-hidden">
              <button
                onClick={() => { onCreateWorktree(); setShowMenu(false); }}
                className="w-full flex items-center gap-2 px-3 py-2 text-xs text-foreground hover:bg-accent transition-colors"
              >
                <Plus className="w-3.5 h-3.5" />
                New session
              </button>
              <button
                onClick={() => { onRemoveRepository(); setShowMenu(false); }}
                className="w-full flex items-center gap-2 px-3 py-2 text-xs text-red-500 hover:bg-accent transition-colors"
              >
                Remove
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Worktrees List */}
      {repository.expanded && (
        <div className="mt-1 px-2">
          {repository.worktrees.map((worktree) => {
            const session = getSessionForWorktree(worktree.path);
            const sessionId = session?.id ?? `new-${worktree.path}`;

            return (
              <WorktreeItem
                key={worktree.path}
                worktree={worktree}
                session={session}
                isActive={session?.id === activeSessionId}
                onClick={() => {
                  if (session) {
                    onSelectSession(session.id);
                  } else {
                    onSelectSession(sessionId, worktree.path);
                  }
                }}
                onRemove={onRemoveWorktree ? () => onRemoveWorktree(worktree.path) : undefined}
              />
            );
          })}

        </div>
      )}
    </div>
  );
}
