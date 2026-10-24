//! Find in scrollback, like iTerm2's find bar: all matches highlighted, a current match,
//! a `k/N` counter, smart case and optional regex.

use std::time::{Duration, Instant};

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Direction, Point};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

use crate::layout::PaneId;
use crate::pane::Engine;

/// Upper bound on collected matches, so a pattern like `.` cannot stall the UI.
const MAX_MATCHES: usize = 10_000;
/// How often matches are refreshed while new output arrives.
const REFRESH: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct Find {
    pub open: bool,
    pub query: String,
    /// Off = smart case (case-insensitive unless the query has uppercase letters).
    pub case_sensitive: bool,
    pub regex: bool,
    pub matches: Vec<Match>,
    pub current: Option<usize>,
    /// The query is not a valid regular expression.
    pub invalid: bool,
    /// The search field should take keyboard focus on the next frame.
    pub focus: bool,
    computed: Option<(String, bool, bool, PaneId, u64)>,
    computed_at: Option<Instant>,
}

/// Escapes regex metacharacters for a literal search.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl Find {
    pub fn open(&mut self) {
        self.open = true;
        self.focus = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.matches.clear();
        self.current = None;
        self.computed = None;
    }

    fn pattern(&self) -> String {
        let body = if self.regex { self.query.clone() } else { escape(&self.query) };
        // RegexSearch is smart-case by itself; force case sensitivity when asked.
        if self.case_sensitive { format!("(?-i){body}") } else { body }
    }

    /// Recomputes matches for `pane` when the query, options or terminal content changed.
    /// Returns true when the current match changed and should be scrolled into view.
    pub fn update(&mut self, pane: PaneId, engine: &Engine) -> bool {
        let key = (self.query.clone(), self.case_sensitive, self.regex, pane, engine.generation);
        let Some(old) = &self.computed else { return self.recompute(key, engine, true) };
        if *old == key {
            return false;
        }
        let same_search = old.0 == key.0 && old.1 == key.1 && old.2 == key.2 && old.3 == key.3;
        if same_search {
            // Only new output: refresh at a limited rate and keep the current match.
            if self.computed_at.is_some_and(|t| t.elapsed() < REFRESH) {
                return false;
            }
            return self.recompute(key, engine, false);
        }
        self.recompute(key, engine, true)
    }

    fn recompute(&mut self, key: (String, bool, bool, PaneId, u64), engine: &Engine, new_search: bool) -> bool {
        let previous = self.current.and_then(|i| self.matches.get(i)).map(|m| *m.start());
        self.computed = Some(key);
        self.computed_at = Some(Instant::now());
        self.matches.clear();
        self.invalid = false;
        if self.query.is_empty() {
            self.current = None;
            return false;
        }
        let mut regex = match RegexSearch::new(&self.pattern()) {
            Ok(r) => r,
            Err(_) => {
                self.invalid = true;
                self.current = None;
                return false;
            }
        };
        let term = &engine.term;
        let start = Point::new(term.topmost_line(), Column(0));
        let end = Point::new(term.bottommost_line(), term.last_column());
        self.matches = RegexIter::new(start, end, Direction::Right, term, &mut regex).take(MAX_MATCHES).collect();
        if self.matches.is_empty() {
            self.current = None;
            return false;
        }
        if !new_search && let Some(prev) = previous {
            self.current = self.matches.iter().position(|m| *m.start() == prev).or(self.current);
            self.current = self.current.map(|i| i.min(self.matches.len() - 1));
            return false;
        }
        // Like iTerm2, start from the bottom of what is visible and search upwards.
        let bottom = term.screen_lines() as i32 - 1 - term.grid().display_offset() as i32;
        self.current = self.matches.iter().rposition(|m| m.start().line.0 <= bottom).or(Some(0));
        true
    }

    /// Moves to the previous (upwards) or next (downwards) match, wrapping around.
    pub fn step(&mut self, up: bool) -> bool {
        let n = self.matches.len();
        if n == 0 {
            return false;
        }
        self.current = Some(match self.current {
            Some(i) if up => (i + n - 1) % n,
            Some(i) => (i + 1) % n,
            None => n - 1,
        });
        true
    }

    pub fn current_match(&self) -> Option<&Match> {
        self.current.and_then(|i| self.matches.get(i))
    }

    /// Scrolls `engine` so the current match is visible (about a third from the top).
    pub fn reveal(&self, engine: &mut Engine) {
        let Some(m) = self.current_match() else { return };
        let term = &mut engine.term;
        let rows = term.screen_lines() as i32;
        let offset = term.grid().display_offset() as i32;
        let line = m.start().line.0;
        let visible = (-offset..rows - offset).contains(&line);
        if visible {
            return;
        }
        let history = term.grid().history_size() as i32;
        let target = (rows / 3 - line).clamp(0, history);
        term.scroll_display(Scroll::Delta(target - offset));
    }

    /// Counter text like iTerm2's: `5/188`, `0/0`, or `10000+` when capped.
    pub fn counter(&self) -> String {
        if self.invalid {
            return "invalid".to_owned();
        }
        if self.query.is_empty() {
            return String::new();
        }
        let total = self.matches.len();
        let plus = if total >= MAX_MATCHES { "+" } else { "" };
        match self.current {
            Some(i) => format!("{}/{total}{plus}", i + 1),
            None => format!("0/{total}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_queries_are_escaped() {
        assert_eq!(escape("a.b*(c)"), "a\\.b\\*\\(c\\)");
        let f = Find { query: "x+y".into(), case_sensitive: true, ..Default::default() };
        assert_eq!(f.pattern(), "(?-i)x\\+y");
        let f = Find { query: "x+y".into(), regex: true, ..Default::default() };
        assert_eq!(f.pattern(), "x+y");
    }

    #[test]
    fn step_wraps() {
        let p = |l: i32| Point::new(alacritty_terminal::index::Line(l), Column(0));
        let mut f =
            Find { matches: vec![p(0)..=p(0), p(1)..=p(1), p(2)..=p(2)], current: Some(0), ..Default::default() };
        f.step(true);
        assert_eq!(f.current, Some(2));
        f.step(false);
        assert_eq!(f.current, Some(0));
        assert_eq!(f.counter(), "");
        f.query = "x".into();
        assert_eq!(f.counter(), "1/3");
    }
}
