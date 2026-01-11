import { useState } from 'react';
import { ChevronLeft } from 'lucide-react';
import {
    AppSettings,
    ThemeName,
    THEME_INFO,
    ReasoningEffort,
    PlanDetail,
    SendKey,
} from '../types';

interface SettingsViewProps {
    settings: AppSettings;
    onClose: () => void;
    onSettingsChange: (updates: Partial<AppSettings>) => void;
}

type SettingsCategory = 'chat' | 'approvals' | 'performance' | 'appearance' | 'notifications' | 'experimental' | 'about';

const CATEGORIES: { id: SettingsCategory; label: string }[] = [
    { id: 'chat', label: 'Chat' },
    { id: 'approvals', label: 'Approvals' },
    { id: 'performance', label: 'Performance' },
    { id: 'appearance', label: 'Appearance' },
    { id: 'notifications', label: 'Notifications' },
    { id: 'experimental', label: 'Experimental' },
    { id: 'about', label: 'About' },
];

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

const PLAN_DETAILS: { id: PlanDetail; name: string; description: string }[] = [
    { id: 'auto', name: 'Auto', description: 'Let AI choose based on task complexity' },
    { id: 'coarse', name: 'Coarse', description: '3-4 high-level steps for quick tasks' },
    { id: 'detailed', name: 'Detailed', description: '6-10 steps with file references' },
];

const EXPERIMENTAL_FEATURES = [
    { id: 'ghostCommits', name: 'Ghost Commits', description: 'Create ghost commits for undo support', defaultOn: true },
    { id: 'webSearch', name: 'Web Search', description: 'Allow the model to request web searches', defaultOn: false },
    { id: 'parallelTools', name: 'Parallel Tool Calls', description: 'Allow multiple tools in parallel', defaultOn: false },
    { id: 'unifiedExec', name: 'Unified Exec', description: 'Run commands in background terminals', defaultOn: false },
];

const THEMES = Object.entries(THEME_INFO) as [ThemeName, (typeof THEME_INFO)[ThemeName]][];

// Toggle Switch Component
function Toggle({ checked, onChange }: { checked: boolean; onChange: (checked: boolean) => void }) {
    return (
        <button
            onClick={() => onChange(!checked)}
            className={`relative w-10 h-6 rounded-full transition-colors ${
                checked ? 'bg-primary' : 'bg-muted-foreground/30'
            }`}
        >
            <div
                className={`absolute top-1 w-4 h-4 rounded-full bg-white transition-transform ${
                    checked ? 'translate-x-5' : 'translate-x-1'
                }`}
            />
        </button>
    );
}

// Setting Row Component
function SettingRow({
    label,
    description,
    children,
}: {
    label: string;
    description?: string;
    children: React.ReactNode;
}) {
    return (
        <div className="flex items-center justify-between py-4 border-b border-border last:border-0">
            <div className="flex-1 min-w-0 pr-4">
                <div className="text-sm font-medium text-foreground">{label}</div>
                {description && (
                    <div className="text-xs text-muted-foreground mt-0.5">{description}</div>
                )}
            </div>
            <div className="flex-shrink-0">{children}</div>
        </div>
    );
}

// Dropdown Component
function Dropdown<T extends string>({
    value,
    options,
    onChange,
}: {
    value: T;
    options: { id: T; name: string }[];
    onChange: (value: T) => void;
}) {
    return (
        <select
            value={value}
            onChange={(e) => onChange(e.target.value as T)}
            className="px-3 py-1.5 text-sm bg-muted border border-border rounded-md text-foreground focus:outline-none focus:ring-2 focus:ring-primary/50"
        >
            {options.map((opt) => (
                <option key={opt.id} value={opt.id}>
                    {opt.name}
                </option>
            ))}
        </select>
    );
}

export function SettingsView({ settings, onClose, onSettingsChange }: SettingsViewProps) {
    const [activeCategory, setActiveCategory] = useState<SettingsCategory>('chat');

    const renderChatSettings = () => (
        <div>
            <h2 className="text-lg font-semibold text-foreground mb-1">Chat</h2>
            <p className="text-sm text-muted-foreground mb-6">Configure default chat behavior</p>

            <SettingRow label="Default Model" description="Model for new chats">
                <Dropdown
                    value={settings.model}
                    options={MODELS.map((m) => ({ id: m.id, name: m.name }))}
                    onChange={(model) => onSettingsChange({ model })}
                />
            </SettingRow>

            <SettingRow label="Reasoning Effort" description="Default reasoning depth">
                <Dropdown
                    value={settings.reasoningEffort}
                    options={REASONING_EFFORTS.map((e) => ({ id: e.id, name: e.name }))}
                    onChange={(reasoningEffort) => onSettingsChange({ reasoningEffort })}
                />
            </SettingRow>

            <SettingRow label="Default to Plan Mode" description="Start new chats in plan mode">
                <Toggle
                    checked={settings.planMode}
                    onChange={(planMode) => onSettingsChange({ planMode })}
                />
            </SettingRow>

            <SettingRow label="Plan Detail" description="Level of detail in generated plans">
                <Dropdown
                    value={settings.planDetail}
                    options={PLAN_DETAILS.map((p) => ({ id: p.id, name: p.name }))}
                    onChange={(planDetail) => onSettingsChange({ planDetail })}
                />
            </SettingRow>

            <SettingRow label="Send Messages With" description="Key to send messages">
                <Dropdown
                    value={settings.sendKey}
                    options={[
                        { id: 'enter' as SendKey, name: 'Enter' },
                        { id: 'cmd-enter' as SendKey, name: 'Cmd+Enter' },
                    ]}
                    onChange={(sendKey) => onSettingsChange({ sendKey })}
                />
            </SettingRow>
        </div>
    );

    const renderApprovalSettings = () => (
        <div>
            <h2 className="text-lg font-semibold text-foreground mb-1">Approvals</h2>
            <p className="text-sm text-muted-foreground mb-6">Control what actions require your approval</p>

            <div className="space-y-2">
                {[
                    { id: 'read-only' as const, name: 'Read Only', description: 'Requires approval to edit files and run commands' },
                    { id: 'auto' as const, name: 'Agent', description: 'Read and edit files, run commands automatically' },
                    { id: 'full-access' as const, name: 'Full Access', description: 'Edit files outside workspace, network access' },
                ].map((mode) => (
                    <button
                        key={mode.id}
                        onClick={() => onSettingsChange({ approvalMode: mode.id })}
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
        </div>
    );

    const renderPerformanceSettings = () => (
        <div>
            <h2 className="text-lg font-semibold text-foreground mb-1">Performance</h2>
            <p className="text-sm text-muted-foreground mb-6">Configure system resource usage</p>

            <SettingRow
                label="Subagent Concurrency"
                description={`Run ${settings.subagentConcurrency} helper tasks in parallel`}
            >
                <div className="flex items-center gap-3">
                    <input
                        type="range"
                        min={1}
                        max={8}
                        value={settings.subagentConcurrency}
                        onChange={(e) => onSettingsChange({ subagentConcurrency: parseInt(e.target.value) })}
                        className="w-24"
                    />
                    <span className="text-sm text-foreground w-4">{settings.subagentConcurrency}</span>
                </div>
            </SettingRow>

            <SettingRow label="Animations" description="Enable UI animations and effects">
                <Toggle
                    checked={settings.animations}
                    onChange={(animations) => onSettingsChange({ animations })}
                />
            </SettingRow>
        </div>
    );

    const renderAppearanceSettings = () => (
        <div>
            <h2 className="text-lg font-semibold text-foreground mb-1">Appearance</h2>
            <p className="text-sm text-muted-foreground mb-6">Customize the look and feel</p>

            <div className="space-y-3 mb-6">
                <label className="text-sm font-medium text-foreground">Theme</label>
                <div className="grid grid-cols-3 gap-2">
                    {THEMES.map(([id, info]) => (
                        <button
                            key={id}
                            onClick={() => onSettingsChange({ theme: id })}
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
            </div>

            <SettingRow label="Show Rate Limits" description="Display rate limit usage in footer">
                <Toggle
                    checked={settings.showRateLimits}
                    onChange={(showRateLimits) => onSettingsChange({ showRateLimits })}
                />
            </SettingRow>

            <SettingRow label="Show Token Cost" description="Display token cost in the UI">
                <Toggle
                    checked={settings.showTokenCost}
                    onChange={(showTokenCost) => onSettingsChange({ showTokenCost })}
                />
            </SettingRow>
        </div>
    );

    const renderNotificationSettings = () => (
        <div>
            <h2 className="text-lg font-semibold text-foreground mb-1">Notifications</h2>
            <p className="text-sm text-muted-foreground mb-6">Configure alerts and sounds</p>

            <SettingRow label="Desktop Notifications" description="Get notified when AI finishes working">
                <Toggle
                    checked={settings.desktopNotifications}
                    onChange={(desktopNotifications) => onSettingsChange({ desktopNotifications })}
                />
            </SettingRow>

            <SettingRow label="Sound Effects" description="Play a sound when AI finishes">
                <Toggle
                    checked={settings.soundEffects}
                    onChange={(soundEffects) => onSettingsChange({ soundEffects })}
                />
            </SettingRow>
        </div>
    );

    const renderExperimentalSettings = () => (
        <div>
            <h2 className="text-lg font-semibold text-foreground mb-1">Experimental</h2>
            <p className="text-sm text-muted-foreground mb-6">Toggle beta and experimental features</p>

            {EXPERIMENTAL_FEATURES.map((feature) => (
                <SettingRow key={feature.id} label={feature.name} description={feature.description}>
                    <Toggle
                        checked={settings.experimentalFeatures[feature.id] ?? feature.defaultOn}
                        onChange={(enabled) =>
                            onSettingsChange({
                                experimentalFeatures: {
                                    ...settings.experimentalFeatures,
                                    [feature.id]: enabled,
                                },
                            })
                        }
                    />
                </SettingRow>
            ))}
        </div>
    );

    const renderAboutSettings = () => (
        <div>
            <h2 className="text-lg font-semibold text-foreground mb-1">About</h2>
            <p className="text-sm text-muted-foreground mb-6">Application information</p>

            <SettingRow label="Version">
                <span className="text-sm text-foreground">0.1.0</span>
            </SettingRow>

            <SettingRow label="Build">
                <span className="text-sm text-foreground">Development</span>
            </SettingRow>

            <div className="mt-6 space-y-2">
                <a
                    href="https://github.com/anthropics/kaioken"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="block text-sm text-primary hover:underline"
                >
                    View on GitHub
                </a>
                <a
                    href="https://kaioken.dev/docs"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="block text-sm text-primary hover:underline"
                >
                    Documentation
                </a>
            </div>
        </div>
    );

    const renderContent = () => {
        switch (activeCategory) {
            case 'chat':
                return renderChatSettings();
            case 'approvals':
                return renderApprovalSettings();
            case 'performance':
                return renderPerformanceSettings();
            case 'appearance':
                return renderAppearanceSettings();
            case 'notifications':
                return renderNotificationSettings();
            case 'experimental':
                return renderExperimentalSettings();
            case 'about':
                return renderAboutSettings();
        }
    };

    return (
        <div className="fixed inset-0 bg-background z-50 flex flex-col">
            {/* Header with back button and tabs */}
            <div className="border-b border-border flex-shrink-0">
                {/* Top bar */}
                <div className="flex items-center gap-3 px-4 py-2">
                    <button
                        onClick={onClose}
                        className="p-1.5 text-muted-foreground hover:text-foreground rounded hover:bg-muted transition-colors"
                    >
                        <ChevronLeft className="w-5 h-5" />
                    </button>
                    <h1 className="text-base font-semibold text-foreground">Settings</h1>
                </div>

                {/* Horizontal Tabs */}
                <div className="flex items-center gap-1 px-4 overflow-x-auto">
                    {CATEGORIES.map((cat) => (
                        <button
                            key={cat.id}
                            onClick={() => setActiveCategory(cat.id)}
                            className={`px-3 py-2 text-sm whitespace-nowrap transition-colors border-b-2 -mb-px ${
                                activeCategory === cat.id
                                    ? 'border-primary text-foreground font-medium'
                                    : 'border-transparent text-muted-foreground hover:text-foreground'
                            }`}
                        >
                            {cat.label}
                        </button>
                    ))}
                </div>
            </div>

            {/* Content */}
            <div className="flex-1 overflow-y-auto">
                <div className="max-w-2xl mx-auto p-6">{renderContent()}</div>
            </div>
        </div>
    );
}
