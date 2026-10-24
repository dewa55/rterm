//! User configuration in `~/.config/rterm/config.toml`, reloaded when the file changes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eframe::egui::{Color32, Key, Modifiers};
use serde::Deserialize;

use crate::layout::{Axis, Direction};

pub const EXAMPLE: &str = include_str!("../config.example.toml");

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub font: FontConfig,
    pub window: WindowConfig,
    pub terminal: TerminalConfig,
    pub colors: ColorConfig,
    /// Extra or overriding keybindings: `"ctrl+shift+t" = "new_tab"`, or `"none"` to unbind.
    pub keys: BTreeMap<String, String>,
}

#[derive(Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct FontConfig {
    /// fontconfig family name (`fc-match` syntax) or a path to a .ttf/.otf file.
    pub family: String,
    pub size: f32,
    /// Multiplier on the font's natural line height.
    pub line_height: f32,
    pub use_bold_font: bool,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self { family: "monospace".into(), size: 14.0, line_height: 1.0, use_bold_font: true }
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    Auto,
    Dark,
    Light,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Decorations {
    /// rterm draws an iTerm2-style title bar with traffic-light buttons.
    #[default]
    Iterm,
    /// Native title bar from the desktop environment.
    System,
}

#[derive(Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct WindowConfig {
    pub theme: ThemeChoice,
    pub decorations: Decorations,
    pub columns: u16,
    pub rows: u16,
    pub show_tab_bar_with_one_tab: bool,
    pub pane_title_bars: bool,
    pub dim_inactive_panes: bool,
    /// 0.0–1.0, like iTerm2's dimming amount slider.
    pub dim_amount: f32,
    pub corner_radius: f32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::Auto,
            decorations: Decorations::Iterm,
            columns: 80,
            rows: 25,
            show_tab_bar_with_one_tab: false,
            pane_title_bars: true,
            dim_inactive_panes: true,
            dim_amount: 0.4,
            corner_radius: 10.0,
        }
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Beam,
    Underline,
}

#[derive(Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    /// Shell to run; defaults to `$SHELL`.
    pub shell: Option<String>,
    pub shell_integration: bool,
    pub scrollback: usize,
    pub cursor: CursorStyle,
    pub copy_on_select: bool,
    pub bold_is_bright: bool,
    /// Report mouse clicks/drags/wheel to applications that request it (vim, htop, tmux).
    /// Hold Shift to select text anyway.
    pub mouse_reporting: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            shell: None,
            shell_integration: true,
            scrollback: 10_000,
            cursor: CursorStyle::Block,
            copy_on_select: true,
            bold_is_bright: true,
            mouse_reporting: true,
        }
    }
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ColorConfig {
    /// `.itermcolors` file, or a multi-preset plist such as iTerm2's `ColorPresets.plist`.
    pub itermcolors: Option<PathBuf>,
    /// Preset name inside a multi-preset `itermcolors` file.
    pub preset: Option<String>,
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub bold: Option<String>,
    pub cursor: Option<String>,
    pub cursor_text: Option<String>,
    pub selection: Option<String>,
    pub selection_text: Option<String>,
    pub link: Option<String>,
    /// 16 entries: black, red, green, yellow, blue, magenta, cyan, white, then bright variants.
    pub ansi: Option<Vec<String>>,
}

/// Resolved terminal colors.
#[derive(Clone, PartialEq)]
pub struct Palette {
    pub fg: Color32,
    pub bg: Color32,
    pub bold: Color32,
    pub cursor: Color32,
    pub cursor_text: Color32,
    pub selection: Color32,
    pub selection_text: Color32,
    pub link: Color32,
    pub ansi: [Color32; 16],
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// iTerm2's default "Regular" preset (dark and light variants), as shipped in ColorPresets.plist.
const REGULAR_ANSI: [Color32; 16] = [
    rgb(0x14191e),
    rgb(0xb43c2a),
    rgb(0x00c200),
    rgb(0xc7c400),
    rgb(0x2744c7),
    rgb(0xc040be),
    rgb(0x00c5c7),
    rgb(0xc7c7c7),
    rgb(0x686868),
    rgb(0xdd7975),
    rgb(0x58e790),
    rgb(0xece100),
    rgb(0xa7abf2),
    rgb(0xe17ee1),
    rgb(0x60fdff),
    rgb(0xffffff),
];

impl Palette {
    pub fn regular(dark: bool) -> Palette {
        if dark {
            Palette {
                fg: rgb(0xdcdcdc),
                bg: rgb(0x15191f),
                bold: rgb(0xffffff),
                cursor: rgb(0xffffff),
                cursor_text: rgb(0x000000),
                selection: rgb(0xb3d7ff),
                selection_text: rgb(0x000000),
                link: rgb(0x328eee),
                ansi: REGULAR_ANSI,
            }
        } else {
            Palette {
                fg: rgb(0x101010),
                bg: rgb(0xfafafa),
                bold: rgb(0x101010),
                cursor: rgb(0x000000),
                cursor_text: rgb(0xffffff),
                selection: rgb(0xb3d7ff),
                selection_text: rgb(0x000000),
                link: rgb(0x328eee),
                ansi: REGULAR_ANSI,
            }
        }
    }
}

pub fn parse_color(s: &str) -> Option<Color32> {
    let hex = s.trim().strip_prefix('#').unwrap_or(s.trim());
    if hex.len() != 6 {
        return None;
    }
    u32::from_str_radix(hex, 16).ok().map(rgb)
}

/// Reads an `.itermcolors` dictionary. Keys may carry a ` (Dark)`/` (Light)` suffix
/// (iTerm2 3.5+ adaptive presets); the variant matching `dark` wins.
fn apply_itermcolors(palette: &mut Palette, dict: &plist::Dictionary, dark: bool) {
    let variant = if dark { " (Dark)" } else { " (Light)" };
    let color = |key: &str| -> Option<Color32> {
        let entry = dict.get(&format!("{key}{variant}")).or_else(|| dict.get(key))?.as_dictionary()?;
        let component = |name: &str| entry.get(name).and_then(|v| v.as_real()).unwrap_or(0.0);
        let to_u8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Some(Color32::from_rgb(
            to_u8(component("Red Component")),
            to_u8(component("Green Component")),
            to_u8(component("Blue Component")),
        ))
    };
    let set = |target: &mut Color32, key: &str| {
        if let Some(c) = color(key) {
            *target = c;
        }
    };
    set(&mut palette.fg, "Foreground Color");
    set(&mut palette.bg, "Background Color");
    set(&mut palette.bold, "Bold Color");
    set(&mut palette.cursor, "Cursor Color");
    set(&mut palette.cursor_text, "Cursor Text Color");
    set(&mut palette.selection, "Selection Color");
    set(&mut palette.selection_text, "Selected Text Color");
    set(&mut palette.link, "Link Color");
    for (i, slot) in palette.ansi.iter_mut().enumerate() {
        set(slot, &format!("Ansi {i} Color"));
    }
}

impl ColorConfig {
    pub fn palette(&self, dark: bool) -> Result<Palette, String> {
        let mut p = Palette::regular(dark);
        if let Some(path) = &self.itermcolors {
            let path = expand_home(path);
            let value = plist::Value::from_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let root = value.as_dictionary().ok_or("itermcolors: not a dictionary")?;
            let dict = if root.contains_key("Foreground Color") || root.contains_key("Foreground Color (Dark)") {
                root
            } else {
                let name = self.preset.as_deref().ok_or_else(|| {
                    let names: Vec<&str> = root.keys().map(String::as_str).collect();
                    format!(
                        "{} holds several presets; set colors.preset to one of: {}",
                        path.display(),
                        names.join(", ")
                    )
                })?;
                root.get(name)
                    .and_then(|v| v.as_dictionary())
                    .ok_or_else(|| format!("preset {name:?} not found in {}", path.display()))?
            };
            apply_itermcolors(&mut p, dict, dark);
        }
        let set = |target: &mut Color32, value: &Option<String>, name: &str| -> Result<(), String> {
            if let Some(v) = value {
                *target = parse_color(v).ok_or_else(|| format!("colors.{name}: invalid color {v:?}"))?;
            }
            Ok(())
        };
        set(&mut p.fg, &self.foreground, "foreground")?;
        set(&mut p.bg, &self.background, "background")?;
        set(&mut p.bold, &self.bold, "bold")?;
        set(&mut p.cursor, &self.cursor, "cursor")?;
        set(&mut p.cursor_text, &self.cursor_text, "cursor_text")?;
        set(&mut p.selection, &self.selection, "selection")?;
        set(&mut p.selection_text, &self.selection_text, "selection_text")?;
        set(&mut p.link, &self.link, "link")?;
        if let Some(ansi) = &self.ansi {
            if ansi.len() != 16 {
                return Err(format!("colors.ansi needs 16 entries, got {}", ansi.len()));
            }
            for (i, v) in ansi.iter().enumerate() {
                p.ansi[i] = parse_color(v).ok_or_else(|| format!("colors.ansi[{i}]: invalid color {v:?}"))?;
            }
        }
        Ok(p)
    }
}

pub fn expand_home(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), std::env::var_os("HOME")) {
        (Ok(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => path.to_path_buf(),
    }
}

pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("rterm/config.toml"))
}

/// Loads the config, writing the commented example on first run.
pub fn load(path: &Path) -> Result<Config, String> {
    if !path.exists() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, EXAMPLE);
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let config: Config = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    config.validate()?;
    Ok(config)
}

impl Config {
    fn validate(&self) -> Result<(), String> {
        for (combo, action) in &self.keys {
            KeyCombo::parse(combo)?;
            if action != "none" {
                Action::parse(action)?;
            }
        }
        if !(4.0..=96.0).contains(&self.font.size) {
            return Err(format!("font.size {} is out of range (4–96)", self.font.size));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    NewTab,
    ClosePane,
    Split(Axis),
    Focus(Direction),
    SelectTab(usize),
    NextTab,
    PrevTab,
    Copy,
    Paste,
    PrevPrompt,
    NextPrompt,
    ScrollPageUp,
    ScrollPageDown,
    ScrollTop,
    ScrollBottom,
    FontBigger,
    FontSmaller,
    FontReset,
    ClearBuffer,
    MaximizePane,
    OpenSettings,
    Find,
    FindNext,
    FindPrevious,
    Autocomplete,
}

/// Config name, action and menu label for every bindable action.
pub const ACTIONS: &[(&str, Action, &str)] = &[
    ("new_tab", Action::NewTab, "New Tab"),
    ("close_pane", Action::ClosePane, "Close Pane"),
    ("split_vertical", Action::Split(Axis::Horizontal), "Split Pane Vertically"),
    ("split_horizontal", Action::Split(Axis::Vertical), "Split Pane Horizontally"),
    ("maximize_pane", Action::MaximizePane, "Maximize Active Pane"),
    ("focus_left", Action::Focus(Direction::Left), "Select Pane Left"),
    ("focus_right", Action::Focus(Direction::Right), "Select Pane Right"),
    ("focus_up", Action::Focus(Direction::Up), "Select Pane Above"),
    ("focus_down", Action::Focus(Direction::Down), "Select Pane Below"),
    ("next_tab", Action::NextTab, "Next Tab"),
    ("prev_tab", Action::PrevTab, "Previous Tab"),
    ("select_tab_1", Action::SelectTab(0), "Select Tab 1"),
    ("select_tab_2", Action::SelectTab(1), "Select Tab 2"),
    ("select_tab_3", Action::SelectTab(2), "Select Tab 3"),
    ("select_tab_4", Action::SelectTab(3), "Select Tab 4"),
    ("select_tab_5", Action::SelectTab(4), "Select Tab 5"),
    ("select_tab_6", Action::SelectTab(5), "Select Tab 6"),
    ("select_tab_7", Action::SelectTab(6), "Select Tab 7"),
    ("select_tab_8", Action::SelectTab(7), "Select Tab 8"),
    ("select_tab_9", Action::SelectTab(8), "Select Tab 9"),
    ("copy", Action::Copy, "Copy"),
    ("paste", Action::Paste, "Paste"),
    ("find", Action::Find, "Find…"),
    ("find_next", Action::FindNext, "Find Next (down)"),
    ("find_previous", Action::FindPrevious, "Find Previous (up)"),
    ("autocomplete", Action::Autocomplete, "Autocomplete…"),
    ("prev_prompt", Action::PrevPrompt, "Jump to Previous Prompt"),
    ("next_prompt", Action::NextPrompt, "Jump to Next Prompt"),
    ("scroll_page_up", Action::ScrollPageUp, "Scroll Page Up"),
    ("scroll_page_down", Action::ScrollPageDown, "Scroll Page Down"),
    ("scroll_top", Action::ScrollTop, "Scroll to Top"),
    ("scroll_bottom", Action::ScrollBottom, "Scroll to Bottom"),
    ("font_bigger", Action::FontBigger, "Make Text Bigger"),
    ("font_smaller", Action::FontSmaller, "Make Text Smaller"),
    ("font_reset", Action::FontReset, "Default Text Size"),
    ("clear_buffer", Action::ClearBuffer, "Clear Buffer"),
    ("open_settings", Action::OpenSettings, "Settings…"),
];

impl Action {
    pub fn parse(name: &str) -> Result<Action, String> {
        ACTIONS
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, a, _)| *a)
            .ok_or_else(|| format!("unknown action {name:?}"))
    }

    pub fn name(self) -> &'static str {
        ACTIONS.iter().find(|(_, a, _)| *a == self).map(|(n, _, _)| *n).unwrap_or("none")
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct KeyCombo {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Short config names for keys whose egui names are long or symbolic.
const KEY_ALIASES: &[(&str, Key)] = &[
    ("up", Key::ArrowUp),
    ("down", Key::ArrowDown),
    ("left", Key::ArrowLeft),
    ("right", Key::ArrowRight),
    ("=", Key::Equals),
    ("-", Key::Minus),
    (",", Key::Comma),
    (".", Key::Period),
    ("/", Key::Slash),
    (";", Key::Semicolon),
    ("return", Key::Enter),
];

impl KeyCombo {
    pub fn parse(s: &str) -> Result<KeyCombo, String> {
        let mut combo = KeyCombo { key: Key::Escape, ctrl: false, shift: false, alt: false };
        let mut key = None;
        for part in s.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => combo.ctrl = true,
                "shift" => combo.shift = true,
                "alt" | "option" | "meta" => combo.alt = true,
                "" => key = Some(Key::Plus),
                other => {
                    let found = KEY_ALIASES
                        .iter()
                        .find(|(alias, _)| *alias == other)
                        .map(|(_, k)| *k)
                        .or_else(|| Key::ALL.iter().copied().find(|k| k.name().eq_ignore_ascii_case(other)));
                    key = Some(found.ok_or_else(|| format!("unknown key {part:?} in {s:?}"))?);
                }
            }
        }
        combo.key = key.ok_or_else(|| format!("no key in {s:?}"))?;
        Ok(combo)
    }

    pub fn from_event(key: Key, m: Modifiers) -> KeyCombo {
        KeyCombo { key, ctrl: m.ctrl, shift: m.shift, alt: m.alt }
    }

    pub fn matches(&self, key: Key, m: Modifiers) -> bool {
        self.key == key && self.ctrl == m.ctrl && self.shift == m.shift && self.alt == m.alt
    }

    /// The `[keys]` spelling, e.g. `ctrl+shift+left`; round-trips through `parse`.
    pub fn to_config_string(self) -> String {
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "ctrl+"), (self.alt, "alt+"), (self.shift, "shift+")] {
            if on {
                s.push_str(name);
            }
        }
        match KEY_ALIASES.iter().find(|(_, k)| *k == self.key) {
            Some((alias, _)) => s.push_str(alias),
            None if self.key == Key::Plus => s.push_str("plus"),
            None => s.push_str(&self.key.name().to_ascii_lowercase()),
        }
        s
    }

    /// Human-readable label, e.g. "Ctrl+Shift+T".
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        for (on, name) in [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift")] {
            if on {
                parts.push(name.to_owned());
            }
        }
        // Spelled out rather than symbols like "⏴", which many UI fonts lack.
        let key = match self.key {
            Key::ArrowLeft => "Left",
            Key::ArrowRight => "Right",
            Key::ArrowUp => "Up",
            Key::ArrowDown => "Down",
            Key::Plus => "+",
            k => KEY_ALIASES
                .iter()
                .find(|(alias, a)| *a == k && alias.len() == 1)
                .map(|(alias, _)| *alias)
                .unwrap_or(k.name()),
        };
        parts.push(key.to_owned());
        parts.join("+")
    }
}

/// iTerm2's Cmd shortcuts mapped to Ctrl+Shift (plain Ctrl keys belong to the shell).
const DEFAULT_KEYS: &[(&str, &str)] = &[
    ("ctrl+shift+t", "new_tab"),
    ("ctrl+shift+w", "close_pane"),
    ("ctrl+shift+d", "split_vertical"),
    ("ctrl+shift+e", "split_horizontal"),
    ("ctrl+alt+left", "focus_left"),
    ("ctrl+alt+right", "focus_right"),
    ("ctrl+alt+up", "focus_up"),
    ("ctrl+alt+down", "focus_down"),
    ("ctrl+pagedown", "next_tab"),
    ("ctrl+pageup", "prev_tab"),
    ("ctrl+tab", "next_tab"),
    ("ctrl+shift+tab", "prev_tab"),
    ("ctrl+shift+c", "copy"),
    ("ctrl+shift+v", "paste"),
    ("ctrl+shift+up", "prev_prompt"),
    ("ctrl+shift+down", "next_prompt"),
    ("shift+pageup", "scroll_page_up"),
    ("shift+pagedown", "scroll_page_down"),
    ("shift+home", "scroll_top"),
    ("shift+end", "scroll_bottom"),
    ("ctrl+=", "font_bigger"),
    ("ctrl+shift+=", "font_bigger"),
    ("ctrl+shift+plus", "font_bigger"),
    ("ctrl+-", "font_smaller"),
    ("ctrl+0", "font_reset"),
    ("ctrl+shift+k", "clear_buffer"),
    ("ctrl+shift+enter", "maximize_pane"),
    ("ctrl+,", "open_settings"),
    ("ctrl+shift+f", "find"),
    ("f3", "find_next"),
    ("shift+f3", "find_previous"),
    ("ctrl+;", "autocomplete"),
    ("alt+1", "select_tab_1"),
    ("alt+2", "select_tab_2"),
    ("alt+3", "select_tab_3"),
    ("alt+4", "select_tab_4"),
    ("alt+5", "select_tab_5"),
    ("alt+6", "select_tab_6"),
    ("alt+7", "select_tab_7"),
    ("alt+8", "select_tab_8"),
    ("alt+9", "select_tab_9"),
];

pub struct Bindings(Vec<(KeyCombo, Option<Action>)>);

impl Bindings {
    /// Defaults plus user overrides (validated in `load`, so parse errors are skipped here).
    pub fn new(user: &BTreeMap<String, String>) -> Bindings {
        let mut list: Vec<(KeyCombo, Option<Action>)> = Vec::new();
        let mut bind = |combo: &str, action: &str| {
            let Ok(combo) = KeyCombo::parse(combo) else { return };
            let action = if action == "none" { None } else { Action::parse(action).ok() };
            list.retain(|(c, _)| *c != combo);
            list.push((combo, action));
        };
        for (combo, action) in DEFAULT_KEYS {
            bind(combo, action);
        }
        for (combo, action) in user {
            bind(combo, action);
        }
        Bindings(list)
    }

    /// `Some(None)` means the combo is explicitly unbound, so it goes to the shell.
    pub fn lookup(&self, key: Key, m: Modifiers) -> Option<Action> {
        self.0.iter().find(|(c, _)| c.matches(key, m)).and_then(|(_, a)| *a)
    }

    /// Display label for the first combo bound to `action`, e.g. "Alt+1".
    pub fn label(&self, action: Action) -> Option<String> {
        self.combos(action).first().map(KeyCombo::label)
    }

    /// Every combo currently bound to `action`.
    pub fn combos(&self, action: Action) -> Vec<KeyCombo> {
        self.0.iter().filter(|(_, a)| *a == Some(action)).map(|(c, _)| *c).collect()
    }
}

impl ThemeChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeChoice::Auto => "auto",
            ThemeChoice::Dark => "dark",
            ThemeChoice::Light => "light",
        }
    }
}

impl Decorations {
    pub fn as_str(self) -> &'static str {
        match self {
            Decorations::Iterm => "iterm",
            Decorations::System => "system",
        }
    }
}

impl CursorStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            CursorStyle::Block => "block",
            CursorStyle::Beam => "beam",
            CursorStyle::Underline => "underline",
        }
    }
}

pub fn color_hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// Writes `config` to `path`, editing the existing document in place so comments and
/// layout survive (settings window changes).
pub fn save(path: &Path, c: &Config) -> Result<(), String> {
    use toml_edit::{Array, DocumentMut, Item, value};

    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc: DocumentMut = text.parse().map_err(|e| format!("{}: {e}", path.display()))?;
    // f32 -> f64 without float noise like 0.4000000059604645.
    let num = |v: f32| ((v as f64) * 1000.0).round() / 1000.0;
    let set_opt = |doc: &mut DocumentMut, table: &str, key: &str, v: Option<String>| match v {
        Some(v) => doc[table][key] = value(v),
        None => {
            if let Some(t) = doc.get_mut(table).and_then(Item::as_table_like_mut) {
                t.remove(key);
            }
        }
    };

    doc["font"]["family"] = value(c.font.family.clone());
    doc["font"]["size"] = value(num(c.font.size));
    doc["font"]["line_height"] = value(num(c.font.line_height));
    doc["font"]["use_bold_font"] = value(c.font.use_bold_font);

    let w = &c.window;
    doc["window"]["theme"] = value(w.theme.as_str());
    doc["window"]["decorations"] = value(w.decorations.as_str());
    doc["window"]["columns"] = value(w.columns as i64);
    doc["window"]["rows"] = value(w.rows as i64);
    doc["window"]["show_tab_bar_with_one_tab"] = value(w.show_tab_bar_with_one_tab);
    doc["window"]["pane_title_bars"] = value(w.pane_title_bars);
    doc["window"]["dim_inactive_panes"] = value(w.dim_inactive_panes);
    doc["window"]["dim_amount"] = value(num(w.dim_amount));
    doc["window"]["corner_radius"] = value(num(w.corner_radius));

    let t = &c.terminal;
    set_opt(&mut doc, "terminal", "shell", t.shell.clone());
    doc["terminal"]["shell_integration"] = value(t.shell_integration);
    doc["terminal"]["scrollback"] = value(t.scrollback as i64);
    doc["terminal"]["cursor"] = value(t.cursor.as_str());
    doc["terminal"]["copy_on_select"] = value(t.copy_on_select);
    doc["terminal"]["bold_is_bright"] = value(t.bold_is_bright);
    doc["terminal"]["mouse_reporting"] = value(t.mouse_reporting);

    let k = &c.colors;
    set_opt(&mut doc, "colors", "itermcolors", k.itermcolors.as_ref().map(|p| p.display().to_string()));
    set_opt(&mut doc, "colors", "preset", k.preset.clone());
    for (key, v) in [
        ("foreground", &k.foreground),
        ("background", &k.background),
        ("bold", &k.bold),
        ("cursor", &k.cursor),
        ("cursor_text", &k.cursor_text),
        ("selection", &k.selection),
        ("selection_text", &k.selection_text),
        ("link", &k.link),
    ] {
        set_opt(&mut doc, "colors", key, v.clone());
    }
    match &k.ansi {
        Some(ansi) => doc["colors"]["ansi"] = value(ansi.iter().map(String::as_str).collect::<Array>()),
        None => set_opt(&mut doc, "colors", "ansi", None),
    }

    if doc.get("keys").and_then(Item::as_table).is_none() {
        doc["keys"] = toml_edit::table();
    }
    if let Some(keys) = doc["keys"].as_table_mut() {
        keys.clear();
        for (combo, action) in &c.keys {
            keys.insert(combo, value(action.clone()));
        }
    }

    std::fs::write(path, doc.to_string()).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_round_trip() {
        for s in ["ctrl+shift+t", "ctrl+alt+left", "ctrl+,", "shift+pageup", "alt+1", "ctrl+=", "ctrl+shift+plus", "f5"]
        {
            let c = KeyCombo::parse(s).unwrap();
            assert_eq!(KeyCombo::parse(&c.to_config_string()).unwrap(), c, "{s}");
        }
        assert_eq!(Action::parse("open_settings").unwrap().name(), "open_settings");
    }

    #[test]
    fn save_keeps_comments_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("rterm-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, EXAMPLE).unwrap();
        let mut c: Config = toml::from_str(EXAMPLE).unwrap();
        c.font.size = 16.5;
        c.window.theme = ThemeChoice::Light;
        c.colors.background = Some("#101010".into());
        c.keys.insert("ctrl+shift+n".into(), "new_tab".into());
        save(&path, &c).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# Multiplier on the font's natural line height."));
        let back = load(&path).unwrap();
        assert_eq!(back.font.size, 16.5);
        assert!(back.window.theme == ThemeChoice::Light);
        assert_eq!(back.colors.background.as_deref(), Some("#101010"));
        assert_eq!(back.keys.get("ctrl+shift+n").map(String::as_str), Some("new_tab"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn example_config_parses() {
        let config: Config = toml::from_str(EXAMPLE).expect("config.example.toml must parse");
        config.validate().unwrap();
    }

    #[test]
    fn key_combos() {
        let c = KeyCombo::parse("ctrl+shift+t").unwrap();
        assert!(c.matches(Key::T, Modifiers::CTRL | Modifiers::SHIFT));
        assert_eq!(KeyCombo::parse("ctrl+=").unwrap().key, Key::Equals);
        assert_eq!(KeyCombo::parse("alt+left").unwrap().key, Key::ArrowLeft);
        assert!(KeyCombo::parse("ctrl+nope").is_err());
    }

    #[test]
    fn user_bindings_override_defaults() {
        let mut user = BTreeMap::new();
        user.insert("ctrl+shift+t".to_owned(), "none".to_owned());
        user.insert("ctrl+shift+n".to_owned(), "new_tab".to_owned());
        let b = Bindings::new(&user);
        assert_eq!(b.lookup(Key::T, Modifiers::CTRL | Modifiers::SHIFT), None);
        assert_eq!(b.lookup(Key::N, Modifiers::CTRL | Modifiers::SHIFT), Some(Action::NewTab));
        assert_eq!(b.label(Action::SelectTab(0)).as_deref(), Some("Alt+1"));
    }

    #[test]
    fn palette_overrides() {
        let colors = ColorConfig { background: Some("#101010".into()), ..Default::default() };
        assert_eq!(colors.palette(true).unwrap().bg, Color32::from_rgb(0x10, 0x10, 0x10));
        let bad = ColorConfig { ansi: Some(vec!["#000000".into()]), ..Default::default() };
        assert!(bad.palette(true).is_err());
    }
}
