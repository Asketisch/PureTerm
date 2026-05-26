use std::io;
use std::sync::Arc;

use alacritty_terminal::event::{EventListener, Notify, WindowSize};
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty::{self, Options as PtyOptions};
use crossbeam_channel::{bounded, Receiver, Sender};

/// Terminal event types sent from PTY thread to Iced.
#[derive(Debug, Clone)]
pub enum TerminalEvent {
    Wakeup,
    Exit,
}

/// Bridge: implements `EventListener` by sending through a crossbeam channel.
#[derive(Clone)]
pub struct ChannelEventListener {
    tx: Sender<TerminalEvent>,
}

impl ChannelEventListener {
    pub fn new(tx: Sender<TerminalEvent>) -> Self {
        Self { tx }
    }
}

impl EventListener for ChannelEventListener {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        use alacritty_terminal::event::Event as TEvent;
        let mapped = match event {
            TEvent::Wakeup => TerminalEvent::Wakeup,
            TEvent::Exit => TerminalEvent::Exit,
            TEvent::ChildExit(_) => TerminalEvent::Exit,
            _ => return,
        };
        let _ = self.tx.send(mapped);
    }
}

/// Terminal context: owns PTY, Term grid, and provides I/O interfaces.
pub struct TerminalContext {
    pub terminal: Arc<FairMutex<Term<ChannelEventListener>>>,
    pub notifier: Notifier,
    pub event_rx: Receiver<TerminalEvent>,
    pub size: TermSize,
}

impl TerminalContext {
    /// Create a new terminal context with a PTY running the user's shell.
    pub fn new() -> io::Result<Self> {
        let num_cols: u16 = 80;
        let num_lines: u16 = 24;
        let cell_width: u16 = 8;
        let cell_height: u16 = 16;
        let size = TermSize::new(num_cols as usize, num_lines as usize);

        let (event_tx, event_rx) = bounded(64);
        let event_listener = ChannelEventListener::new(event_tx);

        let term_config = TermConfig::default();
        let terminal = Term::new(term_config, &size, event_listener.clone());
        let terminal = Arc::new(FairMutex::new(terminal));

        let pty_options = PtyOptions::default();
        let window_size = WindowSize {
            num_lines,
            num_cols,
            cell_width,
            cell_height,
        };
        tty::setup_env();
        let pty = tty::new(&pty_options, window_size, 0)?;

        let pty_loop = PtyEventLoop::new(
            Arc::clone(&terminal),
            event_listener,
            pty,
            false,
            false,
        )?;

        let loop_tx = pty_loop.channel();
        let _io_thread = pty_loop.spawn();

        Ok(Self {
            terminal,
            notifier: Notifier(loop_tx),
            event_rx,
            size,
        })
    }

    /// Write bytes to the PTY (keyboard input).
    pub fn write_to_pty(&self, bytes: &[u8]) {
        self.notifier.notify(bytes.to_vec());
    }

    /// Resize the terminal grid and notify the PTY.
    pub fn resize(&self, cols: usize, rows: usize, cell_w: u16, cell_h: u16) {
        let size = TermSize::new(cols, rows);
        self.terminal.lock().resize(size);
        let window_size = WindowSize {
            num_cols: cols as u16,
            num_lines: rows as u16,
            cell_width: cell_w,
            cell_height: cell_h,
        };
        let _ = self.notifier.0.send(Msg::Resize(window_size));
    }
}
