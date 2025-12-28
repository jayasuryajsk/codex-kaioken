use strum::IntoEnumIterator;
use strum_macros::AsRefStr;
use strum_macros::EnumIter;
use strum_macros::EnumString;
use strum_macros::IntoStaticStr;

/// Commands that can be invoked by starting a message with a leading slash.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, EnumIter, AsRefStr, IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum SlashCommand {
    // DO NOT ALPHA-SORT! Enum order is presentation order in the popup, so
    // more frequently used commands should be listed first.
    Model,
    Approvals,
    Experimental,
    Settings,
    Skills,
    Plan,
    Review,
    New,
    Init,
    Compact,
    Undo,
    Checkpoint,
    RestoreCheckpoint,
    ListCheckpoints,
    Diff,
    Mention,
    Status,
    Ps,
    Kill,
    Mcp,
    Remember,
    Memories,
    Logout,
    Quit,
    Exit,
    Feedback,
    Rollout,
    TestApproval,
}

impl SlashCommand {
    /// User-visible description shown in the popup.
    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::Feedback => "send logs to maintainers",
            SlashCommand::New => "start a new chat during a conversation",
            SlashCommand::Init => "create an AGENTS.md file with instructions for Codex Kaioken",
            SlashCommand::Compact => "summarize conversation to prevent hitting the context limit",
            SlashCommand::Review => "review my current changes and find issues",
            SlashCommand::Undo => "ask Codex Kaioken to undo a turn",
            SlashCommand::Checkpoint => "save a named checkpoint (use `/checkpoint <name>`)",
            SlashCommand::RestoreCheckpoint => {
                "restore a saved checkpoint (`/restore-checkpoint <name>`)"
            }
            SlashCommand::ListCheckpoints => "list saved checkpoints",
            SlashCommand::Quit | SlashCommand::Exit => "exit Codex Kaioken",
            SlashCommand::Diff => "show git diff (including untracked files)",
            SlashCommand::Mention => "mention a file",
            SlashCommand::Status => "show current session configuration and token usage",
            SlashCommand::Ps => "list background terminals",
            SlashCommand::Kill => "kill a background terminal (`/kill <id>`)",
            SlashCommand::Model => "choose what model and reasoning effort to use",
            SlashCommand::Approvals => "choose what Codex Kaioken can do without approval",
            SlashCommand::Experimental => "toggle beta features",
            SlashCommand::Settings => "customize footer and other Kaioken UI defaults",
            SlashCommand::Skills => "list and toggle available skills",
            SlashCommand::Plan => "toggle plan mode or review pending plans",
            SlashCommand::Mcp => "list configured MCP tools",
            SlashCommand::Remember => "save something to memory (`/remember <text>`)",
            SlashCommand::Memories => "show stored memories and stats",
            SlashCommand::Logout => "log out of Codex Kaioken",
            SlashCommand::Rollout => "print the rollout file path",
            SlashCommand::TestApproval => "test approval request",
        }
    }

    /// Command string without the leading '/'. Provided for compatibility with
    /// existing code that expects a method named `command()`.
    pub fn command(self) -> &'static str {
        self.into()
    }

    /// Whether this command can be run while a task is in progress.
    pub fn available_during_task(self) -> bool {
        match self {
            // Commands that either start a new turn, mutate in-flight work,
            // or would conflict with backend state stay disabled.
            SlashCommand::New
            | SlashCommand::Init
            | SlashCommand::Compact
            | SlashCommand::Undo
            | SlashCommand::Checkpoint
            | SlashCommand::RestoreCheckpoint
            | SlashCommand::Review
            | SlashCommand::Logout => false,
            // Pure UI/configuration commands (toggle plan mode, change model, adjust approvals/settings)
            // are safe to run even while a task is executing.
            SlashCommand::Model
            | SlashCommand::Approvals
            | SlashCommand::Experimental
            | SlashCommand::Settings
            | SlashCommand::Skills
            | SlashCommand::Plan
            // All of the commands below already operated during tasks.
            | SlashCommand::Diff
            | SlashCommand::ListCheckpoints
            | SlashCommand::Mention
            | SlashCommand::Status
            | SlashCommand::Ps
            | SlashCommand::Kill
            | SlashCommand::Mcp
            | SlashCommand::Remember
            | SlashCommand::Memories
            | SlashCommand::Feedback
            | SlashCommand::Quit
            | SlashCommand::Exit
            | SlashCommand::Rollout
            | SlashCommand::TestApproval => true,
        }
    }

    fn is_visible(self) -> bool {
        match self {
            SlashCommand::Rollout | SlashCommand::TestApproval => cfg!(debug_assertions),
            _ => true,
        }
    }
}

/// Return all built-in commands in a Vec paired with their command string.
pub fn built_in_slash_commands() -> Vec<(&'static str, SlashCommand)> {
    SlashCommand::iter()
        .filter(|command| command.is_visible())
        .map(|c| (c.command(), c))
        .collect()
}
