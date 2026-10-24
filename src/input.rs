//! Translates egui key events into the byte sequences an xterm-compatible terminal sends.

use alacritty_terminal::term::TermMode;
use eframe::egui::{Key, Modifiers};

/// xterm modifier parameter: 1 + shift + 2*alt + 4*ctrl.
fn modifier_param(m: Modifiers) -> u8 {
    1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.ctrl as u8
}

/// CSI sequence for cursor-style keys (`ESC [ A`, `ESC O A` in application mode,
/// `ESC [ 1 ; m A` with modifiers).
fn cursor_key(final_byte: char, m: Modifiers, app_mode: bool) -> Vec<u8> {
    let param = modifier_param(m);
    if param > 1 {
        format!("\x1b[1;{param}{final_byte}").into_bytes()
    } else if app_mode {
        format!("\x1bO{final_byte}").into_bytes()
    } else {
        format!("\x1b[{final_byte}").into_bytes()
    }
}

/// `ESC [ n ~` keys (Insert, Delete, PageUp, F5, ...).
fn tilde_key(n: u8, m: Modifiers) -> Vec<u8> {
    let param = modifier_param(m);
    if param > 1 { format!("\x1b[{n};{param}~").into_bytes() } else { format!("\x1b[{n}~").into_bytes() }
}

/// F1-F4 use SS3 without modifiers and CSI with them.
fn ss3_key(final_byte: char, m: Modifiers) -> Vec<u8> {
    let param = modifier_param(m);
    if param > 1 {
        format!("\x1b[1;{param}{final_byte}").into_bytes()
    } else {
        format!("\x1bO{final_byte}").into_bytes()
    }
}

fn ctrl_byte(key: Key) -> Option<u8> {
    let name = key.name();
    if name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic() {
        return Some(name.as_bytes()[0].to_ascii_uppercase() - b'A' + 1);
    }
    Some(match key {
        Key::Space | Key::Num2 => 0,
        Key::OpenBracket | Key::Num3 => 0x1b,
        Key::Backslash | Key::Num4 => 0x1c,
        Key::CloseBracket | Key::Num5 => 0x1d,
        Key::Num6 => 0x1e,
        Key::Slash | Key::Minus | Key::Num7 => 0x1f,
        Key::Num8 => 0x7f,
        _ => return None,
    })
}

/// Bytes for a key press, or `None` when the key is delivered as an `Event::Text` instead.
pub fn key_bytes(key: Key, m: Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    let esc_prefix = |mut v: Vec<u8>| {
        if m.alt {
            v.insert(0, 0x1b);
        }
        v
    };
    let bytes = match key {
        Key::Enter => esc_prefix(vec![b'\r']),
        Key::Tab if m.shift => b"\x1b[Z".to_vec(),
        Key::Tab => esc_prefix(vec![b'\t']),
        Key::Backspace if m.ctrl => esc_prefix(vec![0x08]),
        Key::Backspace => esc_prefix(vec![0x7f]),
        Key::Escape => vec![0x1b],
        Key::ArrowUp => cursor_key('A', m, app_cursor),
        Key::ArrowDown => cursor_key('B', m, app_cursor),
        Key::ArrowRight => cursor_key('C', m, app_cursor),
        Key::ArrowLeft => cursor_key('D', m, app_cursor),
        Key::Home => cursor_key('H', m, app_cursor),
        Key::End => cursor_key('F', m, app_cursor),
        Key::Insert => tilde_key(2, m),
        Key::Delete => tilde_key(3, m),
        Key::PageUp => tilde_key(5, m),
        Key::PageDown => tilde_key(6, m),
        Key::F1 => ss3_key('P', m),
        Key::F2 => ss3_key('Q', m),
        Key::F3 => ss3_key('R', m),
        Key::F4 => ss3_key('S', m),
        Key::F5 => tilde_key(15, m),
        Key::F6 => tilde_key(17, m),
        Key::F7 => tilde_key(18, m),
        Key::F8 => tilde_key(19, m),
        Key::F9 => tilde_key(20, m),
        Key::F10 => tilde_key(21, m),
        Key::F11 => tilde_key(23, m),
        Key::F12 => tilde_key(24, m),
        _ if m.ctrl => esc_prefix(vec![ctrl_byte(key)?]),
        // Alt+printable: send ESC + the character (meta sends escape); egui also emits an
        // Event::Text for this key, which the caller drops.
        _ if m.alt => {
            let name = key.name();
            if name.chars().count() != 1 {
                return None;
            }
            let c = if m.shift { name.to_owned() } else { name.to_lowercase() };
            esc_prefix(c.into_bytes())
        }
        _ => return None,
    };
    Some(bytes)
}

/// Paste payload, wrapped in bracketed-paste markers when the application asked for them.
pub fn paste_bytes(text: &str, mode: TermMode) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if mode.contains(TermMode::BRACKETED_PASTE) {
        // Strip ESC so pasted text cannot terminate the bracket early.
        let clean: String = text.chars().filter(|&c| c != '\x1b').collect();
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}

/// Mouse buttons as xterm encodes them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseButton {
    Left = 0,
    Middle = 1,
    Right = 2,
    /// Motion with no button held (any-event tracking).
    None = 3,
    WheelUp = 64,
    WheelDown = 65,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

/// Encodes a mouse event for an application that enabled mouse tracking.
/// `col`/`row` are 0-based viewport cells. Returns `None` when the legacy encoding
/// cannot represent the position.
pub fn mouse_report(
    button: MouseButton,
    action: MouseAction,
    m: Modifiers,
    col: usize,
    row: usize,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let mods = 4 * m.shift as u32 + 8 * m.alt as u32 + 16 * m.ctrl as u32;
    let motion = if action == MouseAction::Motion { 32 } else { 0 };
    let (x, y) = (col as u32 + 1, row as u32 + 1);
    if mode.contains(TermMode::SGR_MOUSE) {
        let code = button as u32 + mods + motion;
        let suffix = if action == MouseAction::Release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{suffix}").into_bytes());
    }
    // Legacy encodings cannot say which button was released.
    let base = if action == MouseAction::Release { 3 } else { button as u32 };
    let values = [32 + base + mods + motion, 32 + x, 32 + y];
    let mut out = b"\x1b[M".to_vec();
    if mode.contains(TermMode::UTF8_MOUSE) {
        for v in values {
            let c = char::from_u32(v).filter(|_| v < 2048)?;
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    } else {
        for v in values {
            out.push(u8::try_from(v).ok()?);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_encodings() {
        let sgr = TermMode::SGR_MOUSE;
        let none = Modifiers::NONE;
        assert_eq!(
            mouse_report(MouseButton::Left, MouseAction::Press, none, 0, 0, sgr),
            Some(b"\x1b[<0;1;1M".to_vec())
        );
        assert_eq!(
            mouse_report(MouseButton::Left, MouseAction::Release, none, 9, 4, sgr),
            Some(b"\x1b[<0;10;5m".to_vec())
        );
        assert_eq!(
            mouse_report(MouseButton::Left, MouseAction::Motion, Modifiers::CTRL, 1, 1, sgr),
            Some(b"\x1b[<48;2;2M".to_vec())
        );
        assert_eq!(
            mouse_report(MouseButton::WheelUp, MouseAction::Press, none, 0, 0, TermMode::empty()),
            Some(vec![0x1b, b'[', b'M', 32 + 64, 33, 33])
        );
        assert_eq!(
            mouse_report(MouseButton::Right, MouseAction::Release, none, 0, 0, TermMode::empty()),
            Some(vec![0x1b, b'[', b'M', 32 + 3, 33, 33])
        );
        assert_eq!(mouse_report(MouseButton::Left, MouseAction::Press, none, 300, 0, TermMode::empty()), None);
    }

    #[test]
    fn control_and_cursor_keys() {
        let none = TermMode::empty();
        assert_eq!(key_bytes(Key::C, Modifiers::CTRL, none), Some(vec![3]));
        assert_eq!(key_bytes(Key::ArrowUp, Modifiers::NONE, none), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(Key::ArrowUp, Modifiers::NONE, TermMode::APP_CURSOR), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_bytes(Key::ArrowLeft, Modifiers::CTRL, none), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(key_bytes(Key::B, Modifiers::ALT, none), Some(b"\x1bb".to_vec()));
        assert_eq!(key_bytes(Key::A, Modifiers::NONE, none), None);
    }

    #[test]
    fn bracketed_paste() {
        assert_eq!(paste_bytes("a\nb", TermMode::empty()), b"a\rb");
        assert_eq!(paste_bytes("x\x1b", TermMode::BRACKETED_PASTE), b"\x1b[200~x\x1b[201~");
    }
}
