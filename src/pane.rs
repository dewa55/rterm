//! A pane: one shell on a PTY, its terminal state and shell-integration state.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{ClipboardType, Config};
use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle, Processor};
use eframe::egui;
use portable_pty::{ChildKiller, MasterPty, PtySize, native_pty_system};

use crate::input::MouseButton;
use crate::layout::PaneId;
use crate::shell_integration::{OscScanner, ShellState, shell_command};

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// Receives terminal events from inside `Processor::advance` (while the engine is locked),
/// so it only touches state that is not behind the engine mutex.
#[derive(Clone)]
pub struct Listener {
    writer: SharedWriter,
    ctx: egui::Context,
    title: Arc<Mutex<Option<String>>>,
    bell: Arc<AtomicBool>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            // Replies to device/status queries (DA, DSR, ...).
            Event::PtyWrite(text) => {
                let _ = self.writer.lock().unwrap().write_all(text.as_bytes());
            }
            Event::Title(t) => *self.title.lock().unwrap() = Some(t),
            Event::ResetTitle => *self.title.lock().unwrap() = None,
            Event::ClipboardStore(ClipboardType::Clipboard, text) => self.ctx.copy_text(text),
            Event::Bell => {
                self.bell.store(true, Ordering::Relaxed);
                self.ctx.request_repaint();
            }
            _ => {}
        }
    }
}

pub struct Engine {
    pub term: Term<Listener>,
    parser: Processor,
    scanner: OscScanner,
    pub shell: ShellState,
    /// Incremented on every PTY read, so caches (find matches) know the content changed.
    pub generation: u64,
}

impl Engine {
    pub fn feed(&mut self, bytes: &[u8]) {
        self.generation = self.generation.wrapping_add(1);
        let mut start = 0;
        for (end, event) in self.scanner.scan(bytes) {
            self.parser.advance(&mut self.term, &bytes[start..end]);
            self.shell.apply(event, &self.term);
            start = end;
        }
        self.parser.advance(&mut self.term, &bytes[start..]);
    }

    /// Ends a synchronized update (DEC mode 2026) whose deadline passed.
    /// Returns the pending deadline, if any.
    pub fn check_sync_timeout(&mut self) -> Option<Instant> {
        let deadline = self.parser.sync_timeout().sync_timeout()?;
        if Instant::now() >= deadline {
            self.parser.stop_sync(&mut self.term);
            None
        } else {
            Some(deadline)
        }
    }
}

pub struct Pane {
    pub id: PaneId,
    engine: Arc<Mutex<Engine>>,
    writer: SharedWriter,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pid: Option<u32>,
    exited: Arc<AtomicBool>,
    title: Arc<Mutex<Option<String>>>,
    pub bell: Arc<AtomicBool>,
    size: (u16, u16),
    /// Fractional mouse-wheel lines not yet applied.
    pub scroll_accum: f32,
    /// Mouse reporting state: button held and last reported cell.
    pub mouse_button: Option<MouseButton>,
    pub mouse_cell: Option<(usize, usize)>,
}

/// Settings that affect how a pane's shell and terminal are created.
pub struct SpawnOptions<'a> {
    pub shell: Option<&'a str>,
    pub integration_dir: Option<&'a Path>,
    pub term_config: Config,
    pub size: (u16, u16),
}

pub fn term_config(scrollback: usize, cursor: crate::config::CursorStyle) -> Config {
    let shape = match cursor {
        crate::config::CursorStyle::Block => CursorShape::Block,
        crate::config::CursorStyle::Beam => CursorShape::Beam,
        crate::config::CursorStyle::Underline => CursorShape::Underline,
    };
    Config {
        scrolling_history: scrollback,
        default_cursor_style: CursorStyle { shape, blinking: false },
        ..Config::default()
    }
}

impl Pane {
    pub fn spawn(id: PaneId, ctx: &egui::Context, cwd: Option<&Path>, opts: SpawnOptions<'_>) -> anyhow::Result<Pane> {
        let (cols, rows) = opts.size;
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        let mut child = pair.slave.spawn_command(shell_command(cwd, opts.shell, opts.integration_dir))?;
        // Drop our copy of the slave so reads hit EOF/EIO once the shell exits.
        drop(pair.slave);

        let pid = child.process_id();
        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader()?;
        let writer: SharedWriter = Arc::new(Mutex::new(pair.master.take_writer()?));
        let title = Arc::new(Mutex::new(None));
        let bell = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));

        let listener = Listener { writer: writer.clone(), ctx: ctx.clone(), title: title.clone(), bell: bell.clone() };
        let term = Term::new(opts.term_config, &TermSize::new(cols as usize, rows as usize), listener);
        let engine = Arc::new(Mutex::new(Engine {
            term,
            parser: Processor::new(),
            scanner: OscScanner::default(),
            shell: ShellState::default(),
            generation: 0,
        }));

        {
            let engine = engine.clone();
            let exited = exited.clone();
            let ctx = ctx.clone();
            std::thread::Builder::new().name(format!("pty-reader-{id}")).spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            engine.lock().unwrap().feed(&buf[..n]);
                            ctx.request_repaint();
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                exited.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            })?;
        }
        {
            let exited = exited.clone();
            let ctx = ctx.clone();
            std::thread::Builder::new().name(format!("pty-wait-{id}")).spawn(move || {
                let _ = child.wait();
                exited.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            })?;
        }

        Ok(Pane {
            id,
            engine,
            writer,
            master: pair.master,
            killer,
            pid,
            exited,
            title,
            bell,
            size: (cols, rows),
            scroll_accum: 0.0,
            mouse_button: None,
            mouse_cell: None,
        })
    }

    /// iTerm2's "Clear Buffer": drop scrollback and screen, then let the shell redraw its prompt.
    pub fn clear_buffer(&self) {
        {
            let mut engine = self.engine();
            engine.feed(b"\x1b[H\x1b[2J\x1b[3J");
            engine.shell.prune_all();
        }
        self.write(b"\x0c");
    }

    pub fn engine(&self) -> MutexGuard<'_, Engine> {
        self.engine.lock().unwrap()
    }

    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::Relaxed)
    }

    pub fn write(&self, bytes: &[u8]) {
        let _ = self.writer.lock().unwrap().write_all(bytes);
    }

    /// Writes user input: scrolls back to the bottom and clears the selection, like iTerm2.
    pub fn input(&self, bytes: &[u8]) {
        {
            let mut engine = self.engine();
            engine.term.scroll_display(Scroll::Bottom);
            engine.term.selection = None;
        }
        self.write(bytes);
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell: egui::Vec2) {
        if self.size == (cols, rows) {
            return;
        }
        self.size = (cols, rows);
        self.engine().term.resize(TermSize::new(cols as usize, rows as usize));
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: (cols as f32 * cell.x) as u16,
            pixel_height: (rows as f32 * cell.y) as u16,
        });
    }

    /// Title set by the running program (OSC 0/2).
    pub fn osc_title(&self) -> Option<String> {
        self.title.lock().unwrap().clone()
    }

    /// Name of the foreground process (e.g. `vim`), via the PTY's foreground process group.
    pub fn foreground_process(&self) -> Option<String> {
        let pgid = self.master.process_group_leader()?;
        if Some(pgid as u32) == self.pid {
            return None;
        }
        std::fs::read_to_string(format!("/proc/{pgid}/comm")).ok().map(|s| s.trim().to_owned())
    }

    /// Working directory for a new pane/tab opened from this one.
    ///
    /// Prefers the directory reported by shell integration unless the shell is on another
    /// host (ssh); falls back to the shell's cwd from /proc, which also works without integration.
    pub fn working_dir(&self) -> Option<PathBuf> {
        {
            let engine = self.engine();
            if engine.shell.remote_host.is_none()
                && let Some(cwd) = &engine.shell.cwd
            {
                return Some(cwd.clone());
            }
        }
        std::fs::read_link(format!("/proc/{}/cwd", self.pid?)).ok()
    }
}

impl Drop for Pane {
    fn drop(&mut self) {
        // SIGHUP, as when a terminal window is closed.
        let _ = self.killer.kill();
    }
}
