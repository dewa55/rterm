//! Window chrome modeled on iTerm2 3.6/3.7 (macOS Tahoe theme): traffic-light title bar,
//! capsule tab bar, per-pane title bars, and window resize edges for undecorated windows.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Rect, ResizeDirection, RichText, Sense, Stroke,
    StrokeKind, Ui, ViewportCommand, pos2, vec2,
};

use crate::config::{Action, Bindings};
use crate::find::Find;
use crate::layout::{Axis, PaneId};

pub const TITLE_BAR_HEIGHT: f32 = 30.0;
pub const TAB_BAR_HEIGHT: f32 = 38.0;
pub const PANE_TITLE_HEIGHT: f32 = 22.0;
const RESIZE_EDGE: f32 = 5.0;

pub struct ChromeColors {
    pub window: Color32,
    pub window_border: Color32,
    pub title_text: Color32,
    pub title_text_inactive: Color32,
    pub tab_container: Color32,
    pub tab_active: Color32,
    pub tab_active_highlight: Color32,
    pub tab_text: Color32,
    pub tab_text_inactive: Color32,
    pub tab_separator: Color32,
    pub pane_title_active: Color32,
    pub pane_title_inactive: Color32,
    pub pane_title_border: Color32,
    pub pane_title_text: Color32,
    pub pane_title_text_inactive: Color32,
    pub divider: Color32,
    pub activity: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl ChromeColors {
    pub fn new(dark: bool) -> ChromeColors {
        if dark {
            ChromeColors {
                window: rgb(0x1e2124),
                window_border: Color32::from_black_alpha(160),
                title_text: rgb(0xdedfe0),
                title_text_inactive: rgb(0x85888c),
                tab_container: rgb(0x2b2e31),
                // TahoeDarkMidTab.png: fill #494c4e with a #6d737a top highlight.
                tab_active: rgb(0x494c4e),
                tab_active_highlight: rgb(0x6d737a),
                tab_text: rgb(0xeeeeee),
                tab_text_inactive: rgb(0xa2a5a9),
                tab_separator: rgb(0x43464a),
                pane_title_active: rgb(0x575757),
                pane_title_inactive: rgb(0x3c3c3c),
                pane_title_border: rgb(0x6e6e6e),
                pane_title_text: rgb(0xeaeaea),
                pane_title_text_inactive: rgb(0x9c9c9c),
                divider: rgb(0x555555),
                activity: rgb(0x328eee),
            }
        } else {
            ChromeColors {
                window: rgb(0xebebeb),
                window_border: Color32::from_black_alpha(70),
                title_text: rgb(0x262626),
                title_text_inactive: rgb(0x9a9a9a),
                tab_container: rgb(0xdcdcdc),
                tab_active: rgb(0xffffff),
                tab_active_highlight: rgb(0xffffff),
                tab_text: rgb(0x1d1d1d),
                tab_text_inactive: rgb(0x555555),
                tab_separator: rgb(0xbdbdbd),
                pane_title_active: rgb(0xd3d3d3),
                pane_title_inactive: rgb(0xe6e6e6),
                pane_title_border: rgb(0xb0b0b0),
                pane_title_text: rgb(0x1d1d1d),
                pane_title_text_inactive: rgb(0x808080),
                divider: rgb(0xb4b4b4),
                activity: rgb(0x328eee),
            }
        }
    }
}

/// Shortens `text` with a leading or trailing ellipsis until it fits `max_width`.
pub fn fit_text(ui: &Ui, text: &str, font: &FontId, max_width: f32) -> String {
    let width = |s: &str| ui.fonts_mut(|f| f.layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE).size().x);
    if width(text) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if width(&candidate) <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo == 0 { String::new() } else { chars[..lo].iter().collect::<String>() + "…" }
}

/// Title bar with macOS-style traffic lights; dragging moves the window.
pub fn title_bar(ui: &Ui, rect: Rect, title: &str, colors: &ChromeColors, focused: bool, maximized: bool) {
    let painter = ui.painter();
    let drag = ui.interact(rect, Id::unique("title-bar"), Sense::click_and_drag());
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }

    let r = 6.0;
    let y = rect.center().y;
    let lights = [
        (rgb(0xff5f57), "close", ViewportCommand::Close),
        (rgb(0xfebc2e), "minimize", ViewportCommand::Minimized(true)),
        (rgb(0x28c840), "zoom", ViewportCommand::Maximized(!maximized)),
    ];
    let group = Rect::from_min_max(pos2(rect.min.x + 8.0, rect.min.y), pos2(rect.min.x + 76.0, rect.max.y));
    let group_hovered = ui.rect_contains_pointer(group);
    for (i, (color, name, command)) in lights.into_iter().enumerate() {
        let center = pos2(rect.min.x + 18.0 + i as f32 * 20.0, y);
        let hit = Rect::from_center_size(center, vec2(18.0, 18.0));
        let response = ui.interact(hit, Id::unique(("traffic-light", name)), Sense::click());
        let fill = if focused || group_hovered { color } else { rgb(0x5a5d61) };
        painter.circle_filled(center, r, fill);
        painter.circle_stroke(center, r, Stroke::new(0.5, Color32::from_black_alpha(50)));
        if group_hovered {
            let glyph_color = Color32::from_black_alpha(150);
            let s = Stroke::new(1.2, glyph_color);
            match i {
                0 => {
                    painter.line_segment([center + vec2(-2.5, -2.5), center + vec2(2.5, 2.5)], s);
                    painter.line_segment([center + vec2(-2.5, 2.5), center + vec2(2.5, -2.5)], s);
                }
                1 => {
                    painter.line_segment([center + vec2(-3.0, 0.0), center + vec2(3.0, 0.0)], s);
                }
                _ => {
                    painter.line_segment([center + vec2(-3.0, 0.0), center + vec2(3.0, 0.0)], s);
                    painter.line_segment([center + vec2(0.0, -3.0), center + vec2(0.0, 3.0)], s);
                }
            }
        }
        if response.clicked() {
            ui.ctx().send_viewport_cmd(command);
        }
    }

    let font = FontId::proportional(13.0);
    let text = fit_text(ui, title, &font, rect.width() - 200.0);
    let color = if focused { colors.title_text } else { colors.title_text_inactive };
    painter.text(rect.center(), Align2::CENTER_CENTER, text, font, color);
}

pub struct TabInfo {
    pub title: String,
    pub active: bool,
    pub activity: bool,
    pub panes: usize,
    pub shortcut: Option<String>,
}

pub enum TabBarAction {
    Select(usize),
    Close(usize),
    New,
}

/// Capsule tab bar; tabs stretch to fill the bar like iTerm2's default.
pub fn tab_bar(ui: &Ui, rect: Rect, tabs: &[TabInfo], colors: &ChromeColors, draggable: bool) -> Option<TabBarAction> {
    let painter = ui.painter();
    let mut action = None;
    if draggable {
        // Empty space in the tab bar also moves the window.
        let bg = ui.interact(rect, Id::unique("tab-bar-bg"), Sense::click_and_drag());
        if bg.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            action = Some(TabBarAction::New);
        }
    }

    let height = 28.0;
    let plus_d = height;
    let y = rect.center().y;
    let plus_center = pos2(rect.max.x - 10.0 - plus_d / 2.0, y);
    let container = Rect::from_min_max(
        pos2(rect.min.x + 10.0, y - height / 2.0),
        pos2(plus_center.x - plus_d / 2.0 - 8.0, y + height / 2.0),
    );
    let radius = CornerRadius::same((height / 2.0) as u8);
    painter.rect_filled(container, radius, colors.tab_container);

    let n = tabs.len().max(1) as f32;
    let tab_w = container.width() / n;
    let font = FontId::proportional(13.0);
    let small = FontId::proportional(11.5);
    for (i, tab) in tabs.iter().enumerate() {
        let r = Rect::from_min_size(pos2(container.min.x + i as f32 * tab_w, container.min.y), vec2(tab_w, height));
        let response = ui.interact(r, Id::unique(("tab", i)), Sense::click());
        let hovered = response.hovered();
        if tab.active {
            let pill = r.shrink(2.0);
            let pill_radius = CornerRadius::same((pill.height() / 2.0) as u8);
            painter.rect_filled(pill, pill_radius, colors.tab_active);
            painter.rect_stroke(
                pill,
                pill_radius,
                Stroke::new(1.0, colors.tab_active_highlight.gamma_multiply(0.6)),
                StrokeKind::Inside,
            );
        } else if hovered {
            let pill = r.shrink(2.0);
            painter.rect_filled(
                pill,
                CornerRadius::same((pill.height() / 2.0) as u8),
                colors.tab_active.gamma_multiply(0.35),
            );
        }
        // Separators between neighbouring inactive tabs.
        if i > 0 && !tab.active && !tabs[i - 1].active {
            painter.vline(r.min.x, (y - 7.0)..=(y + 7.0), Stroke::new(1.0, colors.tab_separator));
        }

        let text_color = if tab.active { colors.tab_text } else { colors.tab_text_inactive };
        let shortcut_w = if tab.shortcut.is_some() && tab_w > 140.0 { 44.0 } else { 0.0 };
        let mut title = tab.title.clone();
        if tab.panes > 1 {
            title = format!("{title} ({})", tab.panes);
        }
        let title = fit_text(ui, &title, &font, tab_w - 44.0 - shortcut_w);
        painter.text(pos2(r.center().x - shortcut_w / 2.0, y), Align2::CENTER_CENTER, title, font.clone(), text_color);
        if let Some(shortcut) = tab.shortcut.as_ref().filter(|_| shortcut_w > 0.0) {
            painter.text(
                pos2(r.max.x - 14.0, y),
                Align2::RIGHT_CENTER,
                shortcut,
                small.clone(),
                colors.tab_text_inactive,
            );
        }

        // Close button on hover (left), activity dot otherwise.
        let close_center = pos2(r.min.x + 16.0, y);
        if hovered || ui.rect_contains_pointer(Rect::from_center_size(close_center, vec2(18.0, 18.0))) {
            let close = ui.interact(
                Rect::from_center_size(close_center, vec2(18.0, 18.0)),
                Id::unique(("tab-close", i)),
                Sense::click(),
            );
            if close.hovered() {
                painter.circle_filled(close_center, 8.0, colors.tab_separator);
            }
            let s = Stroke::new(1.3, text_color);
            painter.line_segment([close_center + vec2(-3.5, -3.5), close_center + vec2(3.5, 3.5)], s);
            painter.line_segment([close_center + vec2(-3.5, 3.5), close_center + vec2(3.5, -3.5)], s);
            if close.clicked() {
                action = Some(TabBarAction::Close(i));
            }
        } else if tab.activity {
            painter.circle_filled(close_center, 3.5, colors.activity);
        }
        if response.clicked() && action.is_none() {
            action = Some(TabBarAction::Select(i));
        }
        if response.middle_clicked() {
            action = Some(TabBarAction::Close(i));
        }
    }

    let plus_rect = Rect::from_center_size(plus_center, vec2(plus_d, plus_d));
    let plus = ui.interact(plus_rect, Id::unique("tab-new"), Sense::click()).on_hover_text("New tab");
    let fill = if plus.hovered() { colors.tab_active } else { colors.tab_container };
    painter.circle_filled(plus_center, plus_d / 2.0, fill);
    let s = Stroke::new(1.5, colors.tab_text);
    painter.line_segment([plus_center + vec2(-6.0, 0.0), plus_center + vec2(6.0, 0.0)], s);
    painter.line_segment([plus_center + vec2(0.0, -6.0), plus_center + vec2(0.0, 6.0)], s);
    if plus.clicked() {
        action = Some(TabBarAction::New);
    }
    action
}

pub enum PaneTitleAction {
    Focus,
    Close,
    Menu(Action),
}

/// iTerm2's per-pane title bar: close button, title, and a menu button.
pub fn pane_title(
    ui: &Ui,
    rect: Rect,
    id: PaneId,
    title: &str,
    active: bool,
    colors: &ChromeColors,
    bindings: &Bindings,
) -> Option<PaneTitleAction> {
    let painter = ui.painter();
    let bg = if active { colors.pane_title_active } else { colors.pane_title_inactive };
    painter.rect_filled(rect, 0.0, bg);
    painter.hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, colors.pane_title_border));
    let text_color = if active { colors.pane_title_text } else { colors.pane_title_text_inactive };

    let mut action = None;
    let bar = ui.interact(rect, Id::unique(("pane-title", id)), Sense::click());
    if bar.clicked() {
        action = Some(PaneTitleAction::Focus);
    }

    let close_center = pos2(rect.min.x + 13.0, rect.center().y);
    let close = ui.interact(
        Rect::from_center_size(close_center, vec2(16.0, 16.0)),
        Id::unique(("pane-close", id)),
        Sense::click(),
    );
    let close_color = if close.hovered() { text_color } else { text_color.gamma_multiply(0.7) };
    let s = Stroke::new(1.2, close_color);
    painter.line_segment([close_center + vec2(-4.0, -4.0), close_center + vec2(4.0, 4.0)], s);
    painter.line_segment([close_center + vec2(-4.0, 4.0), close_center + vec2(4.0, -4.0)], s);
    if close.clicked() {
        action = Some(PaneTitleAction::Close);
    }

    let menu_center = pos2(rect.max.x - 14.0, rect.center().y);
    let menu = ui.interact(
        Rect::from_center_size(menu_center, vec2(18.0, 16.0)),
        Id::unique(("pane-menu", id)),
        Sense::click(),
    );
    for dy in [-4.0, 0.0, 4.0] {
        painter.hline((menu_center.x - 6.0)..=(menu_center.x + 6.0), menu_center.y + dy, Stroke::new(1.3, close_color));
    }
    egui::Popup::menu(&menu).show(|ui| {
        if let Some(a) = pane_menu(ui, bindings, false) {
            action = Some(PaneTitleAction::Menu(a));
        }
    });

    let font = FontId::proportional(12.5);
    let text = fit_text(ui, title, &font, rect.width() - 60.0);
    painter.text(pos2(rect.min.x + 26.0, rect.center().y), Align2::LEFT_CENTER, text, font, text_color);
    action
}

/// Entries shared by the terminal's context menu and the pane title bar menu.
pub fn pane_menu(ui: &mut Ui, bindings: &Bindings, has_selection: bool) -> Option<Action> {
    let mut chosen = None;
    let mut item = |ui: &mut Ui, label: &str, action: Action, enabled: bool| {
        let mut button = egui::Button::new(label);
        if let Some(shortcut) = bindings.label(action) {
            button = button.shortcut_text(shortcut);
        }
        if ui.add_enabled(enabled, button).clicked() {
            chosen = Some(action);
            ui.close();
        }
    };
    item(ui, "Copy", Action::Copy, has_selection);
    item(ui, "Paste", Action::Paste, true);
    item(ui, "Find…", Action::Find, true);
    ui.separator();
    item(ui, "Split Pane Vertically", Action::Split(Axis::Horizontal), true);
    item(ui, "Split Pane Horizontally", Action::Split(Axis::Vertical), true);
    item(ui, "Maximize Pane", Action::MaximizePane, true);
    item(ui, "New Tab", Action::NewTab, true);
    ui.separator();
    item(ui, "Clear Buffer", Action::ClearBuffer, true);
    item(ui, "Close Pane", Action::ClosePane, true);
    ui.separator();
    item(ui, "Settings…", Action::OpenSettings, true);
    chosen
}

/// Invisible handles along the window edges for resizing an undecorated window.
pub fn resize_edges(ui: &Ui, rect: Rect) {
    let e = RESIZE_EDGE;
    let corner = e * 3.0;
    let regions = [
        (
            Rect::from_min_max(rect.min, pos2(rect.min.x + corner, rect.min.y + corner)),
            ResizeDirection::NorthWest,
            CursorIcon::ResizeNorthWest,
        ),
        (
            Rect::from_min_max(pos2(rect.max.x - corner, rect.min.y), pos2(rect.max.x, rect.min.y + corner)),
            ResizeDirection::NorthEast,
            CursorIcon::ResizeNorthEast,
        ),
        (
            Rect::from_min_max(pos2(rect.min.x, rect.max.y - corner), pos2(rect.min.x + corner, rect.max.y)),
            ResizeDirection::SouthWest,
            CursorIcon::ResizeSouthWest,
        ),
        (
            Rect::from_min_max(pos2(rect.max.x - corner, rect.max.y - corner), rect.max),
            ResizeDirection::SouthEast,
            CursorIcon::ResizeSouthEast,
        ),
        (
            Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + e)),
            ResizeDirection::North,
            CursorIcon::ResizeNorth,
        ),
        (
            Rect::from_min_max(pos2(rect.min.x, rect.max.y - e), rect.max),
            ResizeDirection::South,
            CursorIcon::ResizeSouth,
        ),
        (Rect::from_min_max(rect.min, pos2(rect.min.x + e, rect.max.y)), ResizeDirection::West, CursorIcon::ResizeWest),
        (Rect::from_min_max(pos2(rect.max.x - e, rect.min.y), rect.max), ResizeDirection::East, CursorIcon::ResizeEast),
    ];
    for (i, (r, direction, icon)) in regions.into_iter().enumerate() {
        let response = ui.interact(r, Id::unique(("resize-edge", i)), Sense::drag()).on_hover_cursor(icon);
        if response.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::BeginResize(direction));
        }
    }
}

pub enum FindBarAction {
    Previous,
    Next,
    Close,
}

/// iTerm2's floating find bar, anchored to the top-right corner of the active pane.
pub fn find_bar(
    ctx: &egui::Context,
    anchor: egui::Pos2,
    find: &mut Find,
    colors: &ChromeColors,
) -> Option<FindBarAction> {
    let mut action = None;
    egui::Area::new(Id::unique("find-bar"))
        .order(egui::Order::Foreground)
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(anchor)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(colors.tab_container)
                .stroke(Stroke::new(1.0, colors.tab_separator))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 4],
                    blur: 14,
                    spread: 0,
                    color: Color32::from_black_alpha(90),
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        // Magnifier.
                        let (r, _) = ui.allocate_exact_size(vec2(16.0, 20.0), Sense::hover());
                        let c = r.center() - vec2(1.5, 1.5);
                        let s = Stroke::new(1.5, colors.tab_text_inactive);
                        ui.painter().circle_stroke(c, 5.0, s);
                        ui.painter().line_segment([c + vec2(3.6, 3.6), c + vec2(7.0, 7.0)], s);

                        let text_color =
                            if find.invalid { Color32::from_rgb(0xf0, 0x52, 0x4f) } else { colors.tab_text };
                        let field = egui::TextEdit::singleline(&mut find.query)
                            .id(Id::unique("find-field"))
                            .hint_text("Find")
                            .desired_width(200.0)
                            .text_color(text_color);
                        let response = ui.add(field);
                        // Keep typing in the field while the bar is open (Enter would drop focus).
                        if find.focus || !response.has_focus() {
                            response.request_focus();
                            find.focus = false;
                        }

                        let counter = find.counter();
                        ui.add_sized(
                            vec2(64.0, 20.0),
                            egui::Label::new(RichText::new(counter).small().color(colors.tab_text_inactive)),
                        );

                        let case = ui
                            .selectable_label(find.case_sensitive, RichText::new("Aa").small())
                            .on_hover_text("Case sensitive (off: smart case)");
                        if case.clicked() {
                            find.case_sensitive = !find.case_sensitive;
                        }
                        let regex = ui
                            .selectable_label(find.regex, RichText::new(".*").small().monospace())
                            .on_hover_text("Regular expression");
                        if regex.clicked() {
                            find.regex = !find.regex;
                        }
                        ui.separator();
                        if icon_button(ui, "find-up", colors, |p, c, s| {
                            p.line_segment([c + vec2(-4.5, 2.0), c + vec2(0.0, -2.5)], s);
                            p.line_segment([c + vec2(0.0, -2.5), c + vec2(4.5, 2.0)], s);
                        })
                        .on_hover_text("Previous (up) — Enter")
                        .clicked()
                        {
                            action = Some(FindBarAction::Previous);
                        }
                        if icon_button(ui, "find-down", colors, |p, c, s| {
                            p.line_segment([c + vec2(-4.5, -2.0), c + vec2(0.0, 2.5)], s);
                            p.line_segment([c + vec2(0.0, 2.5), c + vec2(4.5, -2.0)], s);
                        })
                        .on_hover_text("Next (down) — Shift+Enter")
                        .clicked()
                        {
                            action = Some(FindBarAction::Next);
                        }
                        if icon_button(ui, "find-close", colors, |p, c, s| {
                            p.line_segment([c + vec2(-4.0, -4.0), c + vec2(4.0, 4.0)], s);
                            p.line_segment([c + vec2(-4.0, 4.0), c + vec2(4.0, -4.0)], s);
                        })
                        .on_hover_text("Close — Esc")
                        .clicked()
                        {
                            action = Some(FindBarAction::Close);
                        }
                    });
                });
        });
    action
}

fn icon_button(
    ui: &mut Ui,
    id: &str,
    colors: &ChromeColors,
    draw: impl FnOnce(&egui::Painter, egui::Pos2, Stroke),
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
    let response = ui.interact(rect, Id::unique(("find-button", id)), Sense::click());
    if response.hovered() {
        ui.painter().circle_filled(rect.center(), 11.0, colors.tab_active);
    }
    draw(ui.painter(), rect.center(), Stroke::new(1.5, colors.tab_text));
    response
}
