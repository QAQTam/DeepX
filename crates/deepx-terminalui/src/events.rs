//! AppEvent — DeepX TUI semantic event bus.
//!
//! Maps crossterm raw terminal events to semantic application events,
//! dispatched by event_dispatch to the App state machine.

use deepx_proto::Agent2Ui;

#[derive(Debug, Clone)]
pub enum AppEvent {
    // Navigation
    HistoryPrevious,
    HistoryNext,

    // Scroll
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,

    // Input editing
    TypeChar(char),
    Backspace,
    DeleteWordBack,
    Delete,
    MoveToLineStart,
    MoveToLineEnd,
    CursorLeft,
    CursorRight,
    CursorWordLeft,
    CursorWordRight,
    InsertNewline,

    // Send / commands
    SendMessage(String),
    CancelRequest,
    ClearScreen,
    Quit,
    SendCommand(String),

    // Panel toggles
    ToggleDebug,
    ToggleTasks,
    ToggleContext,
    ToggleHelp,
    ToggleThinking,
    ToggleDetailPane,
    DismissOverlay,
    OpenMenu,

    // Session
    NewSession,
    SwitchSession,

    // Popup interaction
    AskUp,
    AskDown,
    AskConfirm,
    AskCancel,

    // Paste
    Paste(String),

    // Terminal
    Resize(u16, u16),
    Tick,

    // Agent
    AgentFrame(Agent2Ui),
    AgentDisconnected,

    Noop,
}
