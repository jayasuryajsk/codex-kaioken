import { ChevronLeft } from 'lucide-react';
import { AppSettings, ThemeName, THEME_INFO, ReasoningEffort } from '../types';

interface SettingsModalProps {
    settings: AppSettings;
    onClose: () => void;
    onChangeTheme: (theme: ThemeName) => void;
    onModelChange?: (model: string) => void;
    onReasoningEffortChange?: (effort: ReasoningEffort) => void;
    onApprovalModeChange?: (mode: 'read-only' | 'auto' | 'full-access') => void;
}

const THEMES = Object.entries(THEME_INFO) as [ThemeName, (typeof THEME_INFO)[ThemeName]][];

const MODELS = [
    { id: 'gpt-5.2-codex', name: 'gpt-5.2-codex', description: 'Latest frontier agentic coding model', isDefault: true },
    { id: 'gpt-5.1-codex-max', name: 'gpt-5.1-codex-max', description: 'Deep and fast reasoning flagship' },
    { id: 'gpt-5.1-codex-mini', name: 'gpt-5.1-codex-mini', description: 'Cheaper, faster, less capable' },
    { id: 'gpt-5.2', name: 'gpt-5.2', description: 'Latest frontier model' },
];

const REASONING_EFFORTS: { id: ReasoningEffort; name: string; description: string }[] = [
    { id: 'low', name: 'Low', description: 'Fast responses with lighter reasoning' },
    { id: 'medium', name: 'Medium', description: 'Balances speed and reasoning depth' },
    { id: 'high', name: 'High', description: 'Maximizes reasoning depth' },
    { id: 'xhigh', name: 'Max', description: 'Extra high for complex problems' },
];

const APPROVAL_MODES = [
    { id: 'read-only' as const, name: 'Read Only', description: 'Requires approval to edit files and run commands' },
    { id: 'auto' as const, name: 'Agent', description: 'Read and edit files, run commands automatically' },
    { id: 'full-access' as const, name: 'Full Access', description: 'Edit files outside workspace, network access' },
];

export function SettingsModal({
    settings,
    onClose,
    onChangeTheme,
    onModelChange,
    onReasoningEffortChange,
    onApprovalModeChange,
}: SettingsModalProps) {
    return (
        <div className="fixed inset-0 bg-background z-50 flex flex-col">
            {/* Header */}
            <div className="flex items-center gap-3 px-4 py-3 border-b border-border flex-shrink-0">
                <button
                    onClick={onClose}
                    className="p-1.5 text-muted-foreground hover:text-foreground rounded hover:bg-muted transition-colors"
                >
                    <ChevronLeft className="w-5 h-5" />
                </button>
                <h1 className="text-lg font-semibold text-foreground">Settings</h1>
            </div>

            {/* Content */}
            <div className="flex-1 overflow-y-auto">
                <div className="max-w-2xl mx-auto p-6 space-y-8">
                    {/* Model Selection */}
                    <section className="space-y-3">
                        <h2 className="text-sm font-semibold text-foreground">Model</h2>
                        <p className="text-xs text-muted-foreground">Select the AI model for your sessions</p>
                        <div className="space-y-2">
                            {MODELS.map((model) => (
                                <button
                                    key={model.id}
                                    onClick={() => onModelChange?.(model.id)}
                                    className={`w-full flex items-center justify-between px-4 py-3 rounded-lg border transition-colors text-left ${
                                        settings.model === model.id
                                            ? 'border-primary bg-primary/5'
                                            : 'border-border hover:bg-muted/50'
                                    }`}
                                >
                                    <div>
                                        <div className="text-sm font-medium text-foreground">{model.name}</div>
                                        <div className="text-xs text-muted-foreground mt-0.5">{model.description}</div>
                                    </div>
                                    {model.isDefault && (
                                        <span className="text-[10px] px-2 py-0.5 bg-primary/20 text-primary rounded">default</span>
                                    )}
                                </button>
                            ))}
                        </div>
                    </section>

                    {/* Reasoning Effort */}
                    <section className="space-y-3">
                        <h2 className="text-sm font-semibold text-foreground">Reasoning Effort</h2>
                        <p className="text-xs text-muted-foreground">
                            {REASONING_EFFORTS.find(e => e.id === settings.reasoningEffort)?.description}
                        </p>
                        <div className="grid grid-cols-4 gap-2">
                            {REASONING_EFFORTS.map((effort) => (
                                <button
                                    key={effort.id}
                                    onClick={() => onReasoningEffortChange?.(effort.id)}
                                    className={`px-3 py-2.5 rounded-lg border transition-colors ${
                                        settings.reasoningEffort === effort.id
                                            ? 'border-primary bg-primary/5 text-foreground'
                                            : 'border-border hover:bg-muted/50 text-muted-foreground'
                                    }`}
                                >
                                    <div className="text-sm font-medium">{effort.name}</div>
                                </button>
                            ))}
                        </div>
                    </section>

                    {/* Approval Mode */}
                    <section className="space-y-3">
                        <h2 className="text-sm font-semibold text-foreground">Approval Mode</h2>
                        <p className="text-xs text-muted-foreground">Control what actions require your approval</p>
                        <div className="space-y-2">
                            {APPROVAL_MODES.map((mode) => (
                                <button
                                    key={mode.id}
                                    onClick={() => onApprovalModeChange?.(mode.id)}
                                    className={`w-full flex items-start gap-3 px-4 py-3 rounded-lg border transition-colors text-left ${
                                        settings.approvalMode === mode.id
                                            ? 'border-primary bg-primary/5'
                                            : 'border-border hover:bg-muted/50'
                                    }`}
                                >
                                    <div className={`w-4 h-4 rounded-full border-2 mt-0.5 flex-shrink-0 ${
                                        settings.approvalMode === mode.id
                                            ? 'border-primary bg-primary'
                                            : 'border-muted-foreground/50'
                                    }`} />
                                    <div>
                                        <div className="text-sm font-medium text-foreground">{mode.name}</div>
                                        <div className="text-xs text-muted-foreground mt-0.5">{mode.description}</div>
                                    </div>
                                </button>
                            ))}
                        </div>
                    </section>

                    {/* Theme */}
                    <section className="space-y-3">
                        <h2 className="text-sm font-semibold text-foreground">Theme</h2>
                        <p className="text-xs text-muted-foreground">Choose your preferred appearance</p>
                        <div className="grid grid-cols-3 gap-2">
                            {THEMES.map(([id, info]) => (
                                <button
                                    key={id}
                                    onClick={() => onChangeTheme(id)}
                                    className={`flex items-center gap-3 px-4 py-3 rounded-lg border transition-colors ${
                                        settings.theme === id
                                            ? 'border-primary bg-primary/5'
                                            : 'border-border hover:bg-muted/50'
                                    }`}
                                >
                                    <div
                                        className="w-5 h-5 rounded-full border border-border/50"
                                        style={{ backgroundColor: info.color }}
                                    />
                                    <span className="text-sm text-foreground">{info.name}</span>
                                </button>
                            ))}
                        </div>
                    </section>

                    {/* About */}
                    <section className="space-y-3 pt-6 border-t border-border">
                        <h2 className="text-sm font-semibold text-foreground">About</h2>
                        <div className="space-y-2">
                            <div className="flex justify-between py-2">
                                <span className="text-sm text-muted-foreground">Version</span>
                                <span className="text-sm text-foreground">0.1.0</span>
                            </div>
                            <div className="flex justify-between py-2">
                                <span className="text-sm text-muted-foreground">Build</span>
                                <span className="text-sm text-foreground">Development</span>
                            </div>
                        </div>
                    </section>
                </div>
            </div>
        </div>
    );
}
