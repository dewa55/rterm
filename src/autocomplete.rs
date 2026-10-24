//! iTerm2-style autocomplete (Ctrl+;): completes the word before the cursor with words seen
//! earlier in the pane (screen and scrollback), nearest first.

use std::collections::HashSet;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use eframe::egui::{self, Align2, Color32, FontId, Id, Pos2, RichText, Sense, Stroke, vec2};

use crate::chrome::ChromeColors;
use crate::layout::PaneId;
use crate::pane::Engine;

/// Rows shown in the popup at once.
const VISIBLE: usize = 10;
/// Distinct candidates kept per invocation.
const MAX_CANDIDATES: usize = 500;
const MAX_TOKEN_LEN: usize = 200;

/// Characters that separate words. `:` `/` `.` `-` `@` stay inside words so paths, URLs,
/// hosts and hashes complete as one unit.
fn is_delimiter(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '|' | ';' | '&' | ',' | '=' | '$' | '\0'
        )
}

/// Words in `line`, right to left, with sentence punctuation trimmed from their ends.
fn tokens(line: &str) -> Vec<&str> {
    let mut out: Vec<&str> = line
        .split(is_delimiter)
        .map(|t| t.trim_end_matches(['.', ',', ':', '!', '?']))
        .filter(|t| !t.is_empty() && t.chars().count() <= MAX_TOKEN_LEN)
        .collect();
    out.reverse();
    out
}

/// The partial word ending at `col` (exclusive) in `chars`.
fn prefix_before(chars: &[char], col: usize) -> String {
    let end = col.min(chars.len());
    let start = chars[..end].iter().rposition(|c| is_delimiter(*c)).map_or(0, |i| i + 1);
    chars[start..end].iter().collect()
}

/// Smart case: case-insensitive unless `prefix` contains an uppercase letter.
fn matches_prefix(word: &str, prefix: &str) -> bool {
    if prefix.chars().any(char::is_uppercase) {
        word.starts_with(prefix)
    } else {
        word.to_lowercase().starts_with(&prefix.to_lowercase())
    }
}

/// Ranks words by distance from the cursor line: `lines` is ordered nearest first.
fn rank<'a>(lines: impl IntoIterator<Item = &'a str>, prefix: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in lines {
        for t in tokens(line) {
            if t.chars().count() > prefix.chars().count() && matches_prefix(t, prefix) && seen.insert(t.to_owned()) {
                out.push(t.to_owned());
                if out.len() >= MAX_CANDIDATES {
                    return out;
                }
            }
        }
    }
    out
}

/// One logical line per wrapped run of rows, with the grid line it starts on.
fn logical_lines(engine: &Engine) -> Vec<(i32, String)> {
    let term = &engine.term;
    let grid = term.grid();
    let cols = term.columns();
    let mut out: Vec<(i32, String)> = Vec::new();
    let mut continues = false;
    for l in term.topmost_line().0..=term.bottommost_line().0 {
        let row = &grid[Line(l)];
        let mut text = String::with_capacity(cols);
        for c in 0..cols {
            let cell = &row[Column(c)];
            if !cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                text.push(cell.c);
            }
        }
        let wraps = row[Column(cols - 1)].flags.contains(Flags::WRAPLINE);
        match out.last_mut() {
            Some((_, prev)) if continues => prev.push_str(&text),
            _ => out.push((l, text)),
        }
        continues = wraps;
    }
    for (_, text) in &mut out {
        let trimmed = text.trim_end().len();
        text.truncate(trimmed);
    }
    out
}

pub struct Autocomplete {
    pub open: bool,
    pane: PaneId,
    /// The partial word already typed in the shell.
    prefix: String,
    /// Characters typed while the popup is open (not yet sent to the shell).
    filter: String,
    candidates: Vec<String>,
    selected: usize,
    scroll: usize,
}

impl Autocomplete {
    pub fn new() -> Autocomplete {
        Autocomplete {
            open: false,
            pane: 0,
            prefix: String::new(),
            filter: String::new(),
            candidates: Vec::new(),
            selected: 0,
            scroll: 0,
        }
    }

    /// Collects candidates for the word before the cursor in `engine`.
    pub fn start(&mut self, pane: PaneId, engine: &Engine) {
        let term = &engine.term;
        let cursor = term.grid().cursor.point;
        let lines = logical_lines(engine);
        // The logical line holding the cursor, and the cursor's offset within it.
        let index = lines.iter().rposition(|(start, _)| *start <= cursor.line.0).unwrap_or(0);
        let (start_line, text) = &lines[index];
        let col = (cursor.line.0 - start_line) as usize * term.columns() + cursor.column.0;
        let chars: Vec<char> = text.chars().collect();
        let prefix = prefix_before(&chars, col);

        // Nearest first: text before the cursor on its line, then lines above, then below.
        let end = col.min(chars.len());
        let before_cursor: String = chars[..end - prefix.chars().count()].iter().collect();
        let above = lines[..index].iter().rev().map(|(_, t)| t.as_str());
        let below = lines[index + 1..].iter().map(|(_, t)| t.as_str());
        let ordered = std::iter::once(before_cursor.as_str()).chain(above).chain(below);
        self.candidates = rank(ordered, &prefix);
        self.prefix = prefix;
        self.filter.clear();
        self.selected = 0;
        self.scroll = 0;
        self.pane = pane;
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.candidates.clear();
    }

    pub fn pane(&self) -> PaneId {
        self.pane
    }

    fn typed(&self) -> String {
        format!("{}{}", self.prefix, self.filter)
    }

    fn visible(&self) -> Vec<&String> {
        let typed = self.typed();
        self.candidates
            .iter()
            .filter(|c| c.chars().count() > typed.chars().count() && matches_prefix(c, &typed))
            .collect()
    }

    /// Typed text narrows the list; returns false (and closes) if it ends the word.
    pub fn type_text(&mut self, text: &str) -> bool {
        if text.chars().any(is_delimiter) {
            self.close();
            return false;
        }
        self.filter.push_str(text);
        self.selected = 0;
        self.scroll = 0;
        true
    }

    /// Backspace removes a filter character, or closes when there is none.
    pub fn backspace(&mut self) -> bool {
        if self.filter.pop().is_none() {
            self.close();
            return false;
        }
        self.selected = 0;
        self.scroll = 0;
        true
    }

    pub fn move_selection(&mut self, delta: isize) {
        let n = self.visible().len();
        if n == 0 {
            return;
        }
        self.selected = (self.selected as isize + delta).rem_euclid(n as isize) as usize;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + VISIBLE {
            self.scroll = self.selected + 1 - VISIBLE;
        }
    }

    /// Bytes to send to the shell for the selected (or `index`) candidate, then closes.
    /// The prefix is already in the shell; filter characters are not.
    pub fn accept(&mut self, index: Option<usize>) -> Option<Vec<u8>> {
        let visible = self.visible();
        let choice = visible.get(index.unwrap_or(self.selected))?.to_string();
        let prefix_len = self.prefix.chars().count();
        let bytes = if choice.starts_with(&self.prefix) {
            choice.chars().skip(prefix_len).collect::<String>().into_bytes()
        } else {
            // Different case than typed: erase the typed prefix and insert the whole word.
            let mut b = vec![0x7f; prefix_len];
            b.extend_from_slice(choice.as_bytes());
            b
        };
        self.close();
        Some(bytes)
    }

    /// Draws the popup below (or above) `anchor` (the cursor cell's bottom-left).
    /// Returns a clicked row.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        anchor: Pos2,
        cell_height: f32,
        font_size: f32,
        colors: &ChromeColors,
    ) -> Option<usize> {
        let typed = self.typed();
        let visible: Vec<String> = self.visible().into_iter().cloned().collect();
        let font = FontId::monospace(font_size);
        let row_h = (font_size * 1.5).round();
        let rows = visible.len().clamp(1, VISIBLE);
        let height = rows as f32 * row_h + 34.0;
        let screen = ctx.content_rect();
        let below = anchor.y + height < screen.max.y;
        let pos = if below { anchor + vec2(-6.0, 4.0) } else { anchor - vec2(6.0, cell_height + 4.0) };
        let mut clicked = None;
        egui::Area::new(Id::unique("autocomplete"))
            .order(egui::Order::Foreground)
            .pivot(if below { Align2::LEFT_TOP } else { Align2::LEFT_BOTTOM })
            .fixed_pos(pos)
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(colors.tab_container)
                    .stroke(Stroke::new(1.0, colors.tab_separator))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(4, 4))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 4],
                        blur: 14,
                        spread: 0,
                        color: Color32::from_black_alpha(90),
                    })
                    .show(ui, |ui| {
                        let width = visible
                            .iter()
                            .take(50)
                            .map(|w| {
                                ui.fonts_mut(|f| f.layout_no_wrap(w.clone(), font.clone(), Color32::WHITE).size().x)
                            })
                            .fold(220.0f32, f32::max)
                            .min(560.0)
                            + 20.0;
                        if visible.is_empty() {
                            ui.add_sized(
                                vec2(width, row_h),
                                egui::Label::new(RichText::new("No suggestions").color(colors.tab_text_inactive)),
                            );
                        }
                        let typed_len = typed.chars().count();
                        for (i, word) in visible.iter().enumerate().skip(self.scroll).take(VISIBLE) {
                            let (rect, response) = ui.allocate_exact_size(vec2(width, row_h), Sense::click());
                            if i == self.selected {
                                ui.painter().rect_filled(rect, 5.0, colors.activity.gamma_multiply(0.85));
                            } else if response.hovered() {
                                ui.painter().rect_filled(rect, 5.0, colors.tab_active);
                            }
                            // Typed part dimmed, completion in full color.
                            let head: String = word.chars().take(typed_len).collect();
                            let tail: String = word.chars().skip(typed_len).collect();
                            let text_color = if i == self.selected { Color32::WHITE } else { colors.tab_text };
                            let mut job = egui::text::LayoutJob::default();
                            job.append(
                                &head,
                                0.0,
                                egui::TextFormat::simple(font.clone(), text_color.gamma_multiply(0.6)),
                            );
                            job.append(&tail, 0.0, egui::TextFormat::simple(font.clone(), text_color));
                            let galley = ui.fonts_mut(|f| f.layout_job(job));
                            ui.painter().galley(
                                rect.left_center() + vec2(8.0, -galley.size().y / 2.0),
                                galley,
                                text_color,
                            );
                            if response.clicked() {
                                clicked = Some(i);
                            }
                        }
                        let more = visible.len().saturating_sub(VISIBLE);
                        let hint = if more > 0 {
                            format!("{more} more · Up/Down select · Enter insert · Esc close")
                        } else {
                            "Up/Down select · Enter/Tab insert · Esc close".to_owned()
                        };
                        ui.add_space(2.0);
                        ui.label(RichText::new(hint).small().color(colors.tab_text_inactive));
                    });
            });
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_keep_paths_and_trim_punctuation() {
        let t = tokens("see /var/log/syslog, then https://example.com/a?b=1. done");
        assert_eq!(t, vec!["done", "1", "https://example.com/a?b", "then", "/var/log/syslog", "see"]);
        assert_eq!(tokens("pod/web-7d9f8c-x2v (Running)"), vec!["Running", "pod/web-7d9f8c-x2v"]);
    }

    #[test]
    fn prefix_is_word_before_cursor() {
        let line: Vec<char> = "user@host:~$ kubectl logs web-7".chars().collect();
        assert_eq!(prefix_before(&line, line.len()), "web-7");
        let line: Vec<char> = "$ ".chars().collect();
        assert_eq!(prefix_before(&line, 2), "");
    }

    #[test]
    fn ranking_is_nearest_first_and_smart_case() {
        let lines = ["deploy web-7d9f8c", "web-7a11 web-7d9f8c", "Web-Old"];
        assert_eq!(rank(lines, "web"), vec!["web-7d9f8c", "web-7a11", "Web-Old"]);
        assert_eq!(rank(lines, "Web"), vec!["Web-Old"]);
    }

    #[test]
    fn accept_sends_suffix_or_replaces_case() {
        let mut a = Autocomplete::new();
        a.prefix = "web".into();
        a.candidates = vec!["web-7d9f8c".into(), "WEB-UPPER".into()];
        a.open = true;
        assert!(a.type_text("-"));
        assert_eq!(a.accept(None), Some(b"-7d9f8c".to_vec()));

        a.prefix = "web".into();
        a.filter.clear();
        a.candidates = vec!["WEB-UPPER".into()];
        let mut expected = vec![0x7f; 3];
        expected.extend_from_slice(b"WEB-UPPER");
        assert_eq!(a.accept(None), Some(expected));
    }
}
