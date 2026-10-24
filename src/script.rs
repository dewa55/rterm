//! Scripted UI automation for development: `RTERM_SCRIPT` runs steps against the live window
//! (actions, typing, synthetic mouse input, screenshots) so behavior can be checked without a human.
//!
//! Steps are separated by `;;`, e.g.
//! `RTERM_SCRIPT='sleep 1;;type ls\n;;split_vertical;;click 300 200;;screenshot /tmp/a.png;;quit'`

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, ViewportCommand, ViewportId, pos2};

use crate::config::Action;

pub enum Step {
    Action(Action),
    Type(String),
    Sleep(f32),
    /// Synthetic mouse events at a window position.
    Mouse {
        pos: Pos2,
        button: PointerButton,
        pressed: bool,
        shift: bool,
    },
    Move(Pos2),
    Wheel(f32),
    /// Keyboard text input as egui sees typing (goes to focused widgets, unlike `type`).
    Text(String),
    /// Key press + release, e.g. `key ctrl+shift+n`.
    Key(crate::config::KeyCombo),
    /// Screenshot of the main window, or of the Settings window.
    Screenshot {
        path: PathBuf,
        settings: bool,
    },
    SettingsPage(String),
    Quit,
}

pub enum Effect {
    Action(Action),
    Type(String),
    SettingsPage(String),
    Quit,
}

pub struct Script {
    steps: VecDeque<Step>,
    next_at: Instant,
    pending_events: Vec<Event>,
    screenshot_done: Option<(Arc<AtomicBool>, Instant)>,
}

fn parse_pos(args: &[&str]) -> Option<Pos2> {
    Some(pos2(args.first()?.parse().ok()?, args.get(1)?.parse().ok()?))
}

impl Script {
    pub fn from_env() -> Option<Script> {
        let text = std::env::var("RTERM_SCRIPT").ok()?;
        let mut steps = VecDeque::new();
        for raw in text.split(";;") {
            let raw = raw.trim_start();
            let (cmd, rest) = raw.split_once(' ').unwrap_or((raw, ""));
            let args: Vec<&str> = rest.split_whitespace().collect();
            let button = |name: Option<&&str>| match name.copied() {
                Some("right") => PointerButton::Secondary,
                Some("middle") => PointerButton::Middle,
                _ => PointerButton::Primary,
            };
            let step = match cmd {
                "" => continue,
                "type" => Step::Type(rest.replace("\\n", "\r").replace("\\e", "\x1b")),
                "text" => Step::Text(rest.to_owned()),
                "sleep" => Step::Sleep(rest.trim().parse().unwrap_or(0.5)),
                "screenshot" => Step::Screenshot { path: PathBuf::from(rest.trim()), settings: false },
                "screenshot-settings" => Step::Screenshot { path: PathBuf::from(rest.trim()), settings: true },
                "settings-page" => Step::SettingsPage(rest.trim().to_owned()),
                "quit" => Step::Quit,
                "move" => Step::Move(parse_pos(&args)?),
                "wheel" => Step::Wheel(rest.trim().parse().unwrap_or(1.0)),
                "key" => match crate::config::KeyCombo::parse(rest.trim()) {
                    Ok(c) => Step::Key(c),
                    Err(e) => {
                        eprintln!("RTERM_SCRIPT: {e}");
                        continue;
                    }
                },
                "press" | "release" | "click" | "shift-click" => {
                    let pos = parse_pos(&args)?;
                    let b = button(args.get(2));
                    let shift = cmd == "shift-click";
                    if cmd == "click" || shift {
                        steps.push_back(Step::Move(pos));
                        steps.push_back(Step::Mouse { pos, button: b, pressed: true, shift });
                        steps.push_back(Step::Mouse { pos, button: b, pressed: false, shift });
                        continue;
                    }
                    Step::Mouse { pos, button: b, pressed: cmd == "press", shift: false }
                }
                action => match Action::parse(action) {
                    Ok(a) => Step::Action(a),
                    Err(e) => {
                        eprintln!("RTERM_SCRIPT: {e}");
                        continue;
                    }
                },
            };
            steps.push_back(step);
        }
        Some(Script {
            steps,
            next_at: Instant::now() + Duration::from_millis(800),
            pending_events: Vec::new(),
            screenshot_done: None,
        })
    }

    /// Synthetic input for this frame (fed through `App::raw_input_hook`).
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending_events)
    }

    /// Runs due steps; returns effects for the app to apply.
    pub fn tick(&mut self, ctx: &egui::Context) -> Vec<Effect> {
        let mut effects = Vec::new();
        ctx.request_repaint_after(Duration::from_millis(50));
        while Instant::now() >= self.next_at {
            if let Some((done, since)) = &self.screenshot_done {
                if !done.load(Ordering::Relaxed) {
                    if since.elapsed() < Duration::from_secs(5) {
                        break;
                    }
                    eprintln!("rterm: screenshot timed out");
                }
                self.screenshot_done = None;
            }
            let Some(step) = self.steps.pop_front() else { break };
            let mut delay = 0.15;
            match step {
                Step::Action(a) => effects.push(Effect::Action(a)),
                Step::Type(t) => effects.push(Effect::Type(t)),
                Step::Sleep(s) => delay = s,
                Step::Move(pos) => self.pending_events.push(Event::PointerMoved(pos)),
                Step::Mouse { pos, button, pressed, shift } => {
                    let modifiers = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
                    self.pending_events.push(Event::PointerButton { pos, button, pressed, modifiers });
                }
                Step::Text(t) => self.pending_events.push(Event::Text(t)),
                Step::Key(c) => {
                    let modifiers =
                        Modifiers { alt: c.alt, ctrl: c.ctrl, shift: c.shift, mac_cmd: false, command: c.ctrl };
                    for pressed in [true, false] {
                        self.pending_events.push(Event::Key {
                            key: c.key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers,
                        });
                    }
                }
                Step::Wheel(lines) => self.pending_events.push(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0.0, lines),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: Modifiers::NONE,
                }),
                Step::SettingsPage(page) => effects.push(Effect::SettingsPage(page)),
                Step::Screenshot { path, settings } => {
                    let done = Arc::new(AtomicBool::new(false));
                    self.screenshot_done = Some((done.clone(), Instant::now()));
                    let callback = egui::ScreenshotCallback::new(move |image| {
                        let [w, h] = image.size;
                        let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                        match image::save_buffer(&path, &bytes, w as u32, h as u32, image::ExtendedColorType::Rgba8) {
                            Ok(()) => eprintln!("rterm: screenshot saved to {}", path.display()),
                            Err(e) => eprintln!("rterm: screenshot failed: {e}"),
                        }
                        done.store(true, Ordering::Relaxed);
                    });
                    let viewport = if settings { ViewportId::from_hash_of("rterm-settings") } else { ViewportId::ROOT };
                    ctx.send_viewport_cmd_to(viewport, ViewportCommand::Screenshot(callback));
                }
                Step::Quit => effects.push(Effect::Quit),
            }
            self.next_at = Instant::now() + Duration::from_secs_f32(delay);
        }
        effects
    }
}
