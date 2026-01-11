import { FolderOpen, Zap, AlertTriangle, Info, Brain, Lightbulb, MapPin, Heart, GitBranch, BookOpen } from 'lucide-react';
import { useState, useEffect } from 'react';
import { AgentSession } from '../types';
import { getMemories, MemoryEntry } from '../tauri-api';

interface TokenUsageData {
    total: { input: number; output: number; cached: number; reasoning: number; total: number };
    last: { input: number; output: number; total: number };
    contextWindow: number | null;
}

interface SystemMessage {
    id: string;
    type: string;
    message: string;
    timestamp: Date;
}

interface RightPanelProps {
    session: AgentSession;
    tokenUsage?: TokenUsageData | null;
    systemMessages?: SystemMessage[];
    worktreePath?: string;
}

const MEMORY_TYPE_CONFIG: Record<string, { icon: React.ElementType; label: string; color: string }> = {
    fact: { icon: BookOpen, label: 'Fact', color: 'text-blue-500' },
    pattern: { icon: GitBranch, label: 'Pattern', color: 'text-purple-500' },
    decision: { icon: Lightbulb, label: 'Decision', color: 'text-yellow-500' },
    lesson: { icon: Brain, label: 'Lesson', color: 'text-red-500' },
    preference: { icon: Heart, label: 'Preference', color: 'text-pink-500' },
    location: { icon: MapPin, label: 'Location', color: 'text-green-500' },
};

export function RightPanel({ session, tokenUsage, systemMessages = [], worktreePath }: RightPanelProps) {
    const hasMessages = session.messages.length > 0;
    const [memories, setMemories] = useState<MemoryEntry[]>([]);

    // Fetch memories when worktreePath changes or messages change (to catch new memories)
    useEffect(() => {
        if (!worktreePath) {
            setMemories([]);
            return;
        }

        const fetchMemories = () => {
            getMemories(worktreePath)
                .then(setMemories)
                .catch((err) => console.error('Failed to fetch memories:', err));
        };

        fetchMemories();

        // Refresh memories every 5 seconds while in session
        const interval = setInterval(fetchMemories, 5000);
        return () => clearInterval(interval);
    }, [worktreePath, session.messages.length]);

    return (
        <div className="flex flex-col h-full bg-background border-l border-border w-full flex-shrink-0">
            {/* Context / Files Area */}
            <div className="flex-1 flex flex-col min-h-0">
                <div className="flex items-center gap-4 px-4 h-9 border-b border-border bg-muted/30 text-xs font-medium">
                    <span className="text-foreground">Context</span>
                    <span className="text-muted-foreground">Files</span>
                </div>

                <div className="flex-1 overflow-y-auto p-4">
                    {!hasMessages ? (
                        <div className="flex flex-col items-center justify-center h-full text-center">
                            <FolderOpen className="w-10 h-10 text-muted-foreground/30 mb-3" />
                            <p className="text-sm text-muted-foreground">No files yet</p>
                            <p className="text-xs text-muted-foreground/70 mt-1">
                                Files mentioned in conversation will appear here
                            </p>
                        </div>
                    ) : (
                        <div className="space-y-1">
                            <p className="text-xs text-muted-foreground mb-2">
                                {session.messages.length} message{session.messages.length !== 1 ? 's' : ''} in conversation
                            </p>
                        </div>
                    )}
                </div>
            </div>

            {/* System Messages */}
            {systemMessages.length > 0 && (
                <div className="px-4 py-2 border-t border-border max-h-32 overflow-y-auto">
                    <div className="space-y-1">
                        {systemMessages.slice(-5).map((msg) => (
                            <div key={msg.id} className="flex items-start gap-2 text-xs">
                                {msg.type === 'warning' && <AlertTriangle className="w-3 h-3 text-yellow-500 mt-0.5 flex-shrink-0" />}
                                {msg.type === 'error' && <AlertTriangle className="w-3 h-3 text-red-500 mt-0.5 flex-shrink-0" />}
                                {(msg.type === 'info' || msg.type === 'background') && <Info className="w-3 h-3 text-blue-500 mt-0.5 flex-shrink-0" />}
                                <span className="text-muted-foreground truncate">{msg.message}</span>
                            </div>
                        ))}
                    </div>
                </div>
            )}

            {/* Memory Section */}
            {memories.length > 0 && (
                <div className="px-4 py-3 border-t border-border">
                    <div className="flex items-center gap-2 mb-2">
                        <Brain className="w-3.5 h-3.5 text-purple-500" />
                        <span className="text-xs font-medium text-foreground">Memory</span>
                        <span className="text-[10px] text-muted-foreground">({memories.length})</span>
                    </div>
                    <div className="space-y-1.5 max-h-40 overflow-y-auto">
                        {memories.slice(0, 8).map((memory) => {
                            const config = MEMORY_TYPE_CONFIG[memory.memory_type] || MEMORY_TYPE_CONFIG.fact;
                            const Icon = config.icon;
                            return (
                                <div
                                    key={memory.id}
                                    className="flex items-start gap-2 text-xs group"
                                    title={memory.content}
                                >
                                    <Icon className={`w-3 h-3 mt-0.5 flex-shrink-0 ${config.color}`} />
                                    <span className="text-muted-foreground line-clamp-2 leading-relaxed">
                                        {memory.content}
                                    </span>
                                </div>
                            );
                        })}
                    </div>
                </div>
            )}

            {/* Token Usage - Bottom Right */}
            {tokenUsage && (
                <div className="mt-auto px-4 py-3 border-t border-border bg-muted/30">
                    <div className="flex items-center gap-2 mb-2">
                        <Zap className="w-3.5 h-3.5 text-yellow-500" />
                        <span className="text-xs font-medium text-foreground">Token Usage</span>
                    </div>
                    <div className="space-y-1 text-xs text-muted-foreground font-mono">
                        <div className="flex justify-between">
                            <span>Input:</span>
                            <span>{tokenUsage.total.input.toLocaleString()}</span>
                        </div>
                        {tokenUsage.total.cached > 0 && (
                            <div className="flex justify-between text-muted-foreground/70">
                                <span className="pl-2">Cached:</span>
                                <span>{tokenUsage.total.cached.toLocaleString()}</span>
                            </div>
                        )}
                        <div className="flex justify-between">
                            <span>Output:</span>
                            <span>{tokenUsage.total.output.toLocaleString()}</span>
                        </div>
                        {tokenUsage.total.reasoning > 0 && (
                            <div className="flex justify-between text-muted-foreground/70">
                                <span className="pl-2">Reasoning:</span>
                                <span>{tokenUsage.total.reasoning.toLocaleString()}</span>
                            </div>
                        )}
                        <div className="flex justify-between font-semibold text-foreground">
                            <span>Total:</span>
                            <span>{tokenUsage.total.total.toLocaleString()}</span>
                        </div>
                        {tokenUsage.contextWindow && (() => {
                            // Match TUI calculation: uses cumulative total with baseline offset
                            const BASELINE_TOKENS = 12000;
                            const effectiveWindow = tokenUsage.contextWindow - BASELINE_TOKENS;
                            const used = Math.max(0, tokenUsage.total.total - BASELINE_TOKENS);
                            const remaining = Math.max(0, effectiveWindow - used);
                            const percentRemaining = Math.round((remaining / effectiveWindow) * 100);
                            const percentUsed = 100 - percentRemaining;
                            return (
                                <div className="mt-2">
                                    <div className="h-1.5 bg-muted rounded-full overflow-hidden">
                                        <div
                                            className={`h-full transition-all ${percentRemaining < 10 ? 'bg-red-500' : percentRemaining < 30 ? 'bg-yellow-500' : 'bg-primary'}`}
                                            style={{ width: `${Math.min(100, percentUsed)}%` }}
                                        />
                                    </div>
                                    <div className="flex justify-between mt-1 text-[10px]">
                                        <span>Context</span>
                                        <span>{percentRemaining}% left</span>
                                    </div>
                                </div>
                            );
                        })()}
                    </div>
                </div>
            )}

        </div>
    );
}
