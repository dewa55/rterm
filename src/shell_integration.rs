//! Shell integration compatible with iTerm2 / FinalTerm escape codes.
//!
//! - OSC 133 A/B/C/D: prompt start, command start, command executed, command finished (exit code)
//! - OSC 7 `file://host/path`: current directory
//! - OSC 1337 `CurrentDir=` / `RemoteHost=`: iTerm2's own cwd and host reports
//!
//! `alacritty_terminal` ignores these sequences, so the PTY stream is scanned before it is
//! handed to the parser, and each event is applied at the exact byte where its OSC ends.

use std::path::{Path, PathBuf};

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::TermMode;
use portable_pty::CommandBuilder;

const MAX_OSC_LEN: usize = 4096;
const MAX_MARKS: usize = 10_000;

const BASH_INTEGRATION: &str = include_str!("../shell/rterm.bash");
const BASH_RC: &str = include_str!("../shell/rterm-bashrc");
const ZSH_INTEGRATION: &str = include_str!("../shell/rterm.zsh");
const ZSH_ENV: &str = include_str!("../shell/zsh/.zshenv");

#[derive(Debug, PartialEq)]
pub enum ShellEvent {
    PromptStart,
    CommandStart,
    CommandExecuted,
    CommandFinished(Option<i32>),
    Cwd { host: Option<String>, path: PathBuf },
    RemoteHost(String),
}

#[derive(Clone, Copy, Default)]
enum ScanState {
    #[default]
    Ground,
    Esc,
    Osc,
    OscEsc,
}

/// Incremental OSC scanner; state survives across PTY reads.
#[derive(Default)]
pub struct OscScanner {
    state: ScanState,
    buf: Vec<u8>,
}

impl OscScanner {
    /// Returns shell events with the offset just past the terminating byte of their OSC.
    pub fn scan(&mut self, bytes: &[u8]) -> Vec<(usize, ShellEvent)> {
        let mut out = Vec::new();
        for (i, &b) in bytes.iter().enumerate() {
            self.state = match (self.state, b) {
                (ScanState::Ground, 0x1b) => ScanState::Esc,
                (ScanState::Ground, _) => ScanState::Ground,
                (ScanState::Esc | ScanState::OscEsc, b']') => {
                    self.buf.clear();
                    ScanState::Osc
                }
                (ScanState::Esc, 0x1b) => ScanState::Esc,
                (ScanState::Esc, _) => ScanState::Ground,
                (ScanState::Osc, 0x07) | (ScanState::OscEsc, b'\\') => {
                    if let Some(ev) = parse_osc(&self.buf) {
                        out.push((i + 1, ev));
                    }
                    ScanState::Ground
                }
                (ScanState::Osc, 0x1b) => ScanState::OscEsc,
                (ScanState::Osc, 0x18 | 0x1a) => ScanState::Ground,
                (ScanState::Osc, _) => {
                    if self.buf.len() < MAX_OSC_LEN {
                        self.buf.push(b);
                    }
                    ScanState::Osc
                }
                (ScanState::OscEsc, 0x1b) => ScanState::Esc,
                (ScanState::OscEsc, _) => ScanState::Ground,
            };
        }
        out
    }
}

fn parse_osc(payload: &[u8]) -> Option<ShellEvent> {
    let s = std::str::from_utf8(payload).ok()?;
    let (code, rest) = s.split_once(';').unwrap_or((s, ""));
    match code {
        "133" => {
            let mut parts = rest.split(';');
            match parts.next()? {
                "A" => Some(ShellEvent::PromptStart),
                "B" => Some(ShellEvent::CommandStart),
                "C" => Some(ShellEvent::CommandExecuted),
                "D" => Some(ShellEvent::CommandFinished(parts.next().and_then(|c| c.parse().ok()))),
                _ => None,
            }
        }
        "7" => {
            let url = rest.strip_prefix("file://")?;
            let (host, path) = match url.find('/') {
                Some(i) => (&url[..i], &url[i..]),
                None => (url, "/"),
            };
            Some(ShellEvent::Cwd {
                host: (!host.is_empty()).then(|| host.to_owned()),
                path: PathBuf::from(percent_decode(path)),
            })
        }
        "1337" => {
            if let Some(dir) = rest.strip_prefix("CurrentDir=") {
                Some(ShellEvent::Cwd { host: None, path: PathBuf::from(dir) })
            } else {
                rest.strip_prefix("RemoteHost=").map(|h| ShellEvent::RemoteHost(h.to_owned()))
            }
        }
        _ => None,
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A prompt mark, like iTerm2's blue/red mark circles.
#[derive(Clone, Debug)]
pub struct Mark {
    /// Absolute line: `history_size + viewport line` at the time the mark was set.
    pub line: i64,
    pub executed: bool,
    pub exit: Option<i32>,
}

#[derive(Default)]
pub struct ShellState {
    pub marks: Vec<Mark>,
    pub cwd: Option<PathBuf>,
    /// Host reported by the shell when it differs from this machine (e.g. inside ssh).
    pub remote_host: Option<String>,
    /// True once any integration sequence was seen.
    pub active: bool,
    last_history: usize,
}

impl ShellState {
    pub fn apply<T>(&mut self, event: ShellEvent, term: &Term<T>) {
        self.active = true;
        self.prune(term);
        let alt_screen = term.mode().contains(TermMode::ALT_SCREEN);
        match event {
            ShellEvent::PromptStart if !alt_screen => {
                let line = cursor_line(term);
                // Re-drawn prompts (e.g. after a resize) replace the mark on the same line.
                if self.marks.last().is_some_and(|m| m.line == line) {
                    self.marks.pop();
                }
                self.marks.push(Mark { line, executed: false, exit: None });
                if self.marks.len() > MAX_MARKS {
                    self.marks.drain(..self.marks.len() - MAX_MARKS);
                }
            }
            ShellEvent::CommandExecuted => {
                if let Some(m) = self.marks.last_mut() {
                    m.executed = true;
                }
            }
            ShellEvent::CommandFinished(code) => {
                // Only a prompt that actually ran a command gets an exit status;
                // pressing Enter on an empty line re-reports the previous `$?`.
                if let Some(m) = self.marks.last_mut().filter(|m| m.executed && m.exit.is_none()) {
                    m.exit = Some(code.unwrap_or(0));
                }
            }
            ShellEvent::Cwd { host, path } => {
                self.remote_host = host.filter(|h| !is_local_host(h));
                self.cwd = Some(path);
            }
            ShellEvent::RemoteHost(h) => {
                let host = h.rsplit('@').next().unwrap_or(&h);
                self.remote_host = (!is_local_host(host)).then(|| host.to_owned());
            }
            ShellEvent::PromptStart | ShellEvent::CommandStart => {}
        }
    }

    /// Forgets all marks, e.g. after the buffer was cleared.
    pub fn prune_all(&mut self) {
        self.marks.clear();
        self.last_history = 0;
    }

    /// Drops marks that no longer refer to valid lines (scrollback cleared or shrunk).
    pub fn prune<T>(&mut self, term: &Term<T>) {
        let history = term.grid().history_size();
        if history < self.last_history {
            self.marks.clear();
        }
        self.last_history = history;
    }

    /// Absolute line of the viewport's top row.
    pub fn viewport_top<T>(term: &Term<T>) -> i64 {
        term.grid().history_size() as i64 - term.grid().display_offset() as i64
    }
}

fn cursor_line<T>(term: &Term<T>) -> i64 {
    term.grid().history_size() as i64 + term.grid().cursor.point.line.0 as i64
}

pub fn local_hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname").map(|s| s.trim().to_owned()).unwrap_or_default()
}

fn is_local_host(host: &str) -> bool {
    let local = local_hostname();
    host.is_empty()
        || host == "localhost"
        || host.eq_ignore_ascii_case(&local)
        || local.split('.').next().is_some_and(|short| host.eq_ignore_ascii_case(short))
}

/// Writes the bundled integration scripts to `$XDG_DATA_HOME/rterm/shell`.
pub fn install_scripts() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    let dir = data.join("rterm/shell");
    std::fs::create_dir_all(dir.join("zsh")).ok()?;
    for (name, content) in [
        ("rterm.bash", BASH_INTEGRATION),
        ("rterm-bashrc", BASH_RC),
        ("rterm.zsh", ZSH_INTEGRATION),
        ("zsh/.zshenv", ZSH_ENV),
    ] {
        std::fs::write(dir.join(name), content).ok()?;
    }
    Some(dir)
}

/// Builds the command for a new pane, injecting shell integration for bash and zsh.
/// `shell` overrides `$SHELL`; `integration_dir` is `None` when integration is disabled.
pub fn shell_command(cwd: Option<&Path>, shell: Option<&str>, integration_dir: Option<&Path>) -> CommandBuilder {
    let shell = match shell {
        Some(s) => s.to_owned(),
        None => std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_owned()),
    };
    let name = Path::new(&shell).file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
    let mut cmd = CommandBuilder::new(&shell);
    // Variables inherited from the terminal rterm was launched from would make startup
    // scripts (e.g. /etc/profile.d/vte-2.91.sh) emit their own integration sequences.
    for var in [
        "VTE_VERSION",
        "GNOME_TERMINAL_SCREEN",
        "GNOME_TERMINAL_SERVICE",
        "KONSOLE_VERSION",
        "KONSOLE_DBUS_SESSION",
        "KITTY_WINDOW_ID",
        "WEZTERM_PANE",
        "ALACRITTY_WINDOW_ID",
        "ITERM_SESSION_ID",
        "TERM_SESSION_ID",
    ] {
        cmd.env_remove(var);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "rterm");
    cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    if let Some(dir) = cwd.filter(|d| d.is_dir()) {
        cmd.cwd(dir);
    } else if let Some(home) = std::env::var_os("HOME") {
        cmd.cwd(home);
    }

    let disabled = std::env::var_os("RTERM_NO_SHELL_INTEGRATION").is_some();
    if let (Some(dir), false) = (integration_dir, disabled) {
        cmd.env("RTERM_SHELL_INTEGRATION_DIR", dir);
        match name.as_str() {
            "bash" => {
                cmd.arg("--rcfile");
                cmd.arg(dir.join("rterm-bashrc"));
            }
            "zsh" => {
                if let Some(orig) = std::env::var_os("ZDOTDIR") {
                    cmd.env("RTERM_ORIG_ZDOTDIR", orig);
                }
                cmd.env("ZDOTDIR", dir.join("zsh"));
            }
            // fish >= 4 emits OSC 133 and OSC 7 natively.
            _ => {}
        }
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_osc_across_chunks_and_terminators() {
        let mut s = OscScanner::default();
        let a = s.scan(b"hello\x1b]133;A\x07prompt$ \x1b]13");
        assert_eq!(a, vec![(13, ShellEvent::PromptStart)]);
        let b = s.scan(b"3;D;127\x1b\\\x1b]7;file://box/home/me/a%20b\x07");
        assert_eq!(b[0], (9, ShellEvent::CommandFinished(Some(127))));
        assert_eq!(b[1].1, ShellEvent::Cwd { host: Some("box".into()), path: PathBuf::from("/home/me/a b") });
    }

    #[test]
    fn parses_iterm2_sequences() {
        assert_eq!(
            parse_osc(b"1337;CurrentDir=/tmp"),
            Some(ShellEvent::Cwd { host: None, path: PathBuf::from("/tmp") })
        );
        assert_eq!(parse_osc(b"1337;RemoteHost=me@server"), Some(ShellEvent::RemoteHost("me@server".into())));
        assert_eq!(parse_osc(b"0;title"), None);
    }
}
