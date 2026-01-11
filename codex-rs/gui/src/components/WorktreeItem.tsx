import { GitBranch, Loader2, MoreHorizontal, Trash2 } from "lucide-react";
import { useState, useRef, useEffect } from "react";
import { WorktreeInfo, WorktreeSession } from "../types";

interface WorktreeItemProps {
  worktree: WorktreeInfo;
  session?: WorktreeSession;
  isActive: boolean;
  onClick: () => void;
  onRemove?: () => void;
}

export function WorktreeItem({
  worktree,
  session,
  isActive,
  onClick,
  onRemove,
}: WorktreeItemProps) {
  const [showMenu, setShowMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const isWorking =
    session?.status === "working" || session?.status === "thinking";

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setShowMenu(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  return (
    <div
      className={`group w-full flex items-center gap-2 px-3 py-1.5 rounded-md transition-colors ${
        isActive
          ? "bg-sidebar-accent text-sidebar-foreground"
          : "text-sidebar-foreground/60 hover:text-sidebar-foreground hover:bg-sidebar-accent/40"
      }`}
    >
      <button onClick={onClick} className="flex items-center gap-2 flex-1 min-w-0 text-left">
        <GitBranch className="w-3.5 h-3.5 shrink-0 opacity-50" strokeWidth={1.5} />
        <span className={`text-[12px] truncate ${isActive ? "font-medium" : ""}`}>
          {worktree.branch || worktree.name}
        </span>
      </button>
      {isWorking && (
        <Loader2 className="w-3 h-3 animate-spin opacity-60" strokeWidth={2} />
      )}
      <div className="relative" ref={menuRef}>
          <button
            onClick={(e) => { e.stopPropagation(); setShowMenu(!showMenu); }}
            className="p-0.5 opacity-0 group-hover:opacity-100 text-sidebar-foreground/40 hover:text-sidebar-foreground transition-opacity"
          >
            <MoreHorizontal className="w-3.5 h-3.5" />
          </button>
          {showMenu && (
            <div className="absolute right-0 top-full mt-1 w-28 bg-white dark:bg-zinc-900 border border-border rounded-md shadow-lg z-50 overflow-hidden">
              <button
                onClick={(e) => { e.stopPropagation(); onRemove?.(); setShowMenu(false); }}
                className="w-full flex items-center gap-2 px-2.5 py-1.5 text-xs text-red-500 hover:bg-accent transition-colors"
              >
                <Trash2 className="w-3 h-3" />
                Remove
              </button>
            </div>
          )}
      </div>
    </div>
  );
}
