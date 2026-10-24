//! Draws a pane's terminal grid with egui shapes.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::search::Match;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
use eframe::egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2, pos2, vec2};

use crate::config::Palette;
use crate::pane::Engine;
use crate::shell_integration::{Mark, ShellState};

/// Left margin holding prompt marks.
const GUTTER: f32 = 12.0;
const PAD_RIGHT: f32 = 6.0;
const PAD_Y: f32 = 3.0;

const MARK_PROMPT: Color32 = Color32::from_rgb(0x6e, 0x73, 0x7a);
const MARK_RUNNING: Color32 = Color32::from_rgb(0x32, 0x8e, 0xee);
const MARK_OK: Color32 = Color32::from_rgb(0x3f, 0xb9, 0x50);
const MARK_FAILED: Color32 = Color32::from_rgb(0xf0, 0x52, 0x4f);
/// Find matches other than the current one (iTerm2 highlights them yellow).
const FIND_MATCH: Color32 = Color32::from_rgb(0xff, 0xe9, 0x4d);

/// Everything about appearance that the renderer needs.
pub struct Style<'a> {
    pub palette: &'a Palette,
    pub font: FontId,
    pub bold_font: FontId,
    pub cell: Vec2,
    /// Vertical offset of glyphs inside a cell (from `line_height` > 1).
    pub text_offset: f32,
    pub bold_is_bright: bool,
}

/// The area holding character cells inside a pane's rect.
pub fn grid_rect(rect: Rect) -> Rect {
    Rect::from_min_max(pos2(rect.min.x + GUTTER, rect.min.y + PAD_Y), pos2(rect.max.x - PAD_RIGHT, rect.max.y - PAD_Y))
}

/// Columns and rows that fit in a pane's rect.
pub fn grid_size(rect: Rect, cell: Vec2) -> (u16, u16) {
    let g = grid_rect(rect);
    let cols = (g.width() / cell.x).floor().max(2.0) as u16;
    let rows = (g.height() / cell.y).floor().max(1.0) as u16;
    (cols, rows)
}

/// Window size (in points, excluding chrome) for a grid of `cols` x `rows`.
pub fn pane_size_for(cols: u16, rows: u16, cell: Vec2) -> Vec2 {
    vec2(cols as f32 * cell.x + GUTTER + PAD_RIGHT, rows as f32 * cell.y + 2.0 * PAD_Y)
}

/// Viewport cell (col, row) under `pos`, clamped to the grid.
pub fn cell_at(rect: Rect, cell: Vec2, pos: Pos2, engine: &Engine) -> (usize, usize, Side) {
    let g = grid_rect(rect);
    let term = &engine.term;
    let x = ((pos.x - g.min.x) / cell.x).max(0.0);
    let col = (x as usize).min(term.columns() - 1);
    let side = if x.fract() < 0.5 { Side::Left } else { Side::Right };
    let row = ((pos.y - g.min.y) / cell.y).floor().max(0.0) as usize;
    (col, row.min(term.screen_lines() - 1), side)
}

/// Grid point and cell side under `pos`.
pub fn point_at(rect: Rect, cell: Vec2, pos: Pos2, engine: &Engine) -> (Point, Side) {
    let (col, row, side) = cell_at(rect, cell, pos, engine);
    let line = Line(row as i32 - engine.term.grid().display_offset() as i32);
    (Point::new(line, Column(col)), side)
}

fn rgb(c: Rgb) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

fn dim(c: Color32) -> Color32 {
    Color32::from_rgb((c.r() as f32 * 0.66) as u8, (c.g() as f32 * 0.66) as u8, (c.b() as f32 * 0.66) as u8)
}

fn indexed(i: usize, colors: &Colors, p: &Palette) -> Color32 {
    if let Some(c) = colors[i] {
        return rgb(c);
    }
    match i {
        0..16 => p.ansi[i],
        16..232 => {
            let i = i - 16;
            let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
            Color32::from_rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        232..256 => {
            let v = (8 + (i - 232) * 10) as u8;
            Color32::from_rgb(v, v, v)
        }
        _ => p.fg,
    }
}

fn named(n: NamedColor, colors: &Colors, p: &Palette) -> Color32 {
    if let Some(c) = colors[n] {
        return rgb(c);
    }
    match n {
        NamedColor::Foreground => p.fg,
        NamedColor::BrightForeground => p.bold,
        NamedColor::Background => p.bg,
        NamedColor::Cursor => p.cursor,
        NamedColor::DimForeground => dim(p.fg),
        NamedColor::DimBlack
        | NamedColor::DimRed
        | NamedColor::DimGreen
        | NamedColor::DimYellow
        | NamedColor::DimBlue
        | NamedColor::DimMagenta
        | NamedColor::DimCyan
        | NamedColor::DimWhite => dim(p.ansi[n as usize - NamedColor::DimBlack as usize]),
        _ => indexed(n as usize, colors, p),
    }
}

fn resolve(c: Color, colors: &Colors, p: &Palette) -> Color32 {
    match c {
        Color::Named(n) => named(n, colors, p),
        Color::Spec(rgb_) => rgb(rgb_),
        Color::Indexed(i) => indexed(i as usize, colors, p),
    }
}

fn cell_colors(cell: &Cell, colors: &Colors, style: &Style<'_>) -> (Color32, Color32) {
    let p = style.palette;
    let bold = cell.flags.contains(Flags::BOLD);
    let mut fg = match cell.fg {
        Color::Named(NamedColor::Foreground) if bold => named(NamedColor::BrightForeground, colors, p),
        // iTerm2's "Brighten bold text": bold base colors use their bright variants.
        Color::Named(n) if bold && style.bold_is_bright && (n as usize) < 8 => indexed(n as usize + 8, colors, p),
        Color::Indexed(i) if bold && style.bold_is_bright && i < 8 => indexed(i as usize + 8, colors, p),
        c => resolve(c, colors, p),
    };
    if cell.flags.contains(Flags::DIM) {
        fg = dim(fg);
    }
    let mut bg = resolve(cell.bg, colors, p);
    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.flags.contains(Flags::HIDDEN) {
        fg = bg;
    }
    (fg, bg)
}

/// A run of single-width ASCII cells sharing color and weight, drawn as one text shape.
struct Run {
    row: i32,
    next_col: usize,
    pos: Pos2,
    text: String,
    color: Color32,
    bold: bool,
}

/// A link range on one viewport row, underlined while Ctrl is held over it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LinkSpan {
    pub row: usize,
    pub start: usize,
    pub end: usize,
}

/// Find matches to highlight in a pane (sorted, non-overlapping) and the current one.
pub struct SearchView<'a> {
    pub matches: &'a [Match],
    pub current: Option<usize>,
}

pub struct PaneView<'a> {
    pub focused: bool,
    /// 0.0 = not dimmed; iTerm2-style dimming blends towards gray.
    pub dim: f32,
    pub corners: CornerRadius,
    pub link: Option<LinkSpan>,
    pub search: Option<SearchView<'a>>,
}

pub fn draw(painter: &Painter, rect: Rect, style: &Style<'_>, engine: &Engine, view: &PaneView<'_>) {
    let term = &engine.term;
    let content = term.renderable_content();
    let colors = content.colors;
    let offset = content.display_offset as i32;
    let rows = term.screen_lines() as i32;
    // Matches overlapping the viewport, with their index (to recognize the current one).
    let visible_matches: Vec<(usize, &Match)> = match &view.search {
        Some(s) => {
            let first = s.matches.partition_point(|m| m.end().line.0 < -offset);
            s.matches[first..]
                .iter()
                .enumerate()
                .take_while(|(_, m)| m.start().line.0 < rows - offset)
                .map(|(i, m)| (first + i, m))
                .collect()
        }
        None => Vec::new(),
    };
    let current_match = view.search.as_ref().and_then(|s| s.current);
    let origin = grid_rect(rect).min;
    let cell = style.cell;
    let default_bg = named(NamedColor::Background, colors, style.palette);

    painter.rect_filled(rect, view.corners, default_bg);

    let cell_rect = |row: i32, col: usize, width: usize| {
        Rect::from_min_size(
            pos2(origin.x + col as f32 * cell.x, origin.y + row as f32 * cell.y),
            vec2(cell.x * width as f32, cell.y),
        )
    };

    let mut backgrounds: Vec<(Rect, Color32)> = Vec::new();
    let mut lines: Vec<(Rect, Color32, Flags)> = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut glyphs: Vec<(Rect, String, Color32, bool)> = Vec::new();

    for item in content.display_iter {
        let point = item.point;
        let row = point.line.0 + offset;
        if !(0..rows).contains(&row) {
            continue;
        }
        let cell_data = item.cell;
        let flags = cell_data.flags;
        if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        let col = point.column.0;
        let width = if flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
        let (mut fg, mut bg) = cell_colors(cell_data, colors, style);
        if content.selection.is_some_and(|s| s.contains(point)) {
            bg = style.palette.selection;
            fg = style.palette.selection_text;
        }
        if let Some((i, _)) = visible_matches.iter().find(|(_, m)| m.contains(&point)) {
            // Current match looks selected (as in iTerm2), the others yellow.
            if Some(*i) == current_match {
                bg = style.palette.selection;
                fg = style.palette.selection_text;
            } else {
                bg = FIND_MATCH;
                fg = Color32::BLACK;
            }
        }
        let r = cell_rect(row, col, width);

        if bg != default_bg {
            match backgrounds.last_mut() {
                Some((prev, color)) if *color == bg && prev.max.x == r.min.x && prev.min.y == r.min.y => {
                    prev.max.x = r.max.x;
                }
                _ => backgrounds.push((r, bg)),
            }
        }
        if flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT) {
            lines.push((r, fg, flags));
        }

        let c = cell_data.c;
        if c == ' ' || c == '\t' || flags.contains(Flags::HIDDEN) {
            continue;
        }
        let bold = flags.contains(Flags::BOLD);
        let zerowidth = cell_data.zerowidth();
        if c.is_ascii_graphic() && width == 1 && zerowidth.is_none() {
            match runs.last_mut() {
                Some(run) if run.row == row && run.next_col == col && run.color == fg && run.bold == bold => {
                    run.text.push(c);
                    run.next_col += 1;
                }
                _ => runs.push(Run { row, next_col: col + 1, pos: r.min, text: c.to_string(), color: fg, bold }),
            }
        } else {
            let mut text = c.to_string();
            text.extend(zerowidth.into_iter().flatten());
            glyphs.push((r, text, fg, bold));
        }
    }

    let font_for = |bold: bool| if bold { style.bold_font.clone() } else { style.font.clone() };
    for (r, color) in backgrounds {
        painter.rect_filled(r, 0.0, color);
    }
    for run in runs {
        let pos = run.pos + vec2(0.0, style.text_offset);
        painter.text(pos, Align2::LEFT_TOP, run.text, font_for(run.bold), run.color);
    }
    for (r, text, color, bold) in glyphs {
        let pos = pos2(r.center().x, r.min.y + style.text_offset);
        painter.text(pos, Align2::CENTER_TOP, text, font_for(bold), color);
    }
    for (r, color, flags) in lines {
        let stroke = Stroke::new(1.0, color);
        if flags.intersects(Flags::ALL_UNDERLINES) {
            painter.hline(r.x_range(), r.max.y - 1.5, stroke);
        }
        if flags.contains(Flags::STRIKEOUT) {
            painter.hline(r.x_range(), r.center().y, stroke);
        }
    }
    if let Some(link) = view.link {
        let r = cell_rect(link.row as i32, link.start, link.end - link.start + 1);
        painter.hline(r.x_range(), r.max.y - 1.5, Stroke::new(1.0, style.palette.link));
    }

    draw_cursor(painter, engine, &content.cursor, offset, rows, style, cell_rect, view.focused);
    draw_marks(painter, rect, cell, origin, engine);

    if offset > 0 {
        let label = format!("↑ {offset}");
        // Terminal font: UI fonts often lack the arrow glyph.
        let small = FontId::monospace(style.font.size * 0.8);
        // Bottom-right, so it never hides under the find bar.
        painter.text(
            pos2(rect.max.x - PAD_RIGHT - 4.0, rect.max.y - PAD_Y),
            Align2::RIGHT_BOTTOM,
            label,
            small,
            MARK_RUNNING,
        );
    }
    if let Some(search) = &view.search {
        draw_search_marks(painter, rect, engine, search);
    }
    if view.dim > 0.0 {
        let alpha = (view.dim.clamp(0.0, 1.0) * 255.0) as u8;
        painter.rect_filled(rect, view.corners, Color32::from_rgba_unmultiplied(128, 128, 128, alpha));
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_cursor(
    painter: &Painter,
    engine: &Engine,
    cursor: &alacritty_terminal::term::RenderableCursor,
    offset: i32,
    rows: i32,
    style: &Style<'_>,
    cell_rect: impl Fn(i32, usize, usize) -> Rect,
    focused: bool,
) {
    let row = cursor.point.line.0 + offset;
    if cursor.shape == CursorShape::Hidden || !(0..rows).contains(&row) {
        return;
    }
    let term = &engine.term;
    let under = &term.grid()[cursor.point];
    let width = if under.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
    let r = cell_rect(row, cursor.point.column.0, width);
    let color = named(NamedColor::Cursor, term.colors(), style.palette);
    if !focused {
        painter.rect_stroke(r.shrink(0.5), 0.0, Stroke::new(1.0, color), StrokeKind::Inside);
        return;
    }
    match cursor.shape {
        CursorShape::Beam => {
            painter.rect_filled(Rect::from_min_size(r.min, vec2(2.0, r.height())), 0.0, color);
        }
        CursorShape::Underline => {
            painter.rect_filled(Rect::from_min_max(pos2(r.min.x, r.max.y - 2.0), r.max), 0.0, color);
        }
        CursorShape::HollowBlock => {
            painter.rect_stroke(r.shrink(0.5), 0.0, Stroke::new(1.0, color), StrokeKind::Inside);
        }
        _ => {
            painter.rect_filled(r, 0.0, color);
            if under.c != ' ' {
                let font = if under.flags.contains(Flags::BOLD) { &style.bold_font } else { &style.font };
                let pos = pos2(r.center().x, r.min.y + style.text_offset);
                painter.text(pos, Align2::CENTER_TOP, under.c, font.clone(), style.palette.cursor_text);
            }
        }
    }
}

/// Ticks along the right edge showing where matches are in the whole buffer.
fn draw_search_marks(painter: &Painter, rect: Rect, engine: &Engine, search: &SearchView<'_>) {
    let term = &engine.term;
    let history = term.grid().history_size() as f32;
    let total = history + term.screen_lines() as f32;
    let x = rect.max.x - 4.0;
    let mut last_y = f32::NEG_INFINITY;
    for (i, m) in search.matches.iter().enumerate() {
        let current = Some(i) == search.current;
        let y = rect.min.y + (history + m.start().line.0 as f32) / total * rect.height();
        // Skip ticks that would overlap the previous one, except the current match.
        if y - last_y < 2.0 && !current {
            continue;
        }
        last_y = y;
        let color = if current { MARK_RUNNING } else { FIND_MATCH.gamma_multiply(0.85) };
        let width = if current { 4.0 } else { 3.0 };
        painter.rect_filled(Rect::from_min_size(pos2(x - width + 3.0, y), vec2(width, 2.0)), 0.0, color);
    }
}

fn mark_color(mark: &Mark) -> Color32 {
    match (mark.executed, mark.exit) {
        (_, Some(0)) => MARK_OK,
        (_, Some(_)) => MARK_FAILED,
        (true, None) => MARK_RUNNING,
        (false, None) => MARK_PROMPT,
    }
}

/// Prompt marks in the left gutter, like iTerm2's mark indicators.
fn draw_marks(painter: &Painter, rect: Rect, cell: Vec2, origin: Pos2, engine: &Engine) {
    let top = ShellState::viewport_top(&engine.term);
    let rows = engine.term.screen_lines() as i64;
    let start = engine.shell.marks.partition_point(|m| m.line < top);
    for mark in engine.shell.marks[start..].iter().take_while(|m| m.line < top + rows) {
        let y = origin.y + (mark.line - top) as f32 * cell.y + cell.y / 2.0;
        let center = pos2(rect.min.x + GUTTER / 2.0, y);
        painter.circle_filled(center, (cell.y * 0.17).clamp(2.0, 3.5), mark_color(mark));
    }
}

/// URL under a viewport cell: an OSC 8 hyperlink, or a plain-text http(s)/file URL.
pub fn link_at(engine: &Engine, col: usize, row: usize) -> Option<(String, LinkSpan)> {
    let term = &engine.term;
    let line = Line(row as i32 - term.grid().display_offset() as i32);
    let cols = term.columns();
    let grid = term.grid();
    if let Some(link) = grid[line][Column(col)].hyperlink() {
        let same = |c: usize| grid[line][Column(c)].hyperlink().is_some_and(|h| h == link);
        let start = (0..=col).rev().take_while(|&c| same(c)).last().unwrap_or(col);
        let end = (col..cols).take_while(|&c| same(c)).last().unwrap_or(col);
        return Some((link.uri().to_owned(), LinkSpan { row, start, end }));
    }
    let chars: Vec<char> = (0..cols).map(|c| grid[line][Column(c)].c).collect();
    let is_url_char = |c: char| !c.is_whitespace() && !matches!(c, '"' | '\'' | '<' | '>' | '`' | '|' | '\0');
    if !is_url_char(chars[col]) {
        return None;
    }
    let mut start = col;
    while start > 0 && is_url_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end + 1 < cols && is_url_char(chars[end + 1]) {
        end += 1;
    }
    let token: String = chars[start..=end].iter().collect();
    let offset = ["https://", "http://", "file://"].iter().filter_map(|scheme| token.find(scheme)).min()?;
    let start = start + token[..offset].chars().count();
    let mut url: String = token[offset..].to_owned();
    // Trailing punctuation usually belongs to the sentence, not the URL.
    while url.ends_with(['.', ',', ';', ':', ')', ']', '}', '!', '?']) {
        url.pop();
    }
    let end = start + url.chars().count() - 1;
    (col >= start && col <= end).then_some((url, LinkSpan { row, start, end }))
}
