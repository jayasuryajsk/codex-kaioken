# Upstream Features Implementation Plan

## Overview

This document outlines the plan to port missing features from OpenAI's upstream Codex to the Kaioken fork.

**Upstream Repo:** https://github.com/openai/codex
**Fork:** codex-kaioken

---

## Feature Categories

### 🟢 Can Port Directly (Copy with minor changes)
| Feature | Upstream Location | Effort | Notes |
|---------|-------------------|--------|-------|
| Skills System | `core/src/skills/` | 2-3 days | Full module exists upstream |
| TUI2 | `tui2/` | 3-5 days | Complete crate, may need dependency updates |
| Shell Snapshotting | Check `core/src/` | 1 day | Likely in session/state management |

### 🟡 Partial Port (Copy + Modify)
| Feature | Upstream Location | Effort | Notes |
|---------|-------------------|--------|-------|
| Vim Navigation | `tui/` or `tui2/` | 2 days | Need to find keybinding system |
| Splash Screen | `tui2/` frames/onboarding | 1 day | May be in onboarding module |
| `allowed_sandbox_modes` | `execpolicy/` or `core/config/` | 1 day | Config parsing addition |

### 🔴 Need Custom Implementation
| Feature | Reason | Effort | Notes |
|---------|--------|--------|-------|
| DMG Builds | CI/CD specific | 1 day | GitHub Actions workflow |
| Linux Sigstore Signing | CI/CD specific | 1 day | GitHub Actions workflow |

---

## Detailed Implementation Plan

### Phase 1: Skills System (Priority: HIGH)
**Timeline: 2-3 days**

#### Files to Port from Upstream
```
codex-rs/core/src/skills/
├── mod.rs           # Module exports
├── injection.rs     # Skill injection into prompts
├── loader.rs        # SKILL.md file loading
├── manager.rs       # SkillsManager with caching
├── model.rs         # SkillMetadata, SkillError types
├── render.rs        # Skill rendering for TUI
├── system.rs        # System/bundled skills
└── assets/samples/  # Sample skill files
```

#### Integration Points
1. **core/src/lib.rs** - Add `pub mod skills;`
2. **core/src/codex.rs** - Initialize SkillsManager
3. **core/src/config/mod.rs** - Add skills config options
4. **tui/src/chatwidget.rs** - Add `/skills` command
5. **tui/src/slash_command.rs** - Register Skills slash command

#### Steps
1. Copy `skills/` directory from upstream
2. Update imports/paths for Kaioken structure
3. Add SkillsManager to Codex struct
4. Wire up skill loading on session start
5. Add `/skills` slash command to list/toggle skills
6. Test with sample SKILL.md files

---

### Phase 2: TUI2 (Priority: MEDIUM)
**Timeline: 3-5 days**

#### Approach
TUI2 is a complete rewrite. Options:
1. **Full port** - Copy entire `tui2/` crate
2. **Feature extraction** - Port specific improvements to existing TUI

#### Recommendation: Feature Extraction
Port these specific improvements from TUI2 to existing TUI:
- Scroll normalization (`tui.scroll_*` config) ✅ DONE
- Improved transcript rendering
- Better vim-style keybindings

#### Files to Analyze
```
codex-rs/tui2/src/
├── app.rs                    # Compare with our app.rs
├── transcript_render.rs      # New rendering approach
├── transcript_selection.rs   # Selection improvements
├── app_backtrack.rs          # Undo improvements
└── key_hint.rs               # Keybinding system
```

#### Steps
1. Compare TUI vs TUI2 architectures
2. Identify specific improvements worth porting
3. Port incrementally without breaking existing TUI
4. Add feature flag to switch between TUI/TUI2 if needed

---

### Phase 3: Vim Navigation Mode (Priority: MEDIUM)
**Timeline: 2 days**

#### Current State
- hjkl keys work in lists/navigation
- No modal vim mode (normal/insert/visual)

#### Implementation
1. Add `VimMode` enum: `Normal`, `Insert`, `Visual`
2. Add mode state to ChatWidget
3. Handle mode transitions (Esc → Normal, i → Insert)
4. Update keybinding handling based on mode
5. Add mode indicator to status bar

#### Files to Modify
```
tui/src/chatwidget.rs      # Add vim_mode field
tui/src/update_prompt.rs   # Handle mode-specific keys
tui/src/bottom_pane/       # Mode indicator
```

---

### Phase 4: Splash Screen (Priority: LOW)
**Timeline: 1 day**

#### Check Upstream
Look in `tui2/src/onboarding/` or `tui2/src/frames/`

#### Implementation
1. Create `splash.rs` module
2. Add ASCII art or animated welcome
3. Show on startup before main TUI
4. Auto-dismiss after 1-2 seconds or on keypress

---

### Phase 5: Shell Snapshotting (Priority: LOW)
**Timeline: 1 day**

#### Current State
- Environment variables captured
- Missing: full shell state (aliases, functions, history position)

#### Implementation
1. Capture shell state before command execution
2. Store in session state
3. Allow restore on undo

---

### Phase 6: allowed_sandbox_modes (Priority: LOW)
**Timeline: 1 day**

#### Implementation
1. Add to `core/src/config/types.rs`:
   ```rust
   pub struct RequirementsToml {
       pub allowed_sandbox_modes: Option<Vec<SandboxMode>>,
   }
   ```
2. Parse `requirements.toml` in project root
3. Enforce constraints in sandbox selection

---

### Phase 7: CI/CD Features (Priority: LOW)
**Timeline: 1-2 days**

#### DMG Builds
1. Add `.github/workflows/build-dmg.yml`
2. Use `create-dmg` or similar tool
3. Sign with Apple Developer cert (if available)

#### Linux Sigstore Signing
1. Add sigstore action to release workflow
2. Sign Linux binaries with cosign

---

## Porting Checklist

### Before Porting Any Feature
- [ ] Check upstream commit history for the feature
- [ ] Identify all files involved
- [ ] Check for new dependencies
- [ ] Look for breaking API changes

### After Porting
- [ ] Update imports/module paths
- [ ] Run `cargo check`
- [ ] Run `cargo test`
- [ ] Test manually in TUI
- [ ] Update any snapshot tests

---

## Dependency Updates Needed

Check if upstream uses newer versions:
- [ ] `ratatui` version
- [ ] `crossterm` version
- [ ] `tokio` version
- [ ] Any new crates for skills/tui2

---

## Risk Assessment

| Feature | Risk | Mitigation |
|---------|------|------------|
| Skills System | Medium - Core integration | Port incrementally, test each step |
| TUI2 | High - Major rewrite | Extract features, don't replace |
| Vim Mode | Low - Isolated change | Feature flag to disable |
| Splash | Low - Cosmetic | Easy to revert |

---

## Recommended Order

1. **Skills System** - High value, clean module boundary
2. **Vim Navigation** - Popular request, isolated change
3. **Splash Screen** - Quick win, visible improvement
4. **Shell Snapshotting** - Useful for undo
5. **allowed_sandbox_modes** - Security improvement
6. **TUI2 features** - Incremental extraction
7. **CI/CD** - Final polish

---

## Commands to Start

```bash
# Clone upstream for reference
git clone https://github.com/openai/codex.git /tmp/codex-upstream

# Compare skills module
diff -r /tmp/codex-upstream/codex-rs/core/src/skills \
        ./core/src/skills 2>/dev/null || echo "skills/ doesn't exist yet"

# Check for new dependencies
diff /tmp/codex-upstream/codex-rs/Cargo.toml ./Cargo.toml

# Start porting skills
mkdir -p core/src/skills
cp -r /tmp/codex-upstream/codex-rs/core/src/skills/* core/src/skills/
```
