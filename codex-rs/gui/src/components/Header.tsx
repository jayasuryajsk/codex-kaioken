import {
  Settings,
  ChevronDown,
} from "lucide-react";
import { useState, useEffect } from "react";
import { AppSettings, ThemeName, THEME_INFO } from "../types";
import { getRateLimits, RateLimitsResponse } from "../tauri-api";

interface HeaderProps {
  settings: AppSettings;
  onOpenSettings: () => void;
  onChangeTheme: (theme: ThemeName) => void;
}

const THEMES = Object.entries(THEME_INFO) as [
  ThemeName,
  (typeof THEME_INFO)[ThemeName],
][];

export function Header({
  settings,
  onOpenSettings,
  onChangeTheme,
}: HeaderProps) {
  const [showThemePicker, setShowThemePicker] = useState(false);
  const [rateLimits, setRateLimits] = useState<RateLimitsResponse | null>(null);
  const currentTheme = THEME_INFO[settings.theme];

  // Fetch rate limits on mount and periodically
  useEffect(() => {
    const fetchLimits = () => {
      getRateLimits()
        .then(setRateLimits)
        .catch((err) => console.error('Failed to fetch rate limits:', err));
    };
    fetchLimits();
    const interval = setInterval(fetchLimits, 60000); // Refresh every minute
    return () => clearInterval(interval);
  }, []);

  // Calculate remaining percentages
  const primaryRemaining = rateLimits?.primary ? 100 - rateLimits.primary.usedPercent : null;
  const secondaryRemaining = rateLimits?.secondary ? 100 - rateLimits.secondary.usedPercent : null;

  return (
    <header
      data-tauri-drag-region
      className="flex items-center justify-between px-3 py-2 border-b border-border bg-muted/30 flex-shrink-0 cursor-default"
    >
      {/* Left: Name */}
      <span className="text-sm font-medium text-foreground">kaioken</span>

      {/* Right: Context + Theme + Settings */}
      <div className="flex items-center gap-3">
        {/* Rate Limits */}
        {(primaryRemaining !== null || secondaryRemaining !== null) && (
          <div className="flex items-center gap-2 text-xs tabular-nums">
            {primaryRemaining !== null && (
              <span
                className={primaryRemaining < 20 ? 'text-red-500' : primaryRemaining < 50 ? 'text-amber-500' : 'text-muted-foreground'}
                title="5-hour rate limit remaining"
              >
                5h: {Math.round(primaryRemaining)}%
              </span>
            )}
            {secondaryRemaining !== null && (
              <span
                className={secondaryRemaining < 20 ? 'text-red-500' : secondaryRemaining < 50 ? 'text-amber-500' : 'text-muted-foreground'}
                title="Weekly rate limit remaining"
              >
                wk: {Math.round(secondaryRemaining)}%
              </span>
            )}
          </div>
        )}

        {/* Theme Picker */}
        <div className="relative">
          <button
            onClick={() => setShowThemePicker(!showThemePicker)}
            className="flex items-center gap-1 px-2 py-1 rounded hover:bg-muted transition-colors text-muted-foreground hover:text-foreground"
            title="Change theme"
          >
            <span className="text-xs">{currentTheme.name}</span>
            <ChevronDown className="w-3 h-3" />
          </button>

          {/* Theme Dropdown */}
          {showThemePicker && (
            <>
              <div
                className="fixed inset-0 z-40"
                onClick={() => setShowThemePicker(false)}
              />
              <div className="absolute top-full right-0 mt-1 w-40 bg-white dark:bg-zinc-900 border border-border rounded-md shadow-xl z-50 overflow-hidden">
                {/* Light Themes */}
                <div className="px-2 py-1.5 text-[10px] font-medium text-muted-foreground uppercase tracking-wide border-b border-border/50">
                  Light
                </div>
                {THEMES.filter(([_, info]) => !info.isDark).map(
                  ([id, info]) => (
                    <button
                      key={id}
                      onClick={() => {
                        onChangeTheme(id);
                        setShowThemePicker(false);
                      }}
                      className={`w-full flex items-center gap-2 px-2 py-1.5 hover:bg-accent transition-colors ${
                        settings.theme === id ? "bg-accent" : ""
                      }`}
                    >
                      <div
                        className="w-4 h-4 rounded-full border border-border/50"
                        style={{ backgroundColor: info.color }}
                      />
                      <span className="text-xs text-foreground">
                        {info.name}
                      </span>
                    </button>
                  ),
                )}

                {/* Dark Themes */}
                <div className="px-2 py-1.5 text-[10px] font-medium text-muted-foreground uppercase tracking-wide border-y border-border/50">
                  Dark
                </div>
                {THEMES.filter(([_, info]) => info.isDark).map(([id, info]) => (
                  <button
                    key={id}
                    onClick={() => {
                      onChangeTheme(id);
                      setShowThemePicker(false);
                    }}
                    className={`w-full flex items-center gap-2 px-2 py-1.5 hover:bg-accent transition-colors ${
                      settings.theme === id ? "bg-accent" : ""
                    }`}
                  >
                    <div
                      className="w-4 h-4 rounded-full border border-border/50"
                      style={{ backgroundColor: info.color }}
                    />
                    <span className="text-xs text-foreground">{info.name}</span>
                  </button>
                ))}
              </div>
            </>
          )}
        </div>

        {/* Settings */}
        <button
          onClick={onOpenSettings}
          className="p-1.5 rounded hover:bg-muted transition-colors text-muted-foreground hover:text-foreground"
          title="Settings"
        >
          <Settings className="w-4 h-4" />
        </button>
      </div>
    </header>
  );
}
