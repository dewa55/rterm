//! iTerm2-style Settings window (Ctrl+,). Edits a copy of the config that the app applies
//! live and saves back to `config.toml`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, Align2, Color32, ComboBox, CornerRadius, DragValue, Event, FontId, Grid, Key, RichText, ScrollArea, Sense,
    Slider, TextEdit, Ui, ViewportBuilder, ViewportId, vec2,
};

use crate::config::{
    self, ACTIONS, Action, Bindings, Config, CursorStyle, Decorations, KeyCombo, Palette, ThemeChoice,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Appearance,
    Profiles,
    Keys,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProfileTab {
    Text,
    Colors,
    Terminal,
}

/// What the app currently uses, for display in the window.
pub struct Env<'a> {
    pub palette: &'a Palette,
    pub bindings: &'a Bindings,
    pub config_path: Option<&'a Path>,
}

#[derive(Default)]
pub struct Output {
    /// The draft changed and should be applied and saved.
    pub changed: bool,
    pub resize_window: bool,
}

type PickerResult = Arc<Mutex<Option<Option<PathBuf>>>>;

pub struct Settings {
    pub open: bool,
    pub draft: Config,
    page: Page,
    profile_tab: ProfileTab,
    /// Action waiting for a new shortcut.
    recording: Option<Action>,
    mono_fonts: Option<Vec<String>>,
    presets: Option<(PathBuf, Vec<String>)>,
    picker: Option<PickerResult>,
}

impl Settings {
    pub fn new() -> Settings {
        Settings {
            open: false,
            draft: Config::default(),
            page: Page::General,
            profile_tab: ProfileTab::Text,
            recording: None,
            mono_fonts: None,
            presets: None,
            picker: None,
        }
    }

    pub fn open_with(&mut self, config: &Config) {
        self.draft = config.clone();
        self.recording = None;
        self.open = true;
    }

    /// Selects a page by name (used by `RTERM_SCRIPT`): general, appearance, text, colors, terminal, keys.
    pub fn set_page(&mut self, name: &str) {
        match name {
            "general" => self.page = Page::General,
            "appearance" => self.page = Page::Appearance,
            "text" | "colors" | "terminal" => {
                self.page = Page::Profiles;
                self.profile_tab = match name {
                    "text" => ProfileTab::Text,
                    "colors" => ProfileTab::Colors,
                    _ => ProfileTab::Terminal,
                };
            }
            "keys" => self.page = Page::Keys,
            _ => {}
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, env: &Env<'_>) -> Output {
        let mut out = Output::default();
        let builder = ViewportBuilder::default()
            .with_title("rterm Settings")
            .with_app_id("rterm")
            .with_inner_size([740.0, 580.0])
            .with_min_inner_size([600.0, 440.0]);
        ctx.show_viewport_immediate(ViewportId::from_hash_of("rterm-settings"), builder, |ui, _| {
            if ui.input(|i| i.viewport().close_requested()) {
                self.open = false;
                self.recording = None;
            }
            let fill = ui.visuals().panel_fill;
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(18, 12)))
                .show(ui, |ui| self.contents(ui, env, &mut out));
        });
        out
    }

    fn contents(&mut self, ui: &mut Ui, env: &Env<'_>, out: &mut Output) {
        self.toolbar(ui);
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(8.0);
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.page {
            Page::General => self.general(ui, env, out),
            Page::Appearance => self.appearance(ui, out),
            Page::Profiles => self.profiles(ui, env, out),
            Page::Keys => self.keys(ui, env, out),
        });
    }

    /// Icon toolbar, like the top of iTerm2's Settings window.
    fn toolbar(&mut self, ui: &mut Ui) {
        let pages = [
            (Page::General, "General"),
            (Page::Appearance, "Appearance"),
            (Page::Profiles, "Profiles"),
            (Page::Keys, "Keys"),
        ];
        let width = 92.0;
        let total = width * pages.len() as f32;
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
            for (page, label) in pages {
                let (rect, response) = ui.allocate_exact_size(vec2(width, 54.0), Sense::click());
                let selected = self.page == page;
                let visuals = ui.visuals();
                if selected {
                    ui.painter().rect_filled(
                        rect.shrink(2.0),
                        CornerRadius::same(8),
                        visuals.selection.bg_fill.gamma_multiply(0.45),
                    );
                } else if response.hovered() {
                    ui.painter().rect_filled(rect.shrink(2.0), CornerRadius::same(8), visuals.widgets.hovered.bg_fill);
                }
                let color = if selected { visuals.strong_text_color() } else { visuals.text_color() };
                draw_icon(ui.painter(), rect.center() - vec2(0.0, 9.0), page, color);
                ui.painter().text(
                    rect.center() + vec2(0.0, 14.0),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(12.5),
                    color,
                );
                if response.clicked() {
                    self.page = page;
                    self.recording = None;
                }
            }
        });
    }

    fn general(&mut self, ui: &mut Ui, env: &Env<'_>, out: &mut Output) {
        let t = &mut self.draft.terminal;
        section(ui, "Startup");
        grid(ui, "general-startup", |ui| {
            ui.label("Shell");
            let mut shell = t.shell.clone().unwrap_or_default();
            let hint = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
            let edit = ui
                .add(TextEdit::singleline(&mut shell).hint_text(format!("{hint} (from $SHELL)")).desired_width(280.0));
            if edit.changed() {
                t.shell = Some(shell.trim().to_owned()).filter(|s| !s.is_empty());
                out.changed = true;
            }
            ui.end_row();
            ui.label("");
            note(ui, "Used for new tabs and panes.");
            ui.end_row();
            ui.label("Shell integration");
            out.changed |=
                ui.checkbox(&mut t.shell_integration, "Load rterm's integration into bash and zsh").changed();
            ui.end_row();
            ui.label("");
            note(ui, "Prompt marks, jump to prompt, and new tabs in the current directory.");
            ui.end_row();
        });

        section(ui, "Selection & Mouse");
        grid(ui, "general-mouse", |ui| {
            ui.label("Selection");
            out.changed |= ui.checkbox(&mut t.copy_on_select, "Copy to clipboard on selection").changed();
            ui.end_row();
            ui.label("Mouse");
            out.changed |=
                ui.checkbox(&mut t.mouse_reporting, "Report mouse clicks and scrolling to applications").changed();
            ui.end_row();
            ui.label("");
            note(ui, "For vim, htop, tmux… Hold Shift to select text in those apps.");
            ui.end_row();
        });

        section(ui, "Configuration File");
        grid(ui, "general-file", |ui| {
            ui.label("Location");
            ui.horizontal(|ui| {
                if let Some(p) = env.config_path
                    && ui.button("Open in Editor").clicked()
                {
                    let _ = Command::new("xdg-open").arg(p).spawn();
                }
                let path = env.config_path.map(|p| p.display().to_string()).unwrap_or_else(|| "(none)".into());
                ui.add(egui::Label::new(RichText::new(&path).monospace()).truncate()).on_hover_text(&path);
            });
            ui.end_row();
            ui.label("");
            note(ui, "Changes made here are saved to this file automatically.");
            ui.end_row();
        });
    }

    fn appearance(&mut self, ui: &mut Ui, out: &mut Output) {
        let w = &mut self.draft.window;
        section(ui, "General");
        grid(ui, "appearance-general", |ui| {
            ui.label("Theme");
            ComboBox::from_id_salt("theme")
                .selected_text(match w.theme {
                    ThemeChoice::Auto => "Automatic (follow system)",
                    ThemeChoice::Dark => "Dark",
                    ThemeChoice::Light => "Light",
                })
                .width(220.0)
                .show_ui(ui, |ui| {
                    for (v, label) in [
                        (ThemeChoice::Auto, "Automatic (follow system)"),
                        (ThemeChoice::Dark, "Dark"),
                        (ThemeChoice::Light, "Light"),
                    ] {
                        out.changed |= ui.selectable_value(&mut w.theme, v, label).changed();
                    }
                });
            ui.end_row();
            ui.label("Window style");
            ui.vertical(|ui| {
                out.changed |= ui
                    .radio_value(
                        &mut w.decorations,
                        Decorations::Iterm,
                        "iTerm2 (traffic-light title bar, rounded corners)",
                    )
                    .changed();
                out.changed |= ui.radio_value(&mut w.decorations, Decorations::System, "System title bar").changed();
                note(ui, "Rounded corners need a restart after switching from System.");
            });
            ui.end_row();
            ui.label("Corner radius");
            out.changed |= ui
                .add_enabled(
                    w.decorations == Decorations::Iterm,
                    Slider::new(&mut w.corner_radius, 0.0..=20.0).step_by(1.0).suffix(" px"),
                )
                .changed();
            ui.end_row();
        });

        section(ui, "Tabs & Panes");
        grid(ui, "appearance-tabs", |ui| {
            ui.label("Tab bar");
            out.changed |=
                ui.checkbox(&mut w.show_tab_bar_with_one_tab, "Show tab bar even when there is only one tab").changed();
            ui.end_row();
            ui.label("Panes");
            out.changed |= ui.checkbox(&mut w.pane_title_bars, "Show per-pane title bar with split panes").changed();
            ui.end_row();
            ui.label("Dimming");
            out.changed |= ui.checkbox(&mut w.dim_inactive_panes, "Dim inactive split panes").changed();
            ui.end_row();
            ui.label("Dimming amount");
            out.changed |=
                ui.add_enabled(w.dim_inactive_panes, Slider::new(&mut w.dim_amount, 0.0..=0.9).step_by(0.05)).changed();
            ui.end_row();
        });

        section(ui, "New Windows");
        grid(ui, "appearance-size", |ui| {
            ui.label("Size");
            ui.horizontal(|ui| {
                out.changed |= ui.add(DragValue::new(&mut w.columns).range(20..=500)).changed();
                ui.label("columns ×");
                out.changed |= ui.add(DragValue::new(&mut w.rows).range(5..=200)).changed();
                ui.label("rows");
                if ui.button("Resize Window Now").clicked() {
                    out.resize_window = true;
                }
            });
            ui.end_row();
        });
    }

    fn profiles(&mut self, ui: &mut Ui, env: &Env<'_>, out: &mut Output) {
        ui.horizontal(|ui| {
            for (tab, label) in
                [(ProfileTab::Text, "Text"), (ProfileTab::Colors, "Colors"), (ProfileTab::Terminal, "Terminal")]
            {
                ui.selectable_value(&mut self.profile_tab, tab, RichText::new(label).size(14.0));
            }
        });
        ui.add_space(6.0);
        match self.profile_tab {
            ProfileTab::Text => self.profile_text(ui, env, out),
            ProfileTab::Colors => self.profile_colors(ui, env, out),
            ProfileTab::Terminal => self.profile_terminal(ui, out),
        }
    }

    fn profile_text(&mut self, ui: &mut Ui, env: &Env<'_>, out: &mut Output) {
        let fonts = self.mono_fonts.get_or_insert_with(monospace_fonts).clone();
        let f = &mut self.draft.font;
        section(ui, "Font");
        grid(ui, "text-font", |ui| {
            ui.label("Font");
            ui.horizontal(|ui| {
                ComboBox::from_id_salt("font-family").selected_text(f.family.clone()).width(220.0).show_ui(ui, |ui| {
                    for name in std::iter::once("monospace".to_owned()).chain(fonts) {
                        out.changed |= ui.selectable_value(&mut f.family, name.clone(), name).changed();
                    }
                });
                let edit =
                    ui.add(TextEdit::singleline(&mut f.family).desired_width(160.0).hint_text("family or .ttf path"));
                // Apply when editing finishes, not on every keystroke (each change reloads fonts).
                out.changed |= edit.lost_focus();
            });
            ui.end_row();
            ui.label("Size");
            out.changed |= ui.add(Slider::new(&mut f.size, 8.0..=32.0).step_by(0.5).suffix(" pt")).changed();
            ui.end_row();
            ui.label("Line spacing");
            out.changed |= ui.add(Slider::new(&mut f.line_height, 0.8..=1.8).step_by(0.05)).changed();
            ui.end_row();
            ui.label("Bold");
            ui.vertical(|ui| {
                out.changed |= ui.checkbox(&mut f.use_bold_font, "Use a bold font face").changed();
                out.changed |= ui.checkbox(&mut self.draft.terminal.bold_is_bright, "Brighten bold text").changed();
            });
            ui.end_row();
        });

        section(ui, "Cursor");
        grid(ui, "text-cursor", |ui| {
            ui.label("Style");
            ui.horizontal(|ui| {
                let c = &mut self.draft.terminal.cursor;
                out.changed |= ui.radio_value(c, CursorStyle::Block, "Box").changed();
                out.changed |= ui.radio_value(c, CursorStyle::Beam, "Vertical bar").changed();
                out.changed |= ui.radio_value(c, CursorStyle::Underline, "Underline").changed();
            });
            ui.end_row();
        });

        section(ui, "Preview");
        preview(ui, env.palette, &self.draft);
    }

    fn profile_colors(&mut self, ui: &mut Ui, env: &Env<'_>, out: &mut Output) {
        self.poll_picker(ui, out);
        let colors = &mut self.draft.colors;

        section(ui, "Color Preset");
        let preset_names = match &colors.itermcolors {
            Some(path) => {
                let path = config::expand_home(path);
                if self.presets.as_ref().map(|(p, _)| p != &path).unwrap_or(true) {
                    self.presets = Some((path.clone(), preset_names(&path)));
                }
                self.presets.as_ref().map(|(_, n)| n.clone()).unwrap_or_default()
            }
            None => Vec::new(),
        };
        grid(ui, "colors-preset", |ui| {
            ui.label("Preset");
            ui.horizontal(|ui| {
                let current = match (&colors.itermcolors, &colors.preset) {
                    (None, _) => "Regular (iTerm2 default)".to_owned(),
                    (Some(_), Some(p)) => p.clone(),
                    (Some(path), None) => {
                        path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
                    }
                };
                ComboBox::from_id_salt("color-preset").selected_text(current).width(240.0).show_ui(ui, |ui| {
                    if ui.selectable_label(colors.itermcolors.is_none(), "Regular (iTerm2 default)").clicked() {
                        colors.itermcolors = None;
                        colors.preset = None;
                        out.changed = true;
                    }
                    for name in &preset_names {
                        if ui.selectable_label(colors.preset.as_deref() == Some(name), name).clicked() {
                            colors.preset = Some(name.clone());
                            out.changed = true;
                        }
                    }
                });
                if ui
                    .button("Import…")
                    .on_hover_text("Load an .itermcolors file or iTerm2's ColorPresets.plist")
                    .clicked()
                    && self.picker.is_none()
                {
                    self.picker = Some(pick_color_file());
                }
            });
            ui.end_row();
            if let Some(path) = &colors.itermcolors {
                ui.label("File");
                ui.monospace(path.display().to_string());
                ui.end_row();
            }
        });

        section(ui, "Colors");
        let p = env.palette;
        let mut edits: Vec<(&'static str, Color32)> = Vec::new();
        grid(ui, "colors-basic", |ui| {
            let rows: [(&str, &str, Color32, &str, &str, Color32); 4] = [
                ("Foreground", "foreground", p.fg, "Background", "background", p.bg),
                ("Bold", "bold", p.bold, "Link", "link", p.link),
                ("Cursor", "cursor", p.cursor, "Cursor text", "cursor_text", p.cursor_text),
                ("Selection", "selection", p.selection, "Selected text", "selection_text", p.selection_text),
            ];
            for (l1, k1, c1, l2, k2, c2) in rows {
                for (label, key, color) in [(l1, k1, c1), (l2, k2, c2)] {
                    ui.label(label);
                    let mut c = color;
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        edits.push((key, c));
                    }
                }
                ui.end_row();
            }
        });
        for (key, c) in edits {
            let hex = Some(config::color_hex(c));
            match key {
                "foreground" => colors.foreground = hex,
                "background" => colors.background = hex,
                "bold" => colors.bold = hex,
                "link" => colors.link = hex,
                "cursor" => colors.cursor = hex,
                "cursor_text" => colors.cursor_text = hex,
                "selection" => colors.selection = hex,
                _ => colors.selection_text = hex,
            }
            out.changed = true;
        }

        section(ui, "ANSI Colors");
        let mut ansi = p.ansi;
        let mut ansi_changed = false;
        Grid::new("colors-ansi").spacing([8.0, 8.0]).show(ui, |ui| {
            for (row, label) in [(0, "Normal"), (8, "Bright")] {
                ui.label(label);
                for c in &mut ansi[row..row + 8] {
                    ansi_changed |= ui.color_edit_button_srgba(c).changed();
                }
                ui.end_row();
            }
            ui.label("");
            for name in ["Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White"] {
                ui.label(RichText::new(name).small());
            }
            ui.end_row();
        });
        if ansi_changed {
            colors.ansi = Some(ansi.iter().map(|c| config::color_hex(*c)).collect());
            out.changed = true;
        }
        ui.add_space(8.0);
        let has_overrides = colors.foreground.is_some()
            || colors.background.is_some()
            || colors.bold.is_some()
            || colors.cursor.is_some()
            || colors.cursor_text.is_some()
            || colors.selection.is_some()
            || colors.selection_text.is_some()
            || colors.link.is_some()
            || colors.ansi.is_some();
        if ui.add_enabled(has_overrides, egui::Button::new("Reset Colors to Preset")).clicked() {
            *colors = config::ColorConfig {
                itermcolors: colors.itermcolors.clone(),
                preset: colors.preset.clone(),
                ..Default::default()
            };
            out.changed = true;
        }

        section(ui, "Preview");
        preview(ui, env.palette, &self.draft);
    }

    fn profile_terminal(&mut self, ui: &mut Ui, out: &mut Output) {
        let t = &mut self.draft.terminal;
        section(ui, "Scrollback");
        grid(ui, "terminal-scrollback", |ui| {
            ui.label("Scrollback lines");
            out.changed |= ui.add(DragValue::new(&mut t.scrollback).range(0..=1_000_000).speed(100)).changed();
            ui.end_row();
        });
        section(ui, "Environment");
        grid(ui, "terminal-env", |ui| {
            ui.label("Report terminal type");
            ui.monospace("xterm-256color");
            ui.end_row();
            ui.label("");
            note(ui, "COLORTERM=truecolor and TERM_PROGRAM=rterm are also set.");
            ui.end_row();
        });
    }

    fn keys(&mut self, ui: &mut Ui, env: &Env<'_>, out: &mut Output) {
        if let Some(action) = self.recording {
            let captured = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some((key, modifiers)) = captured {
                let no_mods = !modifiers.ctrl && !modifiers.alt && !modifiers.shift;
                let old: Vec<String> = env.bindings.combos(action).iter().map(|c| c.to_config_string()).collect();
                if key == Key::Escape && no_mods {
                    // cancelled
                } else if matches!(key, Key::Backspace | Key::Delete) && no_mods {
                    for c in old {
                        self.draft.keys.insert(c, "none".into());
                    }
                    out.changed = true;
                } else {
                    for c in old {
                        self.draft.keys.insert(c, "none".into());
                    }
                    let combo = KeyCombo::from_event(key, modifiers).to_config_string();
                    self.draft.keys.insert(combo, action.name().to_owned());
                    out.changed = true;
                }
                self.recording = None;
            }
        }

        ui.horizontal(|ui| {
            note(ui, "Click a shortcut to change it. Esc cancels, Backspace removes it.");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!self.draft.keys.is_empty(), egui::Button::new("Restore Defaults")).clicked() {
                    self.draft.keys.clear();
                    self.recording = None;
                    out.changed = true;
                }
            });
        });
        ui.add_space(6.0);
        Grid::new("keys-grid").num_columns(2).striped(true).spacing([24.0, 6.0]).min_col_width(240.0).show(ui, |ui| {
            for (_, action, label) in ACTIONS {
                ui.label(*label);
                let recording = self.recording == Some(*action);
                let text = if recording {
                    "Press shortcut…".to_owned()
                } else {
                    let combos: Vec<String> = env.bindings.combos(*action).iter().map(KeyCombo::label).collect();
                    if combos.is_empty() { "—".to_owned() } else { combos.join(",  ") }
                };
                let button =
                    egui::Button::new(RichText::new(text).monospace()).min_size(vec2(200.0, 0.0)).selected(recording);
                if ui.add(button).clicked() {
                    self.recording = if recording { None } else { Some(*action) };
                }
                ui.end_row();
            }
        });
    }

    /// Applies the result of a running file picker.
    fn poll_picker(&mut self, ui: &Ui, out: &mut Output) {
        let Some(picker) = &self.picker else { return };
        let result = picker.lock().unwrap().take();
        match result {
            None => ui.ctx().request_repaint_after(std::time::Duration::from_millis(200)),
            Some(path) => {
                self.picker = None;
                if let Some(path) = path {
                    let names = preset_names(&path);
                    self.draft.colors.preset = names.first().cloned();
                    self.draft.colors.itermcolors = Some(path);
                    out.changed = true;
                }
            }
        }
    }
}

/// Small vector icons for the toolbar (emoji fonts are not reliably available).
fn draw_icon(painter: &egui::Painter, c: egui::Pos2, page: Page, color: Color32) {
    use egui::{Stroke, pos2};
    let s = Stroke::new(1.6, color);
    match page {
        Page::General => {
            // Gear: ring, hub and eight teeth.
            painter.circle_stroke(c, 6.5, s);
            painter.circle_stroke(c, 2.5, s);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                painter.line_segment([c + d * 6.5, c + d * 9.5], Stroke::new(2.4, color));
            }
        }
        Page::Appearance => {
            // Window with a title bar and traffic lights.
            let r = egui::Rect::from_center_size(c, vec2(22.0, 16.0));
            painter.rect_stroke(r, 3.0, s, egui::StrokeKind::Middle);
            painter.hline(r.x_range(), r.min.y + 5.0, s);
            for i in 0..3 {
                painter.circle_filled(pos2(r.min.x + 3.5 + i as f32 * 3.5, r.min.y + 2.6), 1.0, color);
            }
        }
        Page::Profiles => {
            // Person: head and shoulders.
            painter.circle_stroke(c - vec2(0.0, 4.0), 3.8, s);
            let points: Vec<egui::Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 16.0);
                    c + vec2(8.0 * a.cos(), 9.0 + 6.0 * a.sin())
                })
                .collect();
            painter.add(egui::Shape::line(points, s));
        }
        Page::Keys => {
            // Keyboard: outline, two rows of keys and a space bar.
            let r = egui::Rect::from_center_size(c, vec2(24.0, 14.0));
            painter.rect_stroke(r, 3.0, s, egui::StrokeKind::Middle);
            for row in 0..2 {
                for col in 0..5 {
                    let p = pos2(r.min.x + 4.0 + col as f32 * 4.0, r.min.y + 3.5 + row as f32 * 3.5);
                    painter.circle_filled(p, 0.9, color);
                }
            }
            painter.hline((r.min.x + 7.0)..=(r.max.x - 7.0), r.max.y - 3.0, s);
        }
    }
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong().size(14.0));
    ui.add_space(4.0);
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().weak());
}

fn grid(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
    ui.indent(id, |ui| {
        Grid::new(id).num_columns(2).spacing([18.0, 8.0]).min_col_width(130.0).show(ui, add);
    });
}

/// Terminal-style sample in the current colors and font.
fn preview(ui: &mut Ui, p: &Palette, config: &Config) {
    let font = FontId::monospace(config.font.size);
    let line_h = ui.fonts_mut(|f| f.row_height(&font)) * config.font.line_height;
    let lines = 4.0;
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().min(560.0), line_h * lines + 20.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, p.bg);
    let x0 = rect.min.x + 12.0;
    let mut y = rect.min.y + 10.0;
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'M'));
    let put = |x: &mut f32, y: f32, text: &str, color: Color32, bg: Option<Color32>| {
        let w = char_w * text.chars().count() as f32;
        if let Some(bg) = bg {
            painter.rect_filled(egui::Rect::from_min_size(egui::pos2(*x, y), vec2(w, line_h)), 0.0, bg);
        }
        painter.text(egui::pos2(*x, y), Align2::LEFT_TOP, text, font.clone(), color);
        *x += w;
    };
    let mut x = x0;
    put(&mut x, y, "dewa@ubuntu", p.ansi[2], None);
    put(&mut x, y, ":", p.fg, None);
    put(&mut x, y, "~/rterm", p.ansi[4], None);
    put(&mut x, y, "$ ", p.fg, None);
    put(&mut x, y, "ls --color", p.fg, None);
    y += line_h;
    x = x0;
    put(&mut x, y, "src", p.ansi[12], None);
    put(&mut x, y, "  build.sh", p.ansi[10], None);
    put(&mut x, y, "  README.md  ", p.fg, None);
    put(&mut x, y, "selected text", p.selection_text, Some(p.selection));
    y += line_h;
    x = x0;
    for c in &p.ansi[..8] {
        put(&mut x, y, "██", *c, None);
    }
    y += line_h;
    x = x0;
    for c in &p.ansi[8..] {
        put(&mut x, y, "██", *c, None);
    }
    put(&mut x, y, "  ", p.fg, None);
    put(&mut x, y, " ", p.cursor_text, Some(p.cursor));
}

/// Monospace font families known to fontconfig.
fn monospace_fonts() -> Vec<String> {
    let Ok(out) = Command::new("fc-list").args([":spacing=mono", "family"]).output() else { return Vec::new() };
    let mut names: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split(',').next())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Preset names in a multi-preset plist (empty for a single `.itermcolors` scheme).
fn preset_names(path: &Path) -> Vec<String> {
    let Ok(value) = plist::Value::from_file(path) else { return Vec::new() };
    let Some(root) = value.as_dictionary() else { return Vec::new() };
    if root.contains_key("Foreground Color") || root.contains_key("Foreground Color (Dark)") {
        return Vec::new();
    }
    root.iter().filter(|(_, v)| v.as_dictionary().is_some()).map(|(k, _)| k.clone()).collect()
}

/// Opens a GTK file chooser (zenity) without blocking the UI.
fn pick_color_file() -> PickerResult {
    let result: PickerResult = Arc::new(Mutex::new(None));
    let slot = result.clone();
    std::thread::spawn(move || {
        let picked = Command::new("zenity")
            .args([
                "--file-selection",
                "--title=Import iTerm2 Color Preset",
                "--file-filter=iTerm2 colors | *.itermcolors *.plist",
            ])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
        *slot.lock().unwrap() = Some(picked);
    });
    result
}
