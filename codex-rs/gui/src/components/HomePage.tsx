import { FolderOpen, Plus, X } from 'lucide-react';
import { useState, useEffect } from 'react';
import { getRateLimits, RateLimitsResponse, RateLimitWindow } from '../tauri-api';

// Official GitHub mark SVG
function GitHubIcon({ className }: { className?: string }) {
    return (
        <svg className={className} viewBox="0 0 24 24" fill="currentColor">
            <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z"/>
        </svg>
    );
}

interface HomePageProps {
    onOpenFolder: () => void;
    onCloneRepo?: (url: string) => Promise<void | 'cancelled'>;
}

// Loading spinner component
function Spinner({ className }: { className?: string }) {
    return (
        <svg className={`animate-spin ${className}`} viewBox="0 0 24 24" fill="none">
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
        </svg>
    );
}

// Format reset time from timestamp
function formatResetTime(resetsAt: number | null): string {
    if (!resetsAt) return '';
    const now = Date.now() / 1000;
    const diff = resetsAt - now;
    if (diff <= 0) return 'now';
    if (diff < 3600) return `${Math.ceil(diff / 60)}m`;
    if (diff < 86400) return `${Math.ceil(diff / 3600)}h`;
    return `${Math.ceil(diff / 86400)}d`;
}

// Detailed rate limit card component
function RateLimitCard({ label, description, window }: { label: string; description: string; window: RateLimitWindow }) {
    const percentRemaining = 100 - window.usedPercent;
    const resetTime = formatResetTime(window.resetsAt);

    // Color based on remaining percentage
    const barColor = percentRemaining > 50
        ? 'bg-emerald-500'
        : percentRemaining > 20
            ? 'bg-amber-500'
            : 'bg-red-500';

    const textColor = percentRemaining > 50
        ? 'text-emerald-600 dark:text-emerald-400'
        : percentRemaining > 20
            ? 'text-amber-600 dark:text-amber-400'
            : 'text-red-600 dark:text-red-400';

    const statusText = percentRemaining > 50
        ? 'Healthy'
        : percentRemaining > 20
            ? 'Limited'
            : 'Low';

    return (
        <div className="flex-1 bg-white dark:bg-card rounded-xl border border-border p-4">
            <div className="flex items-center justify-between mb-3">
                <div>
                    <h4 className="text-sm font-medium text-foreground">{label}</h4>
                    <p className="text-xs text-muted-foreground">{description}</p>
                </div>
                <span className={`text-2xl font-semibold tabular-nums ${textColor}`}>
                    {Math.round(percentRemaining)}%
                </span>
            </div>
            <div className="w-full h-2 bg-muted rounded-full overflow-hidden mb-2">
                <div
                    className={`h-full ${barColor} transition-all duration-300`}
                    style={{ width: `${percentRemaining}%` }}
                />
            </div>
            <div className="flex justify-between text-xs text-muted-foreground">
                <span className={textColor}>{statusText}</span>
                {resetTime && <span>Resets in {resetTime}</span>}
            </div>
        </div>
    );
}

export function HomePage({ onOpenFolder, onCloneRepo }: HomePageProps) {
    const [rateLimits, setRateLimits] = useState<RateLimitsResponse | null>(null);
    const [rateLimitsError, setRateLimitsError] = useState<string | null>(null);
    const [cloneModalOpen, setCloneModalOpen] = useState(false);
    const [cloneUrl, setCloneUrl] = useState('');
    const [isCloning, setIsCloning] = useState(false);
    const [cloneError, setCloneError] = useState<string | null>(null);

    const handleClone = async () => {
        if (!cloneUrl.trim() || !onCloneRepo) return;

        setIsCloning(true);
        setCloneError(null);

        try {
            const result = await onCloneRepo(cloneUrl.trim());

            if (result === 'cancelled') {
                // User cancelled folder picker - just stop loading, don't show error
                setIsCloning(false);
                return;
            }

            // Success - modal will close automatically as app navigates to workspace
            setCloneModalOpen(false);
            setCloneUrl('');
        } catch (err) {
            // Extract meaningful error message
            let errorMessage = 'Failed to clone repository';
            if (err instanceof Error) {
                errorMessage = err.message;
            } else if (typeof err === 'string') {
                errorMessage = err;
            } else if (err && typeof err === 'object' && 'message' in err) {
                errorMessage = String((err as { message: unknown }).message);
            }
            setCloneError(errorMessage);
        } finally {
            setIsCloning(false);
        }
    };

    // Fetch rate limits on mount
    useEffect(() => {
        getRateLimits()
            .then(setRateLimits)
            .catch((err) => {
                console.error('Failed to fetch rate limits:', err);
                setRateLimitsError(String(err));
            });
    }, []);

    return (
        <div className="flex flex-col h-screen bg-background text-foreground overflow-hidden font-sans select-none selection:bg-primary/10 selection:text-primary">
            {/* Top Bar (Draggable) */}
            <div data-tauri-drag-region className="h-12 flex items-center px-6 flex-shrink-0 z-20 bg-background border-b border-border/40">
                <div className="w-6 h-6 rounded-lg bg-foreground flex items-center justify-center">
                    <span className="text-background font-bold text-xs">K</span>
                </div>
            </div>

            {/* Main Content - Centered */}
            <div className="flex-1 flex flex-col items-center justify-center bg-[#FAFAFA] dark:bg-background px-6 py-8">
                <div className="w-full max-w-2xl animate-in fade-in slide-in-from-bottom-2 duration-300">

                    {/* Hero Header - Centered */}
                    <h1 className="text-4xl md:text-5xl font-medium tracking-tight text-foreground text-center mb-12">
                        CODEX <span className="text-primary italic font-serif">KAIOKEN</span>
                    </h1>

                    {/* Actions Grid - Centered */}
                    <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-10">
                        {/* New Project */}
                        <button
                            onClick={onOpenFolder}
                            className="group relative flex flex-col items-center text-center p-6 h-36 bg-white dark:bg-card rounded-xl border border-border shadow-sm hover:shadow-md hover:-translate-y-0.5 transition-all duration-200"
                        >
                            <div className="w-12 h-12 rounded-full bg-muted flex items-center justify-center mb-3 group-hover:scale-110 transition-transform duration-200">
                                <Plus className="w-5 h-5 text-foreground" />
                            </div>
                            <span className="block font-medium text-foreground mb-1">New Project</span>
                            <span className="text-xs text-muted-foreground">Create workspace</span>
                        </button>

                        {/* Open Folder */}
                        <button
                            onClick={onOpenFolder}
                            className="group relative flex flex-col items-center text-center p-6 h-36 bg-white dark:bg-card rounded-xl border border-border shadow-sm hover:shadow-md hover:-translate-y-0.5 transition-all duration-200"
                        >
                            <div className="w-12 h-12 rounded-full bg-muted flex items-center justify-center mb-3 group-hover:scale-110 transition-transform duration-200">
                                <FolderOpen className="w-5 h-5 text-foreground" />
                            </div>
                            <span className="block font-medium text-foreground mb-1">Open Folder</span>
                            <span className="text-xs text-muted-foreground">Local repository</span>
                        </button>

                        {/* Clone Repo */}
                        <button
                            onClick={() => setCloneModalOpen(true)}
                            className="group relative flex flex-col items-center text-center p-6 h-36 bg-white dark:bg-card rounded-xl border border-border shadow-sm hover:shadow-md hover:-translate-y-0.5 transition-all duration-200"
                        >
                            <div className="w-12 h-12 rounded-full bg-muted flex items-center justify-center mb-3 group-hover:scale-110 transition-transform duration-200">
                                <GitHubIcon className="w-5 h-5 text-foreground" />
                            </div>
                            <span className="block font-medium text-foreground mb-1">Clone Repo</span>
                            <span className="text-xs text-muted-foreground">From URL</span>
                        </button>
                    </div>

                    {/* Rate Limits Section - Below Cards */}
                    {rateLimits && (rateLimits.primary || rateLimits.secondary) && (
                        <div className="flex gap-4">
                            {rateLimits.primary && (
                                <RateLimitCard
                                    label="5-Hour Limit"
                                    description="Rolling window"
                                    window={rateLimits.primary}
                                />
                            )}
                            {rateLimits.secondary && (
                                <RateLimitCard
                                    label="Weekly Limit"
                                    description="Resets weekly"
                                    window={rateLimits.secondary}
                                />
                            )}
                        </div>
                    )}

                    {/* Error state for rate limits */}
                    {rateLimitsError && (
                        <div className="text-xs text-muted-foreground/50 bg-muted/30 rounded-lg px-4 py-3 text-center">
                            Rate limits unavailable
                        </div>
                    )}
                </div>
            </div>

            {/* Clone Repository Modal */}
            {cloneModalOpen && (
                <div className="fixed inset-0 z-50 flex items-center justify-center">
                    <div
                        className="absolute inset-0 bg-black/50 backdrop-blur-sm"
                        onClick={() => !isCloning && setCloneModalOpen(false)}
                    />
                    <div className="relative bg-white dark:bg-card rounded-xl border border-border shadow-xl w-full max-w-md mx-4 p-6 animate-in fade-in zoom-in-95 duration-200">
                        {isCloning ? (
                            // Cloning in progress view
                            <div className="flex flex-col items-center py-8">
                                <div className="w-16 h-16 rounded-full bg-muted/50 flex items-center justify-center mb-6">
                                    <Spinner className="w-8 h-8 text-foreground" />
                                </div>
                                <h3 className="text-lg font-medium text-foreground mb-2">Cloning Repository</h3>
                                <p className="text-sm text-muted-foreground text-center max-w-xs">
                                    {cloneUrl.split('/').pop()?.replace('.git', '') || 'repository'}
                                </p>
                                <div className="mt-4 flex items-center gap-2 text-xs text-muted-foreground/60">
                                    <div className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse" />
                                    <span>This may take a moment...</span>
                                </div>
                            </div>
                        ) : (
                            // URL input view
                            <>
                                <div className="flex items-center justify-between mb-4">
                                    <h3 className="text-lg font-medium text-foreground">Clone Repository</h3>
                                    <button
                                        onClick={() => {
                                            setCloneModalOpen(false);
                                            setCloneError(null);
                                        }}
                                        className="p-1 rounded-md hover:bg-muted transition-colors"
                                    >
                                        <X className="w-4 h-4 text-muted-foreground" />
                                    </button>
                                </div>
                                <div className="space-y-4">
                                    <div>
                                        <label className="block text-sm font-medium text-foreground mb-1.5">
                                            Repository URL
                                        </label>
                                        <input
                                            type="text"
                                            value={cloneUrl}
                                            onChange={(e) => {
                                                setCloneUrl(e.target.value);
                                                setCloneError(null);
                                            }}
                                            onKeyDown={(e) => {
                                                if (e.key === 'Enter' && cloneUrl.trim()) {
                                                    handleClone();
                                                }
                                            }}
                                            placeholder="https://github.com/user/repo.git"
                                            className={`w-full h-10 px-3 text-sm bg-background border rounded-lg focus:ring-1 outline-none transition-all ${
                                                cloneError
                                                    ? 'border-red-500 focus:border-red-500 focus:ring-red-500/20'
                                                    : 'border-border focus:border-primary/50 focus:ring-primary/20'
                                            }`}
                                            autoFocus
                                        />
                                        {cloneError && (
                                            <p className="mt-2 text-sm text-red-500 flex items-start gap-1.5">
                                                <span className="shrink-0 mt-0.5">✕</span>
                                                <span>{cloneError}</span>
                                            </p>
                                        )}
                                    </div>
                                    <div className="flex gap-2 justify-end">
                                        <button
                                            onClick={() => {
                                                setCloneModalOpen(false);
                                                setCloneError(null);
                                            }}
                                            className="px-4 py-2 text-sm font-medium text-muted-foreground hover:text-foreground transition-colors"
                                        >
                                            Cancel
                                        </button>
                                        <button
                                            onClick={handleClone}
                                            disabled={!cloneUrl.trim()}
                                            className="px-4 py-2 text-sm font-medium bg-foreground text-background rounded-lg hover:bg-foreground/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                                        >
                                            Clone
                                        </button>
                                    </div>
                                </div>
                            </>
                        )}
                    </div>
                </div>
            )}
        </div>
    );
}
