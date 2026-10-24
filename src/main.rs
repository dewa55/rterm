//! rterm: an iTerm2-inspired terminal for Linux with tabs, split panes and shell integration.

mod autocomplete;
mod chrome;
mod config;
mod find;
mod fonts;
mod input;
mod layout;
mod pane;
mod render;
mod script;
mod settings;
mod shell_integration;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime};

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, Event, EventFilter, FontFamily, FontId, Id, Key, Modifiers,
    MouseWheelUnit, PointerButton, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, ViewportCommand, pos2, vec2,
};

use chrome::{ChromeColors, PaneTitleAction, TabBarAction, TabInfo};
use config::{Action, Bindings, Config, Decorations, Palette, ThemeChoice};
use input::{MouseAction, MouseButton};
use layout::{Axis, Direction, Node, PaneId};
use pane::{Pane, SpawnOptions};
use render::{LinkSpan, PaneView, Style};
use shell_integration::ShellState;

/// Visible divider width; the draggable area is wider.
const DIVIDER: f32 = 1.0;

fn main() -> eframe::Result {
    let config_path = config::config_path();
    let (config, error) = match config_path.as_deref().map(config::load) {
        Some(Ok(c)) => (c, None),
        Some(Err(e)) => (Config::default(), Some(e)),
        None => (Config::default(), None),
    };
    let iterm = config.window.decorations == Decorations::Iterm;
    // Approximate until the real font metrics are known (corrected on the first frames).
    let cell = vec2(config.font.size * 0.6, config.font.size * 1.25 * config.font.line_height);
    let size = render::pane_size_for(config.window.columns, config.window.rows, cell)
        + vec2(0.0, if iterm { chrome::TITLE_BAR_HEIGHT } else { 0.0 });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rterm")
            .with_app_id("rterm")
            .with_inner_size(size)
            .with_min_inner_size([320.0, 200.0])
            .with_decorations(!iterm)
            .with_transparent(iterm)
            .with_icon(app_icon()),
        // Ctrl+C/V/X must reach the shell; copy/paste live on Ctrl+Shift+C/V.
        clipboard_shortcuts: false,
        ..Default::default()
    };
    eframe::run_native(
        "rterm",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, config, config_path, error)?))),
    )
}

/// The window icon, embedded so the binary works without installed assets.
fn app_icon() -> egui::IconData {
    let image = image::load_from_memory(include_bytes!("../assets/rterm-256.png"))
        .expect("embedded icon is a valid PNG")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData { rgba: image.into_raw(), width, height }
}

struct Tab {
    root: Node,
    active: PaneId,
    /// Pane shown alone (iTerm2's "Maximize Active Pane").
    zoomed: Option<PaneId>,
    /// Pane body rects from the last frame, used for directional focus.
    rects: Vec<(PaneId, Rect)>,
}

impl Tab {
    fn new(id: PaneId) -> Tab {
        Tab { root: Node::Leaf(id), active: id, zoomed: None, rects: Vec::new() }
    }

    fn panes(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        self.root.panes(&mut ids);
        ids
    }
}

struct App {
    panes: HashMap<PaneId, Pane>,
    tabs: Vec<Tab>,
    current: usize,
    next_id: PaneId,
    ctx: egui::Context,
    integration_dir: Option<PathBuf>,

    config: Config,
    config_path: Option<PathBuf>,
    config_mtime: Option<SystemTime>,
    last_config_check: Instant,
    dark: bool,
    palette: Palette,
    chrome: ChromeColors,
    bindings: Bindings,
    font_size: f32,
    loaded_font: Option<(String, bool)>,
    /// Exact window size is applied once real font metrics are known.
    sized_window: u8,

    /// Pane receiving the current selection drag.
    selecting: Option<PaneId>,
    /// Pane that received a reported mouse press (gets motion and the release).
    mouse_capture: Option<PaneId>,
    notice: Option<(String, Instant)>,
    script: Option<script::Script>,
    settings: settings::Settings,
    find: find::Find,
    autocomplete: autocomplete::Autocomplete,
    /// Bottom-left of the active pane's cursor cell (anchors the autocomplete popup).
    cursor_anchor: Option<egui::Pos2>,
    /// Settings-window changes are written to the config file shortly after the last edit.
    save_due: Option<Instant>,
}

impl App {
    fn new(
        ctx: &egui::Context,
        config: Config,
        config_path: Option<PathBuf>,
        error: Option<String>,
    ) -> Result<App, Box<dyn std::error::Error + Send + Sync>> {
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        // Development: draw secondary windows (Settings) inside the main one so scripted
        // screenshots can capture them.
        if std::env::var_os("RTERM_EMBED_VIEWPORTS").is_some() {
            ctx.set_embed_viewports(true);
        }
        let integration_dir = shell_integration::install_scripts();
        let mut app = App {
            panes: HashMap::new(),
            tabs: Vec::new(),
            current: 0,
            next_id: 1,
            ctx: ctx.clone(),
            integration_dir,
            config_mtime: config_path.as_deref().and_then(mtime),
            config_path,
            last_config_check: Instant::now(),
            dark: true,
            palette: Palette::regular(true),
            chrome: ChromeColors::new(true),
            bindings: Bindings::new(&config.keys),
            font_size: config.font.size,
            loaded_font: None,
            sized_window: 0,
            config,
            selecting: None,
            mouse_capture: None,
            notice: None,
            script: script::Script::from_env(),
            settings: settings::Settings::new(),
            find: find::Find::default(),
            autocomplete: autocomplete::Autocomplete::new(),
            cursor_anchor: None,
            save_due: None,
        };
        app.apply_config();
        if let Some(e) = error {
            app.notify(format!("Config error: {e}"));
        }
        let id = app.spawn_pane(std::env::current_dir().ok())?;
        app.tabs.push(Tab::new(id));
        Ok(app)
    }

    fn notify(&mut self, message: String) {
        eprintln!("rterm: {message}");
        self.notice = Some((message, Instant::now()));
    }

    fn resolve_dark(&self) -> bool {
        match self.config.window.theme {
            ThemeChoice::Dark => true,
            ThemeChoice::Light => false,
            ThemeChoice::Auto => self.ctx.system_theme() != Some(egui::Theme::Light),
        }
    }

    /// Applies `self.config` to colors, fonts, keys and running panes.
    fn apply_config(&mut self) {
        self.dark = self.resolve_dark();
        self.chrome = ChromeColors::new(self.dark);
        self.ctx.set_visuals(if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        self.palette = match self.config.colors.palette(self.dark) {
            Ok(p) => p,
            Err(e) => {
                self.notify(format!("Config error: {e}"));
                Palette::regular(self.dark)
            }
        };
        self.bindings = Bindings::new(&self.config.keys);
        self.font_size = self.config.font.size;
        let font_key = (self.config.font.family.clone(), self.config.font.use_bold_font);
        if self.loaded_font.as_ref() != Some(&font_key) {
            if let Some(warning) = fonts::install(&self.ctx, &font_key.0, font_key.1) {
                self.notify(warning);
            }
            self.loaded_font = Some(font_key);
        }
        let term_config = pane::term_config(self.config.terminal.scrollback, self.config.terminal.cursor);
        for pane in self.panes.values() {
            pane.engine().term.set_options(term_config.clone());
        }
    }

    fn check_config_reload(&mut self) {
        if self.last_config_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_config_check = Instant::now();
        let Some(path) = self.config_path.clone() else { return };
        let current = mtime(&path);
        if current == self.config_mtime {
            return;
        }
        self.config_mtime = current;
        match config::load(&path) {
            Ok(c) => {
                if self.settings.open {
                    self.settings.draft = c.clone();
                }
                self.config = c;
                self.apply_config();
                self.notify("Config reloaded".to_owned());
            }
            Err(e) => self.notify(format!("Config error: {e}")),
        }
    }

    fn spawn_pane(&mut self, cwd: Option<PathBuf>) -> anyhow::Result<PaneId> {
        let id = self.next_id;
        self.next_id += 1;
        let terminal = &self.config.terminal;
        let opts = SpawnOptions {
            shell: terminal.shell.as_deref(),
            integration_dir: self.integration_dir.as_deref().filter(|_| terminal.shell_integration),
            term_config: pane::term_config(terminal.scrollback, terminal.cursor),
            size: (self.config.window.columns, self.config.window.rows),
        };
        let pane = Pane::spawn(id, &self.ctx, cwd.as_deref(), opts)?;
        self.panes.insert(id, pane);
        Ok(id)
    }

    fn active_pane(&self) -> Option<&Pane> {
        self.panes.get(&self.tabs.get(self.current)?.active)
    }

    fn active_cwd(&self) -> Option<PathBuf> {
        self.active_pane().and_then(Pane::working_dir)
    }

    fn new_tab(&mut self) {
        match self.spawn_pane(self.active_cwd()) {
            Ok(id) => {
                let at = (self.current + 1).min(self.tabs.len());
                self.tabs.insert(at, Tab::new(id));
                self.current = at;
            }
            Err(e) => self.notify(format!("Failed to open tab: {e:#}")),
        }
    }

    fn split(&mut self, axis: Axis) {
        let Some(target) = self.tabs.get(self.current).map(|t| t.active) else { return };
        match self.spawn_pane(self.active_cwd()) {
            Ok(id) => {
                let tab = &mut self.tabs[self.current];
                tab.root.split(target, id, axis);
                tab.active = id;
                tab.zoomed = None;
            }
            Err(e) => self.notify(format!("Failed to split: {e:#}")),
        }
    }

    fn close_pane(&mut self, id: PaneId) {
        self.panes.remove(&id);
        if self.selecting == Some(id) {
            self.selecting = None;
        }
        if self.mouse_capture == Some(id) {
            self.mouse_capture = None;
        }
        let Some(index) = self.tabs.iter().position(|t| t.panes().contains(&id)) else { return };
        let tab = &mut self.tabs[index];
        if tab.zoomed == Some(id) {
            tab.zoomed = None;
        }
        let neighbor = [Direction::Left, Direction::Up, Direction::Right, Direction::Down]
            .into_iter()
            .find_map(|d| layout::neighbor(&tab.rects, id, d));
        let root = std::mem::replace(&mut tab.root, Node::Leaf(0));
        match root.remove(id) {
            Some(root) => {
                if tab.active == id {
                    tab.active = neighbor.unwrap_or_else(|| root.first_pane());
                }
                tab.root = root;
            }
            None => {
                self.tabs.remove(index);
                if index < self.current || self.current >= self.tabs.len() {
                    self.current = self.current.saturating_sub(1);
                }
            }
        }
    }

    fn close_tab(&mut self, index: usize) {
        for id in self.tabs[index].panes() {
            self.close_pane(id);
        }
    }

    fn run(&mut self, action: Action) {
        if action != Action::Autocomplete {
            self.autocomplete.close();
        }
        match action {
            Action::NewTab => self.new_tab(),
            Action::ClosePane => {
                if let Some(id) = self.tabs.get(self.current).map(|t| t.active) {
                    self.close_pane(id);
                }
            }
            Action::Split(axis) => self.split(axis),
            Action::Focus(dir) => {
                if let Some(tab) = self.tabs.get_mut(self.current)
                    && let Some(id) = layout::neighbor(&tab.rects, tab.active, dir)
                {
                    tab.active = id;
                }
            }
            Action::SelectTab(i) => {
                if i < self.tabs.len() {
                    self.current = i;
                }
            }
            Action::NextTab if !self.tabs.is_empty() => self.current = (self.current + 1) % self.tabs.len(),
            Action::PrevTab if !self.tabs.is_empty() => {
                self.current = (self.current + self.tabs.len() - 1) % self.tabs.len();
            }
            Action::NextTab | Action::PrevTab => {}
            Action::Copy => {
                if let Some(text) = self.active_pane().and_then(|p| p.engine().term.selection_to_string()) {
                    self.ctx.copy_text(text);
                }
            }
            Action::Paste => self.ctx.send_viewport_cmd(ViewportCommand::RequestPaste),
            Action::PrevPrompt => self.jump_to_prompt(true),
            Action::NextPrompt => self.jump_to_prompt(false),
            Action::ScrollPageUp => self.scroll_active(Scroll::PageUp),
            Action::ScrollPageDown => self.scroll_active(Scroll::PageDown),
            Action::ScrollTop => self.scroll_active(Scroll::Top),
            Action::ScrollBottom => self.scroll_active(Scroll::Bottom),
            Action::FontBigger => self.font_size = (self.font_size + 1.0).min(96.0),
            Action::FontSmaller => self.font_size = (self.font_size - 1.0).max(4.0),
            Action::FontReset => self.font_size = self.config.font.size,
            Action::ClearBuffer => {
                if let Some(pane) = self.active_pane() {
                    pane.clear_buffer();
                }
            }
            Action::OpenSettings => self.settings.open_with(&self.config),
            Action::Find => self.find.open(),
            Action::Autocomplete => {
                let active = self.tabs.get(self.current).map(|t| t.active);
                if let Some(id) = active
                    && let Some(pane) = self.panes.get(&id)
                {
                    let mut engine = pane.engine();
                    engine.term.scroll_display(Scroll::Bottom);
                    self.autocomplete.start(id, &engine);
                }
            }
            Action::FindNext | Action::FindPrevious if !self.find.open => self.find.open(),
            Action::FindNext => self.find_step(false),
            Action::FindPrevious => self.find_step(true),
            Action::MaximizePane => {
                if let Some(tab) = self.tabs.get_mut(self.current) {
                    tab.zoomed = match tab.zoomed {
                        Some(_) => None,
                        None if tab.panes().len() > 1 => Some(tab.active),
                        None => None,
                    };
                }
            }
        }
    }

    /// Keys while the autocomplete popup is open. Returns true when the key was consumed;
    /// other keys close the popup and are handled normally.
    fn autocomplete_key(&mut self, key: Key, modifiers: Modifiers) -> bool {
        let plain = !modifiers.ctrl && !modifiers.alt;
        match key {
            Key::Escape if plain => self.autocomplete.close(),
            Key::Enter | Key::Tab if plain => self.accept_completion(None),
            Key::ArrowUp if plain => self.autocomplete.move_selection(-1),
            Key::ArrowDown if plain => self.autocomplete.move_selection(1),
            Key::PageUp if plain => self.autocomplete.move_selection(-10),
            Key::PageDown if plain => self.autocomplete.move_selection(10),
            // With no filter left, Backspace closes the popup and edits the shell line.
            Key::Backspace if plain => return self.autocomplete.backspace(),
            _ => {
                let mode = self.active_pane().map(|p| *p.engine().term.mode()).unwrap_or(TermMode::empty());
                // Printable keys arrive again as Event::Text, which filters the list.
                if plain && input::key_bytes(key, modifiers, mode).is_none() {
                    return true;
                }
                self.autocomplete.close();
                return false;
            }
        }
        true
    }

    fn accept_completion(&mut self, index: Option<usize>) {
        let pane_id = self.autocomplete.pane();
        if let Some(bytes) = self.autocomplete.accept(index)
            && let Some(pane) = self.panes.get(&pane_id)
        {
            pane.input(&bytes);
        }
    }

    /// Moves to the previous (up) or next (down) find match and scrolls to it.
    fn find_step(&mut self, up: bool) {
        if !self.find.step(up) {
            return;
        }
        let active = self.tabs.get(self.current).map(|t| t.active);
        if let Some(pane) = active.and_then(|id| self.panes.get(&id)) {
            self.find.reveal(&mut pane.engine());
        }
    }

    fn scroll_active(&self, scroll: Scroll) {
        if let Some(pane) = self.active_pane() {
            pane.engine().term.scroll_display(scroll);
        }
    }

    /// Scrolls so the previous/next shell prompt is at the top of the pane (iTerm2's Cmd+Shift+Up/Down).
    fn jump_to_prompt(&self, up: bool) {
        let Some(pane) = self.active_pane() else { return };
        let mut guard = pane.engine();
        let engine = &mut *guard;
        engine.shell.prune(&engine.term);
        let top = ShellState::viewport_top(&engine.term);
        let marks = &engine.shell.marks;
        let target = if up { marks.iter().rev().find(|m| m.line < top) } else { marks.iter().find(|m| m.line > top) };
        match target {
            Some(mark) => {
                let history = engine.term.grid().history_size() as i64;
                let offset = (history - mark.line).clamp(0, history);
                let delta = offset - engine.term.grid().display_offset() as i64;
                engine.term.scroll_display(Scroll::Delta(delta as i32));
            }
            None if !up => engine.term.scroll_display(Scroll::Bottom),
            None => {}
        }
    }

    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        // egui sends Key and then Text for the same key press; drop the Text when the key
        // was already handled (shortcuts, Alt+letter).
        let mut swallow_text = false;
        for event in events {
            match event {
                Event::Key { key, pressed: true, modifiers, .. } => {
                    swallow_text = false;
                    if self.autocomplete.open && self.autocomplete_key(key, modifiers) {
                        continue;
                    }
                    if let Some(action) = self.bindings.lookup(key, modifiers) {
                        self.run(action);
                        swallow_text = true;
                        continue;
                    }
                    if self.find.open {
                        // The find field gets typing; only its own keys are handled here.
                        let plain = !modifiers.ctrl && !modifiers.alt;
                        match key {
                            Key::Escape if plain => self.find.close(),
                            Key::Enter if plain => self.find_step(!modifiers.shift),
                            Key::V if modifiers.ctrl && !modifiers.alt => {
                                ctx.send_viewport_cmd(ViewportCommand::RequestPaste);
                            }
                            _ => {}
                        }
                        continue;
                    }
                    let Some(pane) = self.active_pane() else { continue };
                    let mode = *pane.engine().term.mode();
                    if let Some(bytes) = input::key_bytes(key, modifiers, mode) {
                        pane.input(&bytes);
                        swallow_text = modifiers.alt;
                    }
                }
                Event::Text(text) => {
                    if std::mem::take(&mut swallow_text) || self.find.open {
                        continue;
                    }
                    // While the popup is open, typing narrows the suggestions.
                    if self.autocomplete.open && self.autocomplete.type_text(&text) {
                        continue;
                    }
                    if let Some(pane) = self.active_pane() {
                        pane.input(text.as_bytes());
                    }
                }
                Event::Paste(text) => {
                    if self.find.open {
                        continue;
                    }
                    self.autocomplete.close();
                    if let Some(pane) = self.active_pane() {
                        let mode = *pane.engine().term.mode();
                        pane.input(&input::paste_bytes(&text, mode));
                    }
                }
                _ => {}
            }
        }
    }

    fn pane_title(&self, id: PaneId) -> String {
        let Some(pane) = self.panes.get(&id) else { return "rterm".to_owned() };
        pane.osc_title().filter(|t| !t.is_empty()).unwrap_or_else(|| {
            let dir = pane.working_dir().map(|d| short_path(&d)).unwrap_or_else(|| "shell".to_owned());
            match pane.foreground_process() {
                Some(process) => format!("{process} — {dir}"),
                None => dir,
            }
        })
    }

    fn style<'a>(&self, palette: &'a Palette, cell: Vec2, row_height: f32) -> Style<'a> {
        Style {
            palette,
            font: FontId::monospace(self.font_size),
            bold_font: FontId::new(self.font_size, FontFamily::Name(fonts::BOLD.into())),
            cell,
            text_offset: ((cell.y - row_height) / 2.0).max(0.0).round(),
            bold_is_bright: self.config.terminal.bold_is_bright,
        }
    }

    /// The mode of `pane` if it should receive mouse events instead of local selection.
    fn mouse_mode(&self, pane: &Pane, modifiers: Modifiers) -> Option<TermMode> {
        let mode = *pane.engine().term.mode();
        (self.config.terminal.mouse_reporting && mode.intersects(TermMode::MOUSE_MODE) && !modifiers.shift)
            .then_some(mode)
    }

    #[allow(clippy::too_many_arguments)]
    fn report_mouse(
        &self,
        id: PaneId,
        rect: Rect,
        cell: Vec2,
        pos: egui::Pos2,
        button: MouseButton,
        action: MouseAction,
        modifiers: Modifiers,
    ) {
        let Some(pane) = self.panes.get(&id) else { return };
        let (col, row, mode) = {
            let engine = pane.engine();
            let (col, row, _) = render::cell_at(rect, cell, pos, &engine);
            (col, row, *engine.term.mode())
        };
        if let Some(bytes) = input::mouse_report(button, action, modifiers, col, row, mode) {
            pane.write(&bytes);
        }
    }

    /// Mouse reporting to applications (vim `mouse=a`, htop, tmux). Returns true when the
    /// pointer is over a pane that grabbed the mouse, so local selection is skipped.
    fn handle_mouse_reporting(&mut self, ui: &Ui, rects: &[(PaneId, Rect)], cell: Vec2) -> bool {
        let events = ui.input(|i| i.events.clone());
        let hover = ui.input(|i| i.pointer.hover_pos());
        let modifiers = ui.input(|i| i.modifiers);
        let pane_at = |pos: egui::Pos2| rects.iter().find(|(_, r)| r.contains(pos)).copied();
        let grabbed = |app: &App, pos: Option<egui::Pos2>, mods: Modifiers| {
            pos.and_then(pane_at)
                .and_then(|(id, r)| app.panes.get(&id).and_then(|p| app.mouse_mode(p, mods)).map(|m| (id, r, m)))
        };
        let mut consumed = self.mouse_capture.is_some() || grabbed(self, hover, modifiers).is_some();

        for event in events {
            match event {
                Event::PointerButton { pos, button, pressed, modifiers } => {
                    let mb = match button {
                        PointerButton::Primary => MouseButton::Left,
                        PointerButton::Middle => MouseButton::Middle,
                        PointerButton::Secondary => MouseButton::Right,
                        _ => continue,
                    };
                    if pressed {
                        let Some((id, r, _)) = grabbed(self, Some(pos), modifiers) else { continue };
                        self.report_mouse(id, r, cell, pos, mb, MouseAction::Press, modifiers);
                        if let Some(pane) = self.panes.get_mut(&id) {
                            pane.mouse_button = Some(mb);
                            pane.engine().term.selection = None;
                        }
                        self.mouse_capture = Some(id);
                        consumed = true;
                    } else if let Some(id) = self.mouse_capture.take()
                        && let Some((_, r)) = rects.iter().find(|(pid, _)| *pid == id)
                    {
                        self.report_mouse(id, *r, cell, pos, mb, MouseAction::Release, modifiers);
                        if let Some(pane) = self.panes.get_mut(&id) {
                            pane.mouse_button = None;
                        }
                        consumed = true;
                    }
                }
                Event::PointerMoved(pos) => {
                    let target = match self.mouse_capture {
                        Some(id) => rects.iter().find(|(pid, _)| *pid == id).copied(),
                        None => pane_at(pos),
                    };
                    let Some((id, r)) = target else { continue };
                    let Some(pane) = self.panes.get(&id) else { continue };
                    let Some(mode) = self.mouse_mode(pane, modifiers) else { continue };
                    let held = pane.mouse_button;
                    let wants = mode.contains(TermMode::MOUSE_MOTION)
                        || (mode.contains(TermMode::MOUSE_DRAG) && held.is_some());
                    let (col, row, _) = render::cell_at(r, cell, pos, &pane.engine());
                    if wants && pane.mouse_cell != Some((col, row)) {
                        self.report_mouse(
                            id,
                            r,
                            cell,
                            pos,
                            held.unwrap_or(MouseButton::None),
                            MouseAction::Motion,
                            modifiers,
                        );
                    }
                    if let Some(pane) = self.panes.get_mut(&id) {
                        pane.mouse_cell = Some((col, row));
                    }
                }
                Event::MouseWheel { unit, delta, modifiers, .. } => {
                    let Some((id, r, _)) = grabbed(self, hover, modifiers) else { continue };
                    let Some(pane) = self.panes.get_mut(&id) else { continue };
                    let rows = pane.engine().term.screen_lines() as f32;
                    pane.scroll_accum += match unit {
                        MouseWheelUnit::Line => delta.y * 3.0,
                        MouseWheelUnit::Point => delta.y / cell.y,
                        MouseWheelUnit::Page => delta.y * rows,
                    };
                    let n = pane.scroll_accum.trunc();
                    pane.scroll_accum -= n;
                    let button = if n > 0.0 { MouseButton::WheelUp } else { MouseButton::WheelDown };
                    for _ in 0..n.abs() as usize {
                        self.report_mouse(
                            id,
                            r,
                            cell,
                            hover.unwrap_or(r.center()),
                            button,
                            MouseAction::Press,
                            modifiers,
                        );
                    }
                    consumed = true;
                }
                _ => {}
            }
        }
        consumed
    }

    fn terminal_area(
        &mut self,
        ui: &mut Ui,
        area: Rect,
        cell: Vec2,
        row_height: f32,
        bottom_radius: u8,
    ) -> Option<Action> {
        let canvas_id = Id::unique("terminal-canvas");
        let canvas = ui.interact(area, canvas_id, Sense::click_and_drag());
        // Keep keyboard focus on the terminal so egui never uses Tab/arrows/Esc for widget
        // navigation; the find bar's text field takes it while open.
        if !self.find.open {
            ui.memory_mut(|m| {
                m.request_focus(canvas_id);
                m.set_focus_lock_filter(
                    canvas_id,
                    EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                );
            });
        }

        let tab = self.tabs.get_mut(self.current)?;
        let mut leaves = Vec::new();
        match tab.zoomed.filter(|z| self.panes.contains_key(z)) {
            Some(z) => leaves.push((z, area)),
            None => layout_node(ui, &mut tab.root, area, Id::unique("split"), &self.chrome, &mut leaves),
        }
        let multi = leaves.len() > 1;
        let show_titles = multi && self.config.window.pane_title_bars;
        // Split each leaf into an optional title bar and the terminal body.
        let bodies: Vec<(PaneId, Rect, Option<Rect>)> = leaves
            .iter()
            .map(|&(id, r)| {
                if show_titles {
                    let title = Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + chrome::PANE_TITLE_HEIGHT));
                    (id, Rect::from_min_max(pos2(r.min.x, title.max.y), r.max), Some(title))
                } else {
                    (id, r, None)
                }
            })
            .collect();
        let rects: Vec<(PaneId, Rect)> = bodies.iter().map(|&(id, r, _)| (id, r)).collect();
        tab.rects = rects.clone();
        let active = tab.active;

        // Focus follows clicks anywhere in a pane.
        let pane_at = |pos: egui::Pos2| rects.iter().find(|(_, r)| r.contains(pos)).copied();
        let pressed = ui.input(|i| i.pointer.any_pressed());
        if pressed
            && canvas.hovered()
            && let Some((id, _)) = ui.input(|i| i.pointer.interact_pos()).and_then(pane_at)
            && let Some(tab) = self.tabs.get_mut(self.current)
        {
            tab.active = id;
        }

        if pressed && canvas.hovered() {
            self.autocomplete.close();
        }
        let reporting = self.handle_mouse_reporting(ui, &rects, cell);
        let modifiers = ui.input(|i| i.modifiers);
        let mut copy = None;
        let mut link_hover: Option<(PaneId, LinkSpan, String)> = None;
        if let Some(hover) = canvas.hover_pos()
            && let Some((id, r)) = pane_at(hover)
            && let Some(pane) = self.panes.get(&id)
        {
            if modifiers.ctrl && !reporting {
                let engine = pane.engine();
                let (col, row, _) = render::cell_at(r, cell, hover, &engine);
                if let Some((url, span)) = render::link_at(&engine, col, row) {
                    link_hover = Some((id, span, url));
                }
            }
            let icon = if link_hover.is_some() {
                CursorIcon::PointingHand
            } else if reporting {
                CursorIcon::Default
            } else {
                CursorIcon::Text
            };
            ui.ctx().set_cursor_icon(icon);
        }

        if !reporting && let Some(pos) = canvas.interact_pointer_pos() {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            if canvas.drag_started_by(PointerButton::Primary)
                && let Some((id, r)) = pane_at(origin)
                && let Some(pane) = self.panes.get(&id)
            {
                let mut engine = pane.engine();
                let (point, side) = render::point_at(r, cell, origin, &engine);
                let ty = if modifiers.alt { SelectionType::Block } else { SelectionType::Simple };
                engine.term.selection = Some(Selection::new(ty, point, side));
                self.selecting = Some(id);
            }
            if let Some(id) = self.selecting
                && let Some((_, r)) = rects.iter().find(|(pid, _)| *pid == id)
                && let Some(pane) = self.panes.get(&id)
            {
                let mut engine = pane.engine();
                let (point, side) = render::point_at(*r, cell, pos, &engine);
                if let Some(sel) = engine.term.selection.as_mut() {
                    sel.update(point, side);
                }
                if canvas.drag_stopped() {
                    copy = engine.term.selection_to_string();
                    self.selecting = None;
                }
            }
            let multi_click = if canvas.triple_clicked() {
                Some(SelectionType::Lines)
            } else if canvas.double_clicked() {
                Some(SelectionType::Semantic)
            } else {
                None
            };
            if let Some((id, r)) = pane_at(pos)
                && let Some(pane) = self.panes.get(&id)
            {
                let mut engine = pane.engine();
                if let Some(ty) = multi_click {
                    let (point, side) = render::point_at(r, cell, pos, &engine);
                    engine.term.selection = Some(Selection::new(ty, point, side));
                    copy = engine.term.selection_to_string();
                } else if canvas.clicked() {
                    engine.term.selection = None;
                    if let Some((_, _, url)) = link_hover.as_ref().filter(|(lid, _, _)| *lid == id) {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    }
                } else if canvas.clicked_by(PointerButton::Middle) {
                    drop(engine);
                    ui.ctx().send_viewport_cmd(ViewportCommand::RequestPaste);
                }
            }
        }
        if self.config.terminal.copy_on_select
            && let Some(text) = copy.filter(|t| !t.is_empty())
        {
            ui.ctx().copy_text(text);
        }

        // Plain wheel scrolling (scrollback, or arrow keys in full-screen apps).
        if !reporting
            && let Some(hover) = canvas.hover_pos()
            && let Some((id, _)) = pane_at(hover)
            && let Some(pane) = self.panes.get_mut(&id)
        {
            scroll_pane(ui, pane, cell);
        }

        let mut menu_action = None;
        if !reporting || canvas.context_menu_opened() {
            let has_selection = self.active_pane().is_some_and(|p| p.engine().term.selection.is_some());
            canvas.context_menu(|ui| {
                if let Some(a) = chrome::pane_menu(ui, &self.bindings, has_selection) {
                    menu_action = Some(a);
                }
            });
        }

        // Refresh find matches for the active pane and keep the current match in view.
        if self.find.open
            && let Some(pane) = self.panes.get(&active)
        {
            let mut engine = pane.engine();
            if self.find.update(active, &engine) {
                self.find.reveal(&mut engine);
            }
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }

        // Draw panes, their title bars and the window's rounded bottom corners.
        let palette = self.palette.clone();
        let style = self.style(&palette, cell, row_height);
        let window_focused = ui.input(|i| i.focused);
        let mut title_actions = Vec::new();
        for &(id, body, title) in &bodies {
            let Some(pane) = self.panes.get_mut(&id) else { continue };
            let (cols, rows) = render::grid_size(body, cell);
            pane.resize(cols, rows, cell);
            if id == active {
                pane.bell.store(false, Ordering::Relaxed);
            }
            let corners = CornerRadius {
                nw: 0,
                ne: 0,
                sw: if (body.min.x - area.min.x).abs() < 1.0 && (body.max.y - area.max.y).abs() < 1.0 {
                    bottom_radius
                } else {
                    0
                },
                se: if (body.max.x - area.max.x).abs() < 1.0 && (body.max.y - area.max.y).abs() < 1.0 {
                    bottom_radius
                } else {
                    0
                },
            };
            let view = PaneView {
                focused: id == active && window_focused,
                dim: if multi && id != active && self.config.window.dim_inactive_panes {
                    self.config.window.dim_amount
                } else {
                    0.0
                },
                corners,
                link: link_hover.as_ref().filter(|(lid, _, _)| *lid == id).map(|(_, span, _)| *span),
                search: (self.find.open && id == active)
                    .then(|| render::SearchView { matches: &self.find.matches, current: self.find.current }),
            };
            {
                let mut engine = pane.engine();
                if let Some(deadline) = engine.check_sync_timeout() {
                    ui.ctx().request_repaint_after(deadline.saturating_duration_since(Instant::now()));
                }
                render::draw(&ui.painter().with_clip_rect(body), body, &style, &engine, &view);
                if id == active {
                    let cursor = engine.term.grid().cursor.point;
                    let row = cursor.line.0 + engine.term.grid().display_offset() as i32;
                    let origin = render::grid_rect(body).min;
                    self.cursor_anchor =
                        Some(origin + vec2(cursor.column.0 as f32 * cell.x, (row + 1) as f32 * cell.y));
                }
            }
            if let Some(title_rect) = title {
                let text = self.pane_title(id);
                if let Some(a) =
                    chrome::pane_title(ui, title_rect, id, &text, id == active, &self.chrome, &self.bindings)
                {
                    title_actions.push((id, a));
                }
            }
        }
        if self.find.open
            && let Some(&(_, body, _)) = bodies.iter().find(|(id, _, _)| *id == active)
        {
            let anchor = pos2(body.max.x - 12.0, body.min.y + 8.0);
            match chrome::find_bar(ui.ctx(), anchor, &mut self.find, &self.chrome) {
                Some(chrome::FindBarAction::Previous) => self.find_step(true),
                Some(chrome::FindBarAction::Next) => self.find_step(false),
                Some(chrome::FindBarAction::Close) => self.find.close(),
                None => {}
            }
        }
        if self.autocomplete.open {
            if self.autocomplete.pane() != active {
                self.autocomplete.close();
            } else if let Some(anchor) = self.cursor_anchor {
                let font_size = self.font_size;
                if let Some(i) = self.autocomplete.show(ui.ctx(), anchor, cell.y, font_size, &self.chrome) {
                    self.accept_completion(Some(i));
                }
            }
        }
        for (id, a) in title_actions {
            match a {
                PaneTitleAction::Focus => self.tabs[self.current].active = id,
                PaneTitleAction::Close => self.close_pane(id),
                PaneTitleAction::Menu(action) => {
                    self.tabs[self.current].active = id;
                    menu_action = Some(action);
                }
            }
        }
        menu_action
    }

    fn tab_infos(&self) -> Vec<TabInfo> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let ids = tab.panes();
                TabInfo {
                    title: self.pane_title(tab.active),
                    active: i == self.current,
                    activity: i != self.current
                        && ids.iter().any(|id| self.panes.get(id).is_some_and(|p| p.bell.load(Ordering::Relaxed))),
                    panes: ids.len(),
                    shortcut: self.bindings.label(Action::SelectTab(i)),
                }
            })
            .collect()
    }

    fn run_script(&mut self, ctx: &egui::Context) {
        let Some(script) = self.script.as_mut() else { return };
        for effect in script.tick(ctx) {
            match effect {
                script::Effect::Action(a) => self.run(a),
                script::Effect::Type(text) => {
                    if let Some(pane) = self.active_pane() {
                        pane.input(text.as_bytes());
                    }
                }
                script::Effect::SettingsPage(page) => self.settings.set_page(&page),
                script::Effect::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
            }
        }
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Lays out the split tree, handling divider drags, and collects leaf rects.
fn layout_node(ui: &mut Ui, node: &mut Node, rect: Rect, id: Id, colors: &ChromeColors, out: &mut Vec<(PaneId, Rect)>) {
    match node {
        Node::Leaf(pane) => out.push((*pane, rect)),
        Node::Split { axis, ratio, first, second } => {
            let (a, divider, b) = layout::split_rect(rect, *axis, *ratio, DIVIDER);
            let icon = match axis {
                Axis::Horizontal => CursorIcon::ResizeHorizontal,
                Axis::Vertical => CursorIcon::ResizeVertical,
            };
            let response = ui.interact(divider.expand(3.0), id, Sense::drag()).on_hover_and_drag_cursor(icon);
            if response.dragged()
                && let Some(pos) = response.interact_pointer_pos()
            {
                let r = match axis {
                    Axis::Horizontal => (pos.x - rect.min.x) / rect.width(),
                    Axis::Vertical => (pos.y - rect.min.y) / rect.height(),
                };
                *ratio = r.clamp(0.1, 0.9);
            }
            ui.painter().rect_filled(divider, 0.0, colors.divider);
            layout_node(ui, first, a, id.with(0), colors, out);
            layout_node(ui, second, b, id.with(1), colors, out);
        }
    }
}

/// Mouse wheel: scrollback normally; arrow keys in full-screen apps (alternate scroll mode).
fn scroll_pane(ui: &Ui, pane: &mut Pane, cell: Vec2) {
    let rows = pane.engine().term.screen_lines() as f32;
    let lines: f32 = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                Event::MouseWheel { unit, delta, .. } => Some(match unit {
                    MouseWheelUnit::Line => delta.y * 3.0,
                    MouseWheelUnit::Point => delta.y / cell.y,
                    MouseWheelUnit::Page => delta.y * rows,
                }),
                _ => None,
            })
            .sum()
    });
    pane.scroll_accum += lines;
    let n = pane.scroll_accum.trunc();
    pane.scroll_accum -= n;
    if n == 0.0 {
        return;
    }
    let mode = *pane.engine().term.mode();
    if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
        let key = if n > 0.0 { Key::ArrowUp } else { Key::ArrowDown };
        if let Some(bytes) = input::key_bytes(key, Modifiers::NONE, mode) {
            pane.write(&bytes.repeat(n.abs() as usize));
        }
    } else {
        pane.engine().term.scroll_display(Scroll::Delta(n as i32));
    }
}

fn short_path(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        if path == home {
            return "~".to_owned();
        }
        if let Ok(rest) = path.strip_prefix(&home) {
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Transparent so the iTerm-style window can have rounded corners.
        [0.0; 4]
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(script) = self.script.as_mut() {
            raw_input.events.extend(script.take_events());
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.check_config_reload();
        if self.config.window.theme == ThemeChoice::Auto && self.resolve_dark() != self.dark {
            self.apply_config();
        }
        self.run_script(&ctx);

        let exited: Vec<PaneId> = self.panes.values().filter(|p| p.has_exited()).map(|p| p.id).collect();
        for id in exited {
            self.close_pane(id);
        }
        self.handle_keyboard(&ctx);
        if self.tabs.is_empty() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }

        let font = FontId::monospace(self.font_size);
        let (glyph_w, row_h) = ctx.fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let cell = vec2(glyph_w, (row_h * self.config.font.line_height).round());

        let iterm = self.config.window.decorations == Decorations::Iterm;
        let (maximized, fullscreen, focused) = ctx.input(|i| {
            let v = i.viewport();
            (v.maximized.unwrap_or(false), v.fullscreen.unwrap_or(false), v.focused.unwrap_or(true))
        });
        let window = ui.max_rect();
        let radius = if iterm && !maximized && !fullscreen {
            self.config.window.corner_radius.clamp(0.0, 30.0) as u8
        } else {
            0
        };
        let painter = ui.painter().clone();
        painter.rect_filled(window, radius, if iterm { self.chrome.window } else { self.palette.bg });

        let show_tabs = self.tabs.len() > 1 || self.config.window.show_tab_bar_with_one_tab;
        let chrome_h =
            if iterm { chrome::TITLE_BAR_HEIGHT } else { 0.0 } + if show_tabs { chrome::TAB_BAR_HEIGHT } else { 0.0 };

        // Size the window to columns x rows once real metrics are known.
        if self.sized_window < 3 {
            self.sized_window += 1;
            if self.sized_window == 3 {
                let size = render::pane_size_for(self.config.window.columns, self.config.window.rows, cell)
                    + vec2(0.0, chrome_h);
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
            }
        }

        let mut y = window.min.y;
        let title = self.pane_title(self.tabs[self.current].active);
        if iterm {
            let r = Rect::from_min_max(window.min, pos2(window.max.x, y + chrome::TITLE_BAR_HEIGHT));
            chrome::title_bar(ui, r, &title, &self.chrome, focused, maximized);
            y = r.max.y;
        }
        let mut tab_action = None;
        if show_tabs {
            let r = Rect::from_min_max(pos2(window.min.x, y), pos2(window.max.x, y + chrome::TAB_BAR_HEIGHT));
            tab_action = chrome::tab_bar(ui, r, &self.tab_infos(), &self.chrome, iterm);
            y = r.max.y;
        }
        if iterm && chrome_h > 0.0 {
            painter.hline(window.x_range(), y - 0.5, Stroke::new(1.0, Color32::from_black_alpha(60)));
        }
        let content = Rect::from_min_max(pos2(window.min.x, y), window.max);
        let menu_action = self.terminal_area(ui, content, cell, row_h, radius);

        if radius > 0 {
            painter.rect_stroke(window, radius, Stroke::new(1.0, self.chrome.window_border), StrokeKind::Inside);
        }
        if iterm && !maximized && !fullscreen {
            chrome::resize_edges(ui, window);
        }
        if !iterm {
            ctx.send_viewport_cmd(ViewportCommand::Title(title));
        }
        self.draw_notice(ui, content);

        match tab_action {
            Some(TabBarAction::Select(i)) => self.current = i,
            Some(TabBarAction::Close(i)) => self.close_tab(i),
            Some(TabBarAction::New) => self.new_tab(),
            None => {}
        }
        if let Some(action) = menu_action {
            self.run(action);
        }
        self.show_settings(&ctx);
    }
}

impl App {
    fn show_settings(&mut self, ctx: &egui::Context) {
        if self.settings.open {
            let env = settings::Env {
                palette: &self.palette,
                bindings: &self.bindings,
                config_path: self.config_path.as_deref(),
            };
            let out = self.settings.show(ctx, &env);
            if out.changed {
                let old_decorations = self.config.window.decorations;
                self.config = self.settings.draft.clone();
                if self.config.window.decorations != old_decorations {
                    ctx.send_viewport_cmd(ViewportCommand::Decorations(
                        self.config.window.decorations == Decorations::System,
                    ));
                }
                self.apply_config();
                self.save_due = Some(Instant::now() + Duration::from_millis(400));
            }
            if out.resize_window {
                // Re-run the one-shot columns x rows sizing on the next frame.
                self.sized_window = 2;
            }
        }
        if let Some(due) = self.save_due {
            if Instant::now() < due {
                ctx.request_repaint_after(due - Instant::now());
            } else {
                self.save_due = None;
                if let Some(path) = self.config_path.clone() {
                    match config::save(&path, &self.config) {
                        Ok(()) => self.config_mtime = mtime(&path),
                        Err(e) => self.notify(format!("Failed to save settings: {e}")),
                    }
                }
            }
        }
    }

    /// Transient message (config errors, reloads) in the bottom-right corner.
    fn draw_notice(&mut self, ui: &Ui, area: Rect) {
        let Some((message, since)) = &self.notice else { return };
        let age = since.elapsed().as_secs_f32();
        if age > 6.0 {
            self.notice = None;
            return;
        }
        ui.ctx().request_repaint_after(Duration::from_millis(250));
        let font = FontId::proportional(12.5);
        let text = chrome::fit_text(ui, message, &font, area.width() - 60.0);
        let galley = ui.painter().layout_no_wrap(text, font, self.chrome.tab_text);
        let size = galley.size() + vec2(20.0, 12.0);
        let rect = Rect::from_min_size(area.max - size - vec2(14.0, 14.0), size);
        let error =
            message.starts_with("Config error") || message.contains("not found") || message.starts_with("Failed");
        let fill = if error { Color32::from_rgb(0x8a, 0x2c, 0x2c) } else { self.chrome.tab_active };
        ui.painter().rect_filled(rect, 8.0, fill);
        ui.painter().galley(rect.min + vec2(10.0, 6.0), galley, self.chrome.tab_text);
    }
}
