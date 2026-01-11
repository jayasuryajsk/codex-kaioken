import { useState, useRef, useEffect, useCallback } from 'react';
import {
    ArrowUp,
    Zap,
    Shield,
    Brain,
    ChevronDown,
    FileText,
    Square
} from 'lucide-react';
import { KeyboardEvent } from 'react';
import { AppSettings, ReasoningEffort } from '../types';
import { listFiles } from '../tauri-api';

// Parse text to find @mentions and return segments
function parseTextWithMentions(text: string): Array<{ type: 'text' | 'mention'; content: string }> {
    const segments: Array<{ type: 'text' | 'mention'; content: string }> = [];
    // Match @followed by non-whitespace characters (file paths)
    const mentionRegex = /@(\S+)/g;
    let lastIndex = 0;
    let match;

    while ((match = mentionRegex.exec(text)) !== null) {
        // Add text before mention
        if (match.index > lastIndex) {
            segments.push({ type: 'text', content: text.slice(lastIndex, match.index) });
        }
        // Add mention
        segments.push({ type: 'mention', content: match[0] });
        lastIndex = match.index + match[0].length;
    }

    // Add remaining text
    if (lastIndex < text.length) {
        segments.push({ type: 'text', content: text.slice(lastIndex) });
    }

    return segments;
}

// Official model presets from codex-rs/common/src/model_presets.rs
const MODEL_PRESETS = [
    {
        id: 'gpt-5.2-codex',
        name: 'gpt-5.2-codex',
        description: 'Latest frontier agentic coding model',
        isDefault: true,
        efforts: ['low', 'medium', 'high', 'xhigh'] as ReasoningEffort[]
    },
    {
        id: 'gpt-5.1-codex-max',
        name: 'gpt-5.1-codex-max',
        description: 'Deep and fast reasoning flagship',
        efforts: ['low', 'medium', 'high', 'xhigh'] as ReasoningEffort[]
    },
    {
        id: 'gpt-5.1-codex-mini',
        name: 'gpt-5.1-codex-mini',
        description: 'Cheaper, faster, less capable',
        efforts: ['medium', 'high'] as ReasoningEffort[]
    },
    {
        id: 'gpt-5.2',
        name: 'gpt-5.2',
        description: 'Latest frontier model',
        efforts: ['low', 'medium', 'high'] as ReasoningEffort[]
    },
];

const EFFORT_LABELS: Record<ReasoningEffort, { short: string; description: string }> = {
    low: { short: 'Low', description: 'Fast responses with lighter reasoning' },
    medium: { short: 'Med', description: 'Balances speed and reasoning depth' },
    high: { short: 'High', description: 'Maximizes reasoning depth' },
    xhigh: { short: 'Max', description: 'Extra high reasoning for complex problems' },
};

interface InputAreaProps {
    onSend: (message: string) => void;
    onCancel?: () => void;
    isLoading: boolean;
    settings: AppSettings;
    onTogglePlanMode: () => void;
    onCycleApproval: () => void;
    onModelChange?: (model: string) => void;
    onReasoningEffortChange?: (effort: ReasoningEffort) => void;
    sessionId?: string;
}

export function InputArea({
    onSend,
    onCancel,
    isLoading,
    settings,
    onTogglePlanMode,
    onCycleApproval,
    onModelChange,
    onReasoningEffortChange,
    sessionId,
}: InputAreaProps) {
    const [input, setInput] = useState('');
    const [showModelPicker, setShowModelPicker] = useState(false);
    const [showEffortPicker, setShowEffortPicker] = useState(false);
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const toolbarRef = useRef<HTMLDivElement>(null);

    // File mention state
    const [showFilePicker, setShowFilePicker] = useState(false);
    const [fileQuery, setFileQuery] = useState('');
    const [fileSuggestions, setFileSuggestions] = useState<string[]>([]);
    const [selectedFileIndex, setSelectedFileIndex] = useState(0);
    const [mentionStartPos, setMentionStartPos] = useState<number | null>(null);
    const [isFocused, setIsFocused] = useState(false);
    const filePickerRef = useRef<HTMLDivElement>(null);

    const currentModel = MODEL_PRESETS.find(m => m.id === settings.model) || MODEL_PRESETS[0];
    const availableEfforts = currentModel.efforts;
    const currentEffort = settings.reasoningEffort;

    // Fetch file suggestions when query changes
    const fetchFileSuggestions = useCallback(async (query: string) => {
        if (!sessionId) return;
        try {
            const files = await listFiles(sessionId, query);
            setFileSuggestions(files);
            setSelectedFileIndex(0);
        } catch (err) {
            console.error('Failed to fetch files:', err);
            setFileSuggestions([]);
        }
    }, [sessionId]);

    // Debounced file search
    useEffect(() => {
        if (!showFilePicker) return;
        const timer = setTimeout(() => {
            fetchFileSuggestions(fileQuery);
        }, 100);
        return () => clearTimeout(timer);
    }, [fileQuery, showFilePicker, fetchFileSuggestions]);

    // Insert selected file into input
    const insertFileMention = useCallback((file: string) => {
        if (mentionStartPos === null) return;

        const before = input.slice(0, mentionStartPos);
        const after = input.slice(mentionStartPos + fileQuery.length + 1); // +1 for @
        const newInput = `${before}@${file} ${after}`;

        setInput(newInput);
        setShowFilePicker(false);
        setFileQuery('');
        setMentionStartPos(null);

        // Focus back on textarea
        setTimeout(() => {
            if (textareaRef.current) {
                textareaRef.current.focus();
                const newPos = before.length + file.length + 2; // +2 for @ and space
                textareaRef.current.setSelectionRange(newPos, newPos);
            }
        }, 0);
    }, [input, fileQuery, mentionStartPos]);

    // Close dropdowns when clicking outside
    useEffect(() => {
        const handleClickOutside = (e: MouseEvent) => {
            if (toolbarRef.current && !toolbarRef.current.contains(e.target as Node)) {
                setShowModelPicker(false);
                setShowEffortPicker(false);
            }
            if (filePickerRef.current && !filePickerRef.current.contains(e.target as Node)) {
                setShowFilePicker(false);
                setFileQuery('');
                setMentionStartPos(null);
            }
        };
        document.addEventListener('mousedown', handleClickOutside);
        return () => document.removeEventListener('mousedown', handleClickOutside);
    }, []);

    const handleSubmit = () => {
        if (!input.trim() || isLoading) return;
        onSend(input.trim());
        setInput('');
        if (textareaRef.current) {
            textareaRef.current.style.height = 'auto';
        }
    };

    const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
        // Handle file picker navigation
        if (showFilePicker && fileSuggestions.length > 0) {
            if (e.key === 'ArrowDown') {
                e.preventDefault();
                setSelectedFileIndex(prev => Math.min(prev + 1, fileSuggestions.length - 1));
                return;
            }
            if (e.key === 'ArrowUp') {
                e.preventDefault();
                setSelectedFileIndex(prev => Math.max(prev - 1, 0));
                return;
            }
            if (e.key === 'Enter' || e.key === 'Tab') {
                e.preventDefault();
                insertFileMention(fileSuggestions[selectedFileIndex]);
                return;
            }
            if (e.key === 'Escape') {
                e.preventDefault();
                setShowFilePicker(false);
                setFileQuery('');
                setMentionStartPos(null);
                return;
            }
        }

        // Normal enter to submit
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            handleSubmit();
        }
    };

    const handleInput = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
        const value = e.target.value;
        const textarea = e.target;
        const cursorPos = textarea.selectionStart;

        setInput(value);
        textarea.style.height = 'auto';
        textarea.style.height = `${Math.min(textarea.scrollHeight, 150)}px`;

        // Detect @ mention
        // Find the last @ before cursor that isn't part of a completed mention
        const textBeforeCursor = value.slice(0, cursorPos);
        const lastAtIndex = textBeforeCursor.lastIndexOf('@');

        if (lastAtIndex !== -1) {
            // Check if this @ is at start or preceded by whitespace
            const charBefore = lastAtIndex > 0 ? value[lastAtIndex - 1] : ' ';
            if (charBefore === ' ' || charBefore === '\n' || lastAtIndex === 0) {
                const queryAfterAt = textBeforeCursor.slice(lastAtIndex + 1);
                // Only show picker if query doesn't contain space (not a completed mention)
                if (!queryAfterAt.includes(' ')) {
                    setShowFilePicker(true);
                    setFileQuery(queryAfterAt);
                    setMentionStartPos(lastAtIndex);
                    return;
                }
            }
        }

        // Close file picker if no valid @ mention
        if (showFilePicker) {
            setShowFilePicker(false);
            setFileQuery('');
            setMentionStartPos(null);
        }
    };

    const handleModelSelect = (modelId: string) => {
        onModelChange?.(modelId);
        setShowModelPicker(false);
        // Reset effort if not supported by new model
        const newModel = MODEL_PRESETS.find(m => m.id === modelId);
        if (newModel && !newModel.efforts.includes(currentEffort)) {
            onReasoningEffortChange?.(newModel.efforts[0]);
        }
    };

    const handleEffortSelect = (effort: ReasoningEffort) => {
        onReasoningEffortChange?.(effort);
        setShowEffortPicker(false);
    };

    const isPlanMode = settings.planMode;

    return (
        <div className={`relative flex flex-col border bg-card hover:border-border/70 transition-colors mx-4 mb-2 rounded-lg ${isPlanMode
                ? 'border-dashed border-primary/50'
                : 'border-solid border-border'
            }`}>

            {/* File Picker Popup */}
            {showFilePicker && (
                <div
                    ref={filePickerRef}
                    className="absolute bottom-full left-4 mb-1 w-72 max-h-52 overflow-y-auto bg-white dark:bg-zinc-900 border border-border rounded-md shadow-lg z-50"
                >
                    {fileSuggestions.length > 0 ? (
                        <div className="py-0.5">
                            {fileSuggestions.slice(0, 8).map((file, index) => (
                                <button
                                    key={file}
                                    onClick={() => insertFileMention(file)}
                                    className={`w-full flex items-center gap-2 px-2.5 py-1.5 text-left transition-colors ${index === selectedFileIndex ? 'bg-accent' : 'hover:bg-muted/50'
                                        }`}
                                >
                                    <FileText className="w-3.5 h-3.5 text-muted-foreground/50 shrink-0" />
                                    <span className="text-xs text-foreground truncate">{file}</span>
                                </button>
                            ))}
                        </div>
                    ) : fileQuery ? (
                        <div className="px-3 py-3 text-xs text-muted-foreground text-center">
                            No files found
                        </div>
                    ) : (
                        <div className="px-3 py-3 text-xs text-muted-foreground text-center">
                            Type to search...
                        </div>
                    )}
                </div>
            )}

            {/* Input Container with overlay for highlighting */}
            <div className="relative flex-1 min-h-[60px]">
                {/* Actual textarea - invisible text, caret visible */}
                <textarea
                    ref={textareaRef}
                    value={input}
                    onChange={handleInput}
                    onKeyDown={handleKeyDown}
                    onFocus={() => setIsFocused(true)}
                    onBlur={() => setIsFocused(false)}
                    rows={1}
                    className="absolute inset-0 w-full h-full bg-transparent px-4 py-3 resize-none focus:outline-none text-[15px] leading-relaxed min-h-[60px] max-h-[300px] z-10"
                    style={{
                        color: 'transparent',
                        caretColor: 'var(--foreground)'
                    }}
                />

                {/* Styled text overlay - visible, clicks pass through to textarea */}
                <div
                    className="px-4 py-3 text-[15px] leading-relaxed whitespace-pre-wrap break-words pointer-events-none min-h-[60px] max-h-[300px] overflow-hidden"
                    aria-hidden="true"
                >
                    {input ? (
                        <>
                            {parseTextWithMentions(input).map((segment, i) =>
                                segment.type === 'mention' ? (
                                    <span
                                        key={i}
                                        className="bg-primary/15 text-primary rounded px-0.5"
                                    >
                                        {segment.content}
                                    </span>
                                ) : (
                                    <span key={i} className="text-foreground">{segment.content}</span>
                                )
                            )}
                        </>
                    ) : (
                        <span className="text-muted-foreground/60 inline-flex items-center">
                            {isFocused && (
                                <span className="inline-block w-[2px] h-[1em] bg-foreground animate-blink mr-1" />
                            )}
                            {isPlanMode
                                ? "Describe your goal..."
                                : "Ask to make changes, @mention files..."
                            }
                        </span>
                    )}
                </div>
            </div>

            {/* Bottom Toolbar */}
            <div ref={toolbarRef} className="flex items-center justify-between px-3 py-1.5 flex-shrink-0">
                {/* Left: Model + Effort + Status Toggles */}
                <div className="flex items-center gap-3">
                    {/* Model Selector */}
                    <div className="relative">
                        <button
                            onClick={() => { setShowModelPicker(!showModelPicker); setShowEffortPicker(false); }}
                            className="flex items-center gap-1 px-1 py-1 hover:bg-muted/50 transition-colors group"
                        >
                            <span className="text-xs font-medium text-muted-foreground group-hover:text-foreground">{currentModel.name}</span>
                            <ChevronDown className="w-3 h-3 text-muted-foreground/60" />
                        </button>

                        {/* Model Dropdown */}
                        {showModelPicker && (
                            <div className="absolute bottom-full left-0 mb-1 w-48 bg-white dark:bg-zinc-900 border border-border rounded-md shadow-xl z-50 overflow-hidden">
                                {MODEL_PRESETS.map((model) => (
                                    <button
                                        key={model.id}
                                        onClick={() => handleModelSelect(model.id)}
                                        className={`w-full text-left px-2 py-1.5 hover:bg-accent transition-colors ${model.id === settings.model ? 'bg-accent' : ''
                                            }`}
                                    >
                                        <div className="flex items-center justify-between">
                                            <span className="text-[10px] font-medium text-foreground">{model.name}</span>
                                            {model.isDefault && (
                                                <span className="text-[8px] px-1 py-0.5 bg-primary/20 text-primary">default</span>
                                            )}
                                        </div>
                                        <div className="text-[9px] text-muted-foreground">{model.description}</div>
                                    </button>
                                ))}
                            </div>
                        )}
                    </div>

                    {/* Reasoning Effort Selector */}
                    <div className="relative">
                        <button
                            onClick={() => { setShowEffortPicker(!showEffortPicker); setShowModelPicker(false); }}
                            className="flex items-center gap-1 px-1 py-1 hover:bg-muted/50 transition-colors group"
                        >
                            <span className="text-xs font-medium text-muted-foreground group-hover:text-foreground">{EFFORT_LABELS[currentEffort].short}</span>
                            <ChevronDown className="w-2.5 h-2.5 text-muted-foreground/60" />
                        </button>

                        {/* Effort Dropdown */}
                        {showEffortPicker && (
                            <div className="absolute bottom-full left-0 mb-1 w-40 bg-white dark:bg-zinc-900 border border-border rounded-md shadow-xl z-50 overflow-hidden">
                                {availableEfforts.map((effort) => (
                                    <button
                                        key={effort}
                                        onClick={() => handleEffortSelect(effort)}
                                        className={`w-full text-left px-2 py-1.5 hover:bg-accent transition-colors ${effort === currentEffort ? 'bg-accent' : ''
                                            }`}
                                    >
                                        <div className="flex items-center justify-between">
                                            <span className="text-[10px] font-medium text-foreground">{EFFORT_LABELS[effort].short}</span>
                                            {effort === 'medium' && (
                                                <span className="text-[8px] px-1 py-0.5 bg-primary/20 text-primary">default</span>
                                            )}
                                        </div>
                                        <div className="text-[9px] text-muted-foreground">{EFFORT_LABELS[effort].description}</div>
                                    </button>
                                ))}
                            </div>
                        )}
                    </div>

                    {/* Plan Mode */}
                    <button
                        onClick={onTogglePlanMode}
                        className="flex items-center gap-1.5 text-[11px] text-muted-foreground hover:text-foreground transition-colors"
                    >
                        <Zap className={`w-3.5 h-3.5 ${settings.planMode ? 'text-foreground' : 'text-muted-foreground/70'}`} />
                        <span>plan: {settings.planMode ? 'on' : 'off'}</span>
                    </button>

                    {/* Approval Mode */}
                    <button
                        onClick={onCycleApproval}
                        className="flex items-center gap-1.5 text-[11px] text-muted-foreground hover:text-foreground transition-colors"
                    >
                        <Shield className={`w-3.5 h-3.5 ${settings.approvalMode === 'full-access' ? 'text-yellow-500' : 'text-muted-foreground/70'}`} />
                        <span>{settings.approvalMode === 'read-only' ? 'read only' : settings.approvalMode === 'auto' ? 'agent' : 'full access'}</span>
                    </button>

                    {/* Memory */}
                    <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                        <Brain className="w-3.5 h-3.5 text-muted-foreground/70" />
                        <span>memory: active</span>
                    </div>
                </div>

                {/* Right: Typing indicator + Send/Cancel */}
                <div className="flex items-center gap-2">
                    {/* Typing indicator */}
                    {isLoading && (
                        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                            <span className="flex gap-0.5">
                                <span className="w-1 h-1 bg-foreground/60 rounded-full animate-bounce" style={{ animationDelay: '0ms' }} />
                                <span className="w-1 h-1 bg-foreground/60 rounded-full animate-bounce" style={{ animationDelay: '150ms' }} />
                                <span className="w-1 h-1 bg-foreground/60 rounded-full animate-bounce" style={{ animationDelay: '300ms' }} />
                            </span>
                        </div>
                    )}

                    {/* Send or Cancel button */}
                    {isLoading ? (
                        <button
                            onClick={onCancel}
                            className="p-2 transition-all flex items-center justify-center rounded bg-red-500 text-white hover:bg-red-600"
                            title="Stop generation"
                        >
                            <Square className="w-3.5 h-3.5" fill="currentColor" />
                        </button>
                    ) : (
                        <button
                            onClick={handleSubmit}
                            disabled={!input.trim()}
                            className={`p-2 transition-all flex items-center justify-center rounded ${input.trim()
                                ? 'bg-foreground text-background hover:opacity-90'
                                : 'bg-muted text-muted-foreground/50 cursor-not-allowed'
                                }`}
                        >
                            <ArrowUp className="w-4 h-4" />
                        </button>
                    )}
                </div>
            </div>
        </div>
    );
}
