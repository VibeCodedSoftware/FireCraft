//! The start screen shown at launch: identity, real startup checks (run
//! on a worker thread so the window appears at once), the licence
//! statement, and "don't show this screen again" (kept in the user's
//! settings file).

use egui::{Color32, Rect, RichText, Sense, Stroke, pos2, vec2};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Ok,
    Warn,
    Fail,
}

struct Check {
    name: &'static str,
    level: Level,
    detail: String,
}

/// What the user picked on the start screen.
pub enum Choice {
    Continue,
}

const NAMES: [&str; 7] = [
    "Configuration",
    "Data folder",
    "Hardware",
    "Graphics",
    "Command engine",
    "Image codecs",
    "Licence notices",
];

const LOGO_PNG: &[u8] = include_bytes!("../../../packaging/windows/firecraft-512.png");
const NOTICE: &str = include_str!("../../../NOTICE");
const LICENSE_MIT: &str = include_str!("../../../LICENSE-MIT");

/// The user's FireCraft data folder (`%LOCALAPPDATA%\FireCraft` on
/// Windows, `~/.local/share/firecraft` elsewhere).
fn data_root() -> Option<PathBuf> {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return Some(PathBuf::from(local).join("FireCraft"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share/firecraft"))
}

fn settings_path() -> Option<PathBuf> {
    data_root().map(|root| root.join("settings.json"))
}

/// Whether the start screen should open (the user has not turned it off).
#[must_use]
pub fn wanted() -> bool {
    settings_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get("show_start_screen").and_then(serde_json::Value::as_bool))
        .unwrap_or(true)
}

fn remember(show: bool) {
    let Some(path) = settings_path() else {
        return;
    };
    let mut value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(object) = value.as_object_mut() {
        object.insert("show_start_screen".to_owned(), serde_json::Value::Bool(show));
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&value) {
        let _ = std::fs::write(path, text);
    }
}

/// The start screen's state.
pub struct StartScreen {
    rx: Receiver<Check>,
    checks: Vec<Check>,
    graphics_checked: bool,
    dont_show: bool,
    show_licences: bool,
    logo: Option<egui::TextureHandle>,
}

impl StartScreen {
    /// Starts the checks; `commands` is the engine's registered command
    /// count (read on the UI thread, where the session lives).
    #[must_use]
    pub fn new(commands: usize) -> Self {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let send = |name, level, detail: String| {
                let _ = tx.send(Check { name, level, detail });
            };
            let root = data_root();
            match settings_path() {
                Some(path) if path.exists() => {
                    let readable = std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                        .is_some();
                    if readable {
                        send("Configuration", Level::Ok, format!("Settings loaded from {}", tidy(&path)));
                    } else {
                        send("Configuration", Level::Warn, "Settings file unreadable — using defaults".into());
                    }
                }
                _ => send("Configuration", Level::Ok, "Default settings (first run)".into()),
            }
            match &root {
                Some(root) => {
                    let probe = root.join(".write-test");
                    let written = std::fs::create_dir_all(root)
                        .and_then(|()| std::fs::write(&probe, b"ok"))
                        .and_then(|()| std::fs::remove_file(&probe));
                    match written {
                        Ok(()) => send("Data folder", Level::Ok, format!("Writable · {}", tidy(root))),
                        Err(error) => send("Data folder", Level::Fail, format!("Not writable ({error}): {}", tidy(root))),
                    }
                }
                None => send("Data folder", Level::Warn, "No user data folder found".into()),
            }
            let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
            send(
                "Hardware",
                Level::Ok,
                format!("{threads} threads · {} · {}", std::env::consts::ARCH, std::env::consts::OS),
            );
            send(
                "Command engine",
                Level::Ok,
                format!("{commands} commands registered · Fireworks PNG (.fw.png) read and write"),
            );
            let started = Instant::now();
            match io::raster::decode(LOGO_PNG) {
                Ok(image) if image.width > 0 => send(
                    "Image codecs",
                    Level::Ok,
                    format!(
                        "PNG, JPEG, GIF, BMP, TIFF, WebP · decoder self-test {} ms",
                        started.elapsed().as_millis()
                    ),
                ),
                _ => send("Image codecs", Level::Fail, "The image decoder self-test failed".into()),
            }
            let beside = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join("NOTICE")))
                .is_some_and(|path| path.exists());
            if beside {
                send("Licence notices", Level::Ok, "NOTICE and licence files found next to the program".into());
            } else {
                send("Licence notices", Level::Warn, "NOTICE not next to the program · notices are built in".into());
            }
        });
        Self {
            rx,
            checks: Vec::new(),
            graphics_checked: false,
            dont_show: false,
            show_licences: false,
            logo: None,
        }
    }

    fn done(&self) -> bool {
        self.checks.len() >= NAMES.len()
    }

    /// Draws the screen; `Some` once the user moves on.
    pub fn ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, accent: Color32) -> Option<Choice> {
        let ctx = ui.ctx().clone();
        while let Ok(check) = self.rx.try_recv() {
            self.checks.push(check);
        }
        if !self.graphics_checked {
            self.graphics_checked = true;
            let display = ctx.input(|input| input.viewport().monitor_size);
            let scale = ctx.pixels_per_point();
            let screen = display.map_or_else(String::new, |size| {
                format!(" · display {:.0}×{:.0} at {:.0}%", size.x * scale, size.y * scale, scale * 100.0)
            });
            let (level, detail) = if frame.gl().is_some() {
                (Level::Ok, format!("OpenGL renderer active{screen}"))
            } else {
                (Level::Warn, format!("Renderer information unavailable{screen}"))
            };
            self.checks.push(Check {
                name: "Graphics",
                level,
                detail,
            });
        }
        if !self.done() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
        let logo = self
            .logo
            .get_or_insert_with(|| {
                let image = io::raster::decode(LOGO_PNG).map_or_else(
                    |_| egui::ColorImage::new([1, 1], vec![Color32::TRANSPARENT]),
                    |image| {
                        egui::ColorImage::from_rgba_unmultiplied(
                            [image.width as usize, image.height as usize],
                            &image.rgba,
                        )
                    },
                );
                ctx.load_texture("start-logo", image, egui::TextureOptions::LINEAR)
            })
            .clone();

        let mut choice = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(14, 14, 16)))
            .show(ui, |ui| {
                let avail = ui.max_rect();
                let size = vec2(980.0_f32.min(avail.width() - 24.0), 600.0_f32.min(avail.height() - 24.0));
                let card = Rect::from_center_size(avail.center(), size);
                let painter = ui.painter();
                painter.rect_filled(card.expand(1.0), 10.0, Color32::from_black_alpha(90));
                painter.rect_filled(card, 8.0, ui.visuals().panel_fill);
                let art = Rect::from_min_size(card.min, vec2((size.x * 0.38).round(), size.y));
                mosaic(&ui.painter().with_clip_rect(art), art);
                let content = Rect::from_min_max(pos2(art.right() + 34.0, card.top() + 30.0), card.max - vec2(30.0, 24.0));
                let mut cu = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(content)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                cu.horizontal(|ui| {
                    ui.add(egui::Image::new(&logo).fit_to_exact_size(vec2(68.0, 68.0)).corner_radius(14));
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new("FireCraft").size(30.0).strong());
                        let kind = if cfg!(debug_assertions) { "development build" } else { "release build" };
                        ui.label(
                            RichText::new(format!(
                                "Version {} · {kind} · web graphics and screen design",
                                env!("CARGO_PKG_VERSION")
                            ))
                            .weak(),
                        );
                    });
                });
                cu.add_space(18.0);
                egui::Grid::new("start_checks")
                    .num_columns(3)
                    .spacing([10.0, 9.0])
                    .show(&mut cu, |ui| {
                        for name in NAMES {
                            let check = self.checks.iter().find(|check| check.name == name);
                            match check {
                                Some(check) => status_icon(ui, check.level),
                                None => {
                                    ui.add(egui::Spinner::new().size(15.0));
                                }
                            }
                            ui.label(RichText::new(name).size(14.5));
                            let detail = check.map_or_else(|| "Checking…".to_owned(), |check| check.detail.clone());
                            let max = (content.right() - ui.cursor().left()).max(80.0);
                            ui.allocate_ui_with_layout(
                                vec2(max, 18.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.set_max_width(max);
                                    ui.add(egui::Label::new(RichText::new(detail).weak()).truncate());
                                },
                            );
                            ui.end_row();
                        }
                    });
                cu.add_space(16.0);
                cu.label(
                    RichText::new(
                        "FireCraft is a clean-room, open-source editor by the FireCraft contributors, released \
                         under MIT OR Apache-2.0. It uses third-party open-source libraries that remain subject \
                         to their own licences; their notices are listed under View licences and in the NOTICE \
                         file. FireCraft is not affiliated with Adobe; Fireworks is a trademark of Adobe.",
                    )
                    .small()
                    .weak(),
                );
                cu.add_space(6.0);
                cu.label(
                    RichText::new("Everything runs locally: nothing you open or make is uploaded.")
                        .small()
                        .weak(),
                );

                let bottom = Rect::from_min_max(
                    pos2(content.left(), card.bottom() - 92.0),
                    pos2(content.right(), card.bottom() - 18.0),
                );
                let mut bu = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(bottom)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                bu.horizontal(|ui| {
                    let failed = self.checks.iter().any(|check| check.level == Level::Fail);
                    let label = if failed { "Continue anyway  →" } else { "Continue to FireCraft  →" };
                    let start = egui::Button::new(RichText::new(label).size(14.5).color(Color32::WHITE))
                        .fill(accent)
                        .min_size(vec2(270.0, 36.0));
                    if ui.add(start).clicked() {
                        choice = Some(Choice::Continue);
                    }
                    let licences = egui::Button::new(RichText::new("View licences").size(14.5)).min_size(vec2(0.0, 36.0));
                    if ui.add(licences).clicked() {
                        self.show_licences = true;
                    }
                });
                bu.add_space(8.0);
                bu.checkbox(&mut self.dont_show, "Don't show this screen again");
            });

        if self.show_licences {
            let mut open = true;
            egui::Window::new("Licences")
                .open(&mut open)
                .default_size(vec2(620.0, 460.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(&ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(RichText::new(NOTICE).monospace().size(11.5));
                        ui.separator();
                        ui.label(RichText::new(LICENSE_MIT).monospace().size(11.5));
                    });
                });
            self.show_licences = open;
        } else if ctx.input(|input| input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Escape)) {
            choice = Some(Choice::Continue);
        }
        if choice.is_some() {
            remember(!self.dont_show);
        }
        choice
    }
}

/// A round status mark: a check, an exclamation or a cross.
fn status_icon(ui: &mut egui::Ui, level: Level) {
    let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
    let color = match level {
        Level::Ok => Color32::from_rgb(76, 194, 128),
        Level::Warn => Color32::from_rgb(230, 170, 60),
        Level::Fail => Color32::from_rgb(226, 82, 82),
    };
    let painter = ui.painter();
    let c = rect.center();
    painter.circle_stroke(c, 7.5, Stroke::new(1.6, color));
    let stroke = Stroke::new(1.8, color);
    match level {
        Level::Ok => {
            painter.line_segment([c + vec2(-3.5, 0.2), c + vec2(-1.0, 2.8)], stroke);
            painter.line_segment([c + vec2(-1.0, 2.8), c + vec2(3.8, -2.6)], stroke);
        }
        Level::Warn => {
            painter.line_segment([c + vec2(0.0, -3.8), c + vec2(0.0, 1.2)], stroke);
            painter.circle_filled(c + vec2(0.0, 3.6), 1.0, color);
        }
        Level::Fail => {
            painter.line_segment([c + vec2(-3.0, -3.0), c + vec2(3.0, 3.0)], stroke);
            painter.line_segment([c + vec2(3.0, -3.0), c + vec2(-3.0, 3.0)], stroke);
        }
    }
}

/// Shows the home folder as `~`, so screenshots don't reveal the account name.
fn tidy(path: &std::path::Path) -> String {
    let shown = path.display().to_string();
    match std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        Ok(home) if !home.is_empty() && shown.to_lowercase().starts_with(&home.to_lowercase()) => shown
            .get(home.len()..)
            .map_or_else(|| shown.clone(), |rest| format!("~{rest}")),
        _ => shown,
    }
}

/// The decorative mosaic in FireCraft's ember palette: deterministic
/// tiles with the odd vector glyph, fading toward the content side.
fn mosaic(painter: &egui::Painter, rect: Rect) {
    let cell = 36.0;
    let cols = (rect.width() / cell).ceil() as i32;
    let rows = (rect.height() / cell).ceil() as i32;
    let palette: [[u8; 3]; 6] = [
        [120, 34, 20],
        [170, 64, 22],
        [214, 104, 30],
        [92, 26, 46],
        [138, 40, 64],
        [60, 30, 40],
    ];
    let glyphs = ["◇", "○", "△", "□", "+", "·"];
    painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 14, 14));
    for y in 0..rows {
        for x in 0..cols {
            let mut h = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663);
            h ^= h >> 13;
            h = h.wrapping_mul(0x5bd1_e995);
            h ^= h >> 15;
            let base = palette[(h % 6) as usize];
            let fade = 1.0 - (x as f32 / cols as f32) * 0.45;
            let k = (0.45 + ((h >> 8) % 100) as f32 / 260.0) * fade;
            let color = Color32::from_rgb(
                (f32::from(base[0]) * k) as u8,
                (f32::from(base[1]) * k) as u8,
                (f32::from(base[2]) * k) as u8,
            );
            let tile = Rect::from_min_size(
                rect.min + vec2(x as f32 * cell, y as f32 * cell),
                vec2(cell - 2.0, cell - 2.0),
            );
            painter.rect_filled(tile, 2.0, color);
            if (h >> 20) % 11 == 0 {
                painter.text(
                    tile.center(),
                    egui::Align2::CENTER_CENTER,
                    glyphs[((h >> 3) % 6) as usize],
                    egui::FontId::proportional(14.0),
                    Color32::from_white_alpha(50),
                );
            }
        }
    }
}
