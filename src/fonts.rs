//! Loads the terminal font (and a bold face) through fontconfig's `fc-match`.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

pub const BOLD: &str = "rterm-bold";

/// Font bytes and face index for a fontconfig pattern or a font file path.
fn find(pattern: &str) -> Option<(Vec<u8>, u32)> {
    let path = Path::new(pattern);
    if path.is_file() {
        return std::fs::read(path).ok().map(|bytes| (bytes, 0));
    }
    let out = Command::new("fc-match").args(["-f", "%{file}\n%{index}", pattern]).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let mut lines = text.lines();
    let file = lines.next()?;
    let index = lines.next().and_then(|i| i.parse().ok()).unwrap_or(0);
    let lower = file.to_ascii_lowercase();
    if !(lower.ends_with(".ttf") || lower.ends_with(".otf") || lower.ends_with(".ttc")) {
        return None;
    }
    std::fs::read(file).ok().map(|bytes| (bytes, index))
}

fn data((bytes, index): (Vec<u8>, u32)) -> Arc<FontData> {
    let mut font = FontData::from_owned(bytes);
    font.index = index;
    Arc::new(font)
}

/// Installs the terminal font as the egui monospace family and a bold variant as
/// `FontFamily::Name(BOLD)`; the UI uses the desktop's sans-serif font. egui's bundled
/// fonts stay as fallbacks (emoji, symbols). Returns a warning if the font was not found.
pub fn install(ctx: &egui::Context, family: &str, use_bold: bool) -> Option<String> {
    let mut defs = FontDefinitions::default();
    let mut warning = None;
    let fallback_mono = defs.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();

    let regular = find(family).or_else(|| find(&format!("{family}:weight=regular")));
    if let Some(font) = regular {
        defs.font_data.insert("term".into(), data(font));
        defs.families.entry(FontFamily::Monospace).or_default().insert(0, "term".into());
    } else {
        warning = Some(format!("font {family:?} not found; using the built-in Hack font"));
    }

    let mut bold_list = fallback_mono.clone();
    if use_bold && let Some(font) = find(&format!("{family}:weight=bold")) {
        defs.font_data.insert("term-bold".into(), data(font));
        bold_list.insert(0, "term-bold".into());
    } else if defs.font_data.contains_key("term") {
        bold_list.insert(0, "term".into());
    }
    defs.families.insert(FontFamily::Name(BOLD.into()), bold_list);

    if let Some(font) = find("sans-serif") {
        defs.font_data.insert("ui".into(), data(font));
        defs.families.entry(FontFamily::Proportional).or_default().insert(0, "ui".into());
    }
    ctx.set_fonts(defs);
    warning
}
