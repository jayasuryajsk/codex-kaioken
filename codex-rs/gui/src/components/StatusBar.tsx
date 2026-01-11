import { TokenUsage } from '../types';

interface StatusBarProps {
    tokenUsage?: TokenUsage;
}

export function StatusBar({ tokenUsage }: StatusBarProps) {
    return (
        <div className="flex items-center justify-end px-6 py-1.5 border-t border-border/40 bg-background text-[11px] text-muted-foreground/70 font-medium select-none">
            {/* Tokens */}
            {tokenUsage && (
                <span className="tabular-nums">
                    tokens: <span className="text-muted-foreground">{tokenUsage.total.toLocaleString()}</span>
                </span>
            )}
        </div>
    );
}
