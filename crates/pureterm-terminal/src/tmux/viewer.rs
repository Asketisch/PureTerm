//! Tmux session viewer
//!
//! Manages connection to a tmux session and displays its state.

use std::collections::HashMap;

use super::control::{ControlParser, ControlEvent, TmuxCommand, encode_command};
use super::layout::{Layout, PaneInfo, validate, flatten};

/// Viewer connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerState {
    /// Not connected
    Disconnected,
    /// Connecting
    Connecting,
    /// Attached to session
    Attached,
    /// Detaching
    Detaching,
}

/// Information about a tmux session
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Session name
    pub name: String,
    /// Session ID
    pub id: u64,
    /// Number of windows
    pub window_count: usize,
    /// Session current window
    pub current_window: usize,
}

/// Information about a tmux window
#[derive(Debug, Clone)]
pub struct WindowInfo {
    /// Window ID
    pub id: u64,
    /// Window index
    pub index: usize,
    /// Window name
    pub name: String,
    /// Layout string
    pub layout: String,
    /// Panes in this window
    pub panes: Vec<u64>,
    /// Whether this is the active window
    pub active: bool,
}

/// Tmux session viewer
pub struct Viewer {
    /// Current state
    state: ViewerState,
    /// Session info
    session: Option<SessionInfo>,
    /// Windows
    windows: HashMap<u64, WindowInfo>,
    /// Panes
    panes: HashMap<u64, PaneInfo>,
    /// Active window ID
    active_window: Option<u64>,
    /// Active pane ID
    active_pane: Option<u64>,
    /// Parsed layout
    current_layout: Option<Layout>,
    /// Control mode parser
    parser: ControlParser,
    /// Pending command output
    pending_output: HashMap<u64, String>,
    /// Default pane size
    #[allow(dead_code)]
    default_pane_size: (u16, u16),
}

impl Viewer {
    pub fn new() -> Self {
        Self {
            state: ViewerState::Disconnected,
            session: None,
            windows: HashMap::new(),
            panes: HashMap::new(),
            active_window: None,
            active_pane: None,
            current_layout: None,
            parser: ControlParser::new(),
            pending_output: HashMap::new(),
            default_pane_size: (80, 24),
        }
    }

    /// Get current state
    pub fn state(&self) -> ViewerState {
        self.state
    }

    /// Get current session
    pub fn session(&self) -> Option<&SessionInfo> {
        self.session.as_ref()
    }

    /// Get all windows
    pub fn windows(&self) -> Vec<&WindowInfo> {
        self.windows.values().collect()
    }

    /// Get active window
    pub fn active_window(&self) -> Option<&WindowInfo> {
        self.active_window.and_then(|id| self.windows.get(&id))
    }

    /// Get active pane
    pub fn active_pane(&self) -> Option<&PaneInfo> {
        self.active_pane.and_then(|id| self.panes.get(&id))
    }

    /// Get current layout
    pub fn layout(&self) -> Option<&Layout> {
        self.current_layout.as_ref()
    }

    /// Get pane by ID
    pub fn get_pane(&self, id: u64) -> Option<&PaneInfo> {
        self.panes.get(&id)
    }

    /// Connect to a tmux session
    pub fn attach(&mut self, _session_name: &str) {
        self.state = ViewerState::Connecting;
        // In a real implementation, this would start tmux control mode
        // and send commands to attach
    }

    /// Disconnect from tmux session
    pub fn detach(&mut self) {
        self.state = ViewerState::Detaching;
        // Clear state
        self.session = None;
        self.windows.clear();
        self.panes.clear();
        self.active_window = None;
        self.active_pane = None;
        self.current_layout = None;
    }

    /// Process data from tmux
    pub fn process_output(&mut self, data: &str) -> Vec<ViewerAction> {
        let mut actions = Vec::new();
        let events = self.parser.feed_string(data);

        for event in events {
            if let Some(action) = self.handle_event(event) {
                actions.push(action);
            }
        }

        actions
    }

    /// Process raw tmux output (lines prefixed with %)
    pub fn process_raw_output(&mut self, data: &str) -> Vec<ViewerAction> {
        let mut actions = Vec::new();
        let events = self.parser.feed_raw_string(data);

        for event in events {
            if let Some(action) = self.handle_event(event) {
                actions.push(action);
            }
        }

        actions
    }

    /// Handle a control event
    fn handle_event(&mut self, event: ControlEvent) -> Option<ViewerAction> {
        match event {
            ControlEvent::SessionChanged { id, name } => {
                self.session = Some(SessionInfo {
                    name,
                    id,
                    window_count: 0,
                    current_window: 0,
                });
                self.state = ViewerState::Attached;
                Some(ViewerAction::Connected)
            }

            ControlEvent::LayoutChange { window_id, layout } => {
                // Parse layout
                if let Ok(parsed) = validate(&layout) {
                    self.current_layout = Some(parsed.clone());

                    // Update pane info
                    for pane in flatten(&parsed) {
                        self.panes.insert(
                            pane.id.parse().unwrap_or(0),
                            pane.clone(),
                        );
                    }
                }

                // Update window
                if let Some(window) = self.windows.get_mut(&window_id) {
                    window.layout = layout;
                }

                Some(ViewerAction::LayoutChanged)
            }

            ControlEvent::Output { pane_id, data } => {
                // Accumulate output for pane
                self.pending_output
                    .entry(pane_id)
                    .or_default()
                    .push_str(&data);
                Some(ViewerAction::PaneOutput { pane_id })
            }

            ControlEvent::Exit { reason } => {
                self.state = ViewerState::Disconnected;
                Some(ViewerAction::Disconnected { reason })
            }

            ControlEvent::WindowAdd { window_id } => {
                let info = WindowInfo {
                    id: window_id,
                    index: self.windows.len(),
                    name: format!("{}", self.windows.len()),
                    layout: String::new(),
                    panes: Vec::new(),
                    active: false,
                };
                self.windows.insert(window_id, info);
                self.active_window = Some(window_id);
                Some(ViewerAction::WindowAdded { window_id })
            }

            ControlEvent::WindowClose { window_id } => {
                self.windows.remove(&window_id);
                if self.active_window == Some(window_id) {
                    self.active_window = self.windows.keys().next().copied();
                }
                Some(ViewerAction::WindowClosed { window_id })
            }

            ControlEvent::PaneMode { pane_id, mode } => {
                // Handle pane mode changes (like copy mode, sync mode, etc.)
                Some(ViewerAction::PaneModeChanged { pane_id, mode })
            }

            ControlEvent::Begin { id } => {
                // Command output starts
                self.pending_output.insert(id, String::new());
                None
            }

            ControlEvent::End { id } => {
                // Command output ends - retrieve results
                if let Some(output) = self.pending_output.remove(&id) {
                    return Some(ViewerAction::CommandOutput { id, output });
                }
                None
            }

            ControlEvent::Error { id, message } => {
                Some(ViewerAction::Error { id, message })
            }

            ControlEvent::Unknown { name: _, args: _ } => {
                // Handle unknown events gracefully
                None
            }
        }
    }

    /// Switch to a different pane
    pub fn switch_pane(&mut self, pane_id: u64) -> Option<String> {
        if self.panes.contains_key(&pane_id) {
            self.active_pane = Some(pane_id);
            let cmd = TmuxCommand::SelectPane { id: pane_id };
            Some(encode_command(&cmd))
        } else {
            None
        }
    }

    /// Switch to a different window
    pub fn switch_window(&mut self, window_id: u64) -> Option<String> {
        if self.windows.contains_key(&window_id) {
            self.active_window = Some(window_id);
            let cmd = TmuxCommand::SelectWindow {
                index: self.windows.get(&window_id).map(|w| w.index).unwrap_or(0),
            };
            Some(encode_command(&cmd))
        } else {
            None
        }
    }

    /// Send keys to the active pane
    pub fn send_keys(&mut self, keys: &str) -> String {
        let cmd = TmuxCommand::SendKeys {
            pane_id: self.active_pane,
            keys: keys.to_string(),
        };
        encode_command(&cmd)
    }

    /// Resize the active pane
    pub fn resize_pane(&mut self, width: u16, height: u16) -> Option<String> {
        let cmd = TmuxCommand::ResizePane {
            pane_id: self.active_pane,
            width: width as usize,
            height: height as usize,
        };
        Some(encode_command(&cmd))
    }

    /// Refresh the layout display
    pub fn refresh_layout(&mut self) -> Option<String> {
        let cmd = TmuxCommand::Custom("show-window-options -g window-size".to_string());
        Some(encode_command(&cmd))
    }
}

impl Default for Viewer {
    fn default() -> Self {
        Self::new()
    }
}

/// Actions returned by the viewer after processing
#[derive(Debug, Clone, PartialEq)]
pub enum ViewerAction {
    /// Successfully connected
    Connected,
    /// Disconnected from session
    Disconnected { reason: Option<String> },
    /// Layout changed
    LayoutChanged,
    /// New pane output received
    PaneOutput { pane_id: u64 },
    /// Window was added
    WindowAdded { window_id: u64 },
    /// Window was closed
    WindowClosed { window_id: u64 },
    /// Pane mode changed
    PaneModeChanged { pane_id: u64, mode: String },
    /// Command output received
    CommandOutput { id: u64, output: String },
    /// Error occurred
    Error { id: u64, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewer_initial_state() {
        let viewer = Viewer::new();
        assert_eq!(viewer.state(), ViewerState::Disconnected);
        assert!(viewer.session().is_none());
        assert!(viewer.windows().is_empty());
    }

    #[test]
    fn test_process_session_changed() {
        let mut viewer = Viewer::new();
        viewer.state = ViewerState::Connecting;
        let actions = viewer.process_raw_output("%session-changed 1 mysession");
        assert!(actions.contains(&ViewerAction::Connected));
        assert!(viewer.session().is_some());
        assert_eq!(viewer.session().unwrap().name, "mysession");
    }

    #[test]
    fn test_process_layout_change() {
        let mut viewer = Viewer::new();
        viewer.state = ViewerState::Attached;
        let actions = viewer.process_raw_output("%layout-change 1 80x24,0,0,80x24,0,0,0");
        assert!(actions.contains(&ViewerAction::LayoutChanged));
        assert!(viewer.layout().is_some());
    }

    #[test]
    fn test_process_output() {
        let mut viewer = Viewer::new();
        viewer.state = ViewerState::Attached;
        let actions = viewer.process_raw_output("%output 1 hello");
        assert!(actions.iter().any(|a| match a {
            ViewerAction::PaneOutput { pane_id } => *pane_id == 1,
            _ => false,
        }));
    }

    #[test]
    fn test_process_exit() {
        let mut viewer = Viewer::new();
        viewer.state = ViewerState::Attached;
        viewer.session = Some(SessionInfo {
            name: "test".to_string(),
            id: 1,
            window_count: 1,
            current_window: 0,
        });
        let actions = viewer.process_raw_output("%exit");
        assert!(actions.iter().any(|a| match a {
            ViewerAction::Disconnected { .. } => true,
            _ => false,
        }));
        assert_eq!(viewer.state(), ViewerState::Disconnected);
    }
}
