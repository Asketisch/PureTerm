//! Independent render thread with message-passing architecture.
//!
//! The render thread owns the TerminalRenderer and Surface, avoiding
//! borrow conflicts between the UI thread and GPU operations.

use crossbeam_channel::{bounded, Receiver, Sender};
use std::time::{Duration, Instant};

/// Messages sent from the main thread to the render thread.
#[derive(Debug)]
pub enum RenderMessage {
    /// Request terminal state to be rendered.
    Frame {
        /// Whether the terminal content changed since last frame.
        dirty: bool,
    },
    /// Window was resized.
    Resize {
        width: u32,
        height: u32,
    },
    /// Set the background image (raw RGBA bytes).
    SetBackgroundImage {
        data: Vec<u8>,
        width: u32,
        height: u32,
    },
    /// Clear the background image.
    ClearBackground,
    /// Set the corner radius.
    SetCornerRadius(f32),
    /// Set cursor blink on/off.
    SetCursorBlink(bool),
    /// Shutdown signal.
    Shutdown,
}

/// Handle for sending messages to the render thread.
/// Cloneable so multiple parts of the app can send commands.
#[derive(Clone)]
pub struct RenderThreadHandle {
    sender: Sender<RenderMessage>,
}

const MAILBOX_CAPACITY: usize = 64;

/// Statistics for the inspector.
#[derive(Default)]
pub struct RenderStats {
    pub fps: f32,
    pub frame_count: u64,
    pub draw_calls: u32,
    pub vertices: u32,
}

impl RenderThreadHandle {
    /// Spawn a new render thread and return a handle to it.
    pub fn spawn() -> Self {
        let (sender, receiver) = bounded(MAILBOX_CAPACITY);

        std::thread::Builder::new()
            .name("pureterm-render".into())
            .spawn(move || {
                RenderThread::new(receiver).run();
            })
            .expect("failed to spawn render thread");

        Self { sender }
    }

    /// Send a message to the render thread. Returns false if the channel is closed.
    pub fn send(&self, msg: RenderMessage) -> bool {
        self.sender.send(msg).is_ok()
    }

    /// Request a redraw.
    pub fn request_frame(&self, dirty: bool) {
        let _ = self.sender.send(RenderMessage::Frame { dirty });
    }

    /// Notify of a window resize.
    pub fn resize(&self, width: u32, height: u32) {
        let _ = self.sender.send(RenderMessage::Resize { width, height });
    }

    /// Set the background image.
    pub fn set_background_image(&self, data: Vec<u8>, width: u32, height: u32) {
        let _ = self.sender.send(RenderMessage::SetBackgroundImage { data, width, height });
    }

    /// Clear the background.
    pub fn clear_background(&self) {
        let _ = self.sender.send(RenderMessage::ClearBackground);
    }

    /// Set corner radius.
    pub fn set_corner_radius(&self, r: f32) {
        let _ = self.sender.send(RenderMessage::SetCornerRadius(r));
    }

    /// Shutdown the render thread.
    pub fn shutdown(&self) {
        let _ = self.sender.send(RenderMessage::Shutdown);
    }
}

/// Statistics handle — shared between render thread and UI.
pub struct RenderStatsHandle {
    fps: std::sync::atomic::AtomicU32, // stored as u32 bits (IEEE754)
    frame_count: std::sync::atomic::AtomicU64,
    draw_calls: std::sync::atomic::AtomicU32,
    vertices: std::sync::atomic::AtomicU32,
}

impl Default for RenderStatsHandle {
    fn default() -> Self {
        Self {
            fps: std::sync::atomic::AtomicU32::new(0),
            frame_count: std::sync::atomic::AtomicU64::new(0),
            draw_calls: std::sync::atomic::AtomicU32::new(0),
            vertices: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

impl RenderStatsHandle {
    pub fn snapshot(&self) -> RenderStats {
        RenderStats {
            fps: f32::from_bits(self.fps.load(std::sync::atomic::Ordering::Relaxed)),
            frame_count: self.frame_count.load(std::sync::atomic::Ordering::Relaxed),
            draw_calls: self.draw_calls.load(std::sync::atomic::Ordering::Relaxed),
            vertices: self.vertices.load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    fn record_frame(&self, fps: f32, draw_calls: u32, vertices: u32) {
        self.fps.store(fps.to_bits(), std::sync::atomic::Ordering::Relaxed);
        self.frame_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.draw_calls.store(draw_calls, std::sync::atomic::Ordering::Relaxed);
        self.vertices.store(vertices, std::sync::atomic::Ordering::Relaxed);
    }
}

// ── Render Thread State ──────────────────────────────────────────────────────

struct RenderThread {
    receiver: Receiver<RenderMessage>,
    running: bool,
    last_fps_time: Instant,
    frame_count: u64,
    current_fps: f32,
    cursor_blink_visible: bool,
    cursor_blink_timer: Instant,
    cursor_blink_enabled: bool,
    stats: RenderStatsHandle,
}

const BLINK_INTERVAL: Duration = Duration::from_millis(500);

impl RenderThread {
    fn new(receiver: Receiver<RenderMessage>) -> Self {
        Self {
            receiver,
            running: true,
            last_fps_time: Instant::now(),
            frame_count: 0,
            current_fps: 0.0,
            cursor_blink_visible: true,
            cursor_blink_timer: Instant::now(),
            cursor_blink_enabled: true,
            stats: RenderStatsHandle::default(),
        }
    }

    fn run(&mut self) {
        while self.running {
            self.process_messages();
            self.update_blink();
            self.update_fps();
            // Brief sleep to avoid spinning when no messages
            let _ = self.receiver.recv_timeout(Duration::from_millis(1));
        }
        log::info!("Render thread shutting down");
    }

    fn process_messages(&mut self) {
        while let Ok(msg) = self.receiver.try_recv() {
            match msg {
                RenderMessage::Shutdown => {
                    self.running = false;
                }
                RenderMessage::Resize { width, height } => {
                    log::debug!("Render: resize {}x{}", width, height);
                    self.handle_resize(width, height);
                }
                RenderMessage::SetBackgroundImage { data, width, height } => {
                    log::debug!("Render: background image {}x{}", width, height);
                    self.upload_background(data, width, height);
                }
                RenderMessage::ClearBackground => {
                    log::debug!("Render: clear background");
                }
                RenderMessage::SetCornerRadius(r) => {
                    log::debug!("Render: corner radius {}", r);
                }
                RenderMessage::SetCursorBlink(enabled) => {
                    self.cursor_blink_enabled = enabled;
                    if enabled {
                        self.cursor_blink_visible = true;
                        self.cursor_blink_timer = Instant::now();
                    }
                }
                RenderMessage::Frame { dirty } => {
                    if dirty {
                        self.draw_frame();
                    }
                }
            }
        }
    }

    fn handle_resize(&mut self, _width: u32, _height: u32) {
        // In full implementation: reconfigure surface, rebuild textures
    }

    fn upload_background(&mut self, _data: Vec<u8>, _width: u32, _height: u32) {
        // In full implementation: create GPU texture from RGBA bytes
    }

    fn draw_frame(&mut self) {
        // Full implementation:
        // 1. Acquire surface texture
        // 2. Build pass data (terminal cells → vertices)
        // 3. Execute passes in order
        // 4. Present
        // 5. Record stats
        self.frame_count += 1;
    }

    fn update_blink(&mut self) {
        if self.cursor_blink_enabled
            && self.cursor_blink_timer.elapsed() >= BLINK_INTERVAL
        {
            self.cursor_blink_visible = !self.cursor_blink_visible;
            self.cursor_blink_timer = Instant::now();
        }
    }

    fn update_fps(&mut self) {
        let elapsed = self.last_fps_time.elapsed();
        if elapsed >= Duration::from_secs(1) {
            self.current_fps = self.frame_count as f32 / elapsed.as_secs_f32();
            self.frame_count = 0;
            self.last_fps_time = Instant::now();
            self.stats.record_frame(self.current_fps, 0, 0);
        }
    }
}
