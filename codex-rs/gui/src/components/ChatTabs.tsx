import { Plus, X, History } from 'lucide-react';
import { useState, useRef, useEffect } from 'react';
import { ChatTab } from '../types';
import { TabConfigPersist } from '../tauri-api';

interface ChatTabsProps {
    tabs: ChatTab[];
    activeTabId: string;
    onSelectTab: (tabId: string) => void;
    onNewTab: () => void;
    onCloseTab: (tabId: string) => void;
    history?: TabConfigPersist[];
    onRestoreFromHistory?: (tab: TabConfigPersist) => void;
}

export function ChatTabs({
    tabs,
    activeTabId,
    onSelectTab,
    onNewTab,
    onCloseTab,
    history = [],
    onRestoreFromHistory,
}: ChatTabsProps) {
    const [showHistory, setShowHistory] = useState(false);
    const historyRef = useRef<HTMLDivElement>(null);

    // Close history dropdown when clicking outside
    useEffect(() => {
        const handleClickOutside = (e: MouseEvent) => {
            if (historyRef.current && !historyRef.current.contains(e.target as Node)) {
                setShowHistory(false);
            }
        };

        if (showHistory) {
            document.addEventListener('mousedown', handleClickOutside);
            return () => document.removeEventListener('mousedown', handleClickOutside);
        }
    }, [showHistory]);

    return (
        <div className="flex items-center border-b border-border bg-muted/30 h-9 shrink-0">
            {/* Tabs */}
            <div className="flex items-center gap-0.5 overflow-x-auto flex-1 h-full pl-3">
                {tabs.map((tab) => {
                    const isActive = tab.id === activeTabId;
                    const hasName = tab.name && tab.name.trim().length > 0;

                    return (
                        <button
                            key={tab.id}
                            onClick={() => onSelectTab(tab.id)}
                            className={`group relative flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-md transition-colors whitespace-nowrap ${
                                isActive
                                    ? 'bg-background text-foreground shadow-sm'
                                    : 'text-muted-foreground hover:text-foreground hover:bg-muted/50'
                            }`}
                        >
                            <span className={`max-w-[120px] truncate ${!hasName ? 'italic text-muted-foreground/70' : ''}`}>
                                {hasName ? tab.name : 'New chat'}
                            </span>

                            {/* Close button - show on hover or if active */}
                            {tabs.length > 1 && (
                                <span
                                    onClick={(e) => {
                                        e.stopPropagation();
                                        onCloseTab(tab.id);
                                    }}
                                    className={`p-0.5 rounded hover:bg-muted-foreground/20 ${
                                        isActive ? 'opacity-60 hover:opacity-100' : 'opacity-0 group-hover:opacity-60 hover:!opacity-100'
                                    }`}
                                >
                                    <X className="w-3 h-3" />
                                </span>
                            )}
                        </button>
                    );
                })}
            </div>

            {/* Action buttons */}
            <div className="flex items-center gap-1 mx-3">
                {/* New tab button */}
                <button
                    onClick={onNewTab}
                    className="p-1.5 text-muted-foreground hover:text-foreground hover:bg-muted/50 rounded transition-colors"
                    title="New chat"
                >
                    <Plus className="w-4 h-4" />
                </button>

                {/* History button */}
                {history.length > 0 && (
                    <div className="relative" ref={historyRef}>
                        <button
                            onClick={() => setShowHistory(!showHistory)}
                            className={`p-1.5 text-muted-foreground hover:text-foreground hover:bg-muted/50 rounded transition-colors ${
                                showHistory ? 'bg-muted/50 text-foreground' : ''
                            }`}
                            title="Chat history"
                        >
                            <History className="w-4 h-4" />
                        </button>

                        {/* History dropdown */}
                        {showHistory && (
                            <div className="absolute right-0 top-full mt-1 w-64 bg-background border border-border rounded-lg shadow-lg z-50 py-1 max-h-80 overflow-y-auto">
                                <div className="px-3 py-1.5 text-xs font-medium text-muted-foreground border-b border-border">
                                    Recent chats
                                </div>
                                {history.map((item) => (
                                    <button
                                        key={item.id}
                                        onClick={() => {
                                            onRestoreFromHistory?.(item);
                                            setShowHistory(false);
                                        }}
                                        className="w-full px-3 py-2 text-left text-sm hover:bg-muted/50 transition-colors flex flex-col gap-0.5"
                                    >
                                        <span className="truncate text-foreground">
                                            {item.name || 'Untitled chat'}
                                        </span>
                                        <span className="text-xs text-muted-foreground">
                                            {new Date(item.createdAt).toLocaleDateString()}
                                        </span>
                                    </button>
                                ))}
                            </div>
                        )}
                    </div>
                )}
            </div>
        </div>
    );
}
