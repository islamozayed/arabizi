//! The widget controller: owns the session, settings and view state, and
//! reacts to platform events.

use super::anim::{Anim, Ease};
use super::geom::{Point, Rect};
use super::paint::Painter;
use super::theme::{Family, TextStyle, Theme};
use crate::color::{self, Rgb};
use crate::picker::Mode;
use crate::session::{self, Ctx, Effect, Field, Key, Mods, Session};
use crate::settings::{self, Settings, Theme as ThemePref};
use arabizi_engine::{TransliterationEngine, UserPreferences};
use std::path::PathBuf;

pub const WINDOW_W: f32 = 600.0;
pub const WINDOW_H: f32 = 440.0;
pub const WINDOW_H_COMPACT: f32 = WINDOW_H - 48.0;
pub const HEADER_H: f32 = 60.0;
pub const FOOTER_H: f32 = 48.0;
pub const CONTENT_BOTTOM: f32 = WINDOW_H - FOOTER_H;
/// Where the native text input sits in the header.
pub const INPUT_RECT: Rect = Rect::new(52.0, 16.0, WINDOW_W - 52.0 - 12.0 - 20.0 - 16.0, 28.0);

/// How long the Cmd+G tip stays away once used.
const TIP_RESURFACE_DAYS: i64 = 7;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Hide the widget.
    Hide,
    /// Hide, refocus the previous app and paste.
    Paste(String),
    /// Begin moving the window with the mouse.
    StartDrag,
    SetField { field: FieldId, text: String, select_all: bool },
    FocusField(FieldId),
    /// Resize the window to this height (compact mode).
    ResizeWindow(f32),
    /// Register a new launch shortcut; answer with `shortcut_result`.
    RegisterShortcut(String),
    SetAutostart(bool),
    CheckForUpdates { manual: bool },
    InstallUpdate,
    Relaunch,
    /// Delete learned words / settings / onboarding flag and restart.
    ResetAppData,
    /// Ask the screen reader to speak this.
    Announce(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldId {
    Input,
    Hex,
}

/// A native text field the platform keeps on top of the canvas.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    pub id: FieldId,
    pub rect: Rect,
    pub style: TextStyle,
    pub visible: bool,
    pub enabled: bool,
    pub caret: super::geom::Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    Pointer,
    Grab,
    Crosshair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Appearance,
    System,
    Accessibility,
    Shortcuts,
}

impl Page {
    pub const ALL: [Page; 4] = [Page::Appearance, Page::System, Page::Accessibility, Page::Shortcuts];
    pub fn title(self) -> &'static str {
        match self {
            Page::Appearance => "Appearance",
            Page::System => "System",
            Page::Accessibility => "Accessibility",
            Page::Shortcuts => "Shortcuts",
        }
    }
}

/// Everything the mouse can act on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// Empty panel area that moves the window.
    Drag,
    /// Swallows clicks without dragging (e.g. picker background).
    Block,
    SettingsBtn,
    HintExample,
    Back,
    Starter(usize),
    Suggestion(usize),
    Emoji(usize),
    PreviewWord(usize),
    PickerChip(usize),
    Nav(Page),
    ThemeOption(ThemePref),
    AccentSystem,
    AccentPreset(usize),
    AccentCustom,
    CompactToggle,
    StartupToggle,
    RecordShortcut,
    CheckUpdates,
    ResetApp,
    LargeText,
    HighContrast,
    Dyslexia,
    ShortcutsRow,
    CpBackdrop,
    CpPanel,
    CpGradient,
    CpHue,
    CpRecent(usize),
    ChangelogClose,
    ChangelogPanel,
    BannerLater,
    BannerAction,
    CmdgTip,
    CmdgTipDismiss,
    ShortcutsTipDismiss,
    ShortcutsTipMore,
}

impl Target {
    pub fn cursor(self) -> Cursor {
        match self {
            Target::Drag => Cursor::Grab,
            Target::Block | Target::CpPanel | Target::ChangelogPanel | Target::CmdgTip | Target::ShortcutsRow => Cursor::Arrow,
            Target::CpBackdrop => Cursor::Arrow,
            Target::CpGradient => Cursor::Crosshair,
            _ => Cursor::Pointer,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateState {
    Idle,
    Available { version: String },
    Downloading { version: String, progress: Option<f32> },
    Installed { version: String },
    Failed,
}

/// Events from the platform's updater.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateEvent {
    /// A check finished. `None` = up to date.
    Checked { manual: bool, available: Option<String> },
    CheckFailed { manual: bool },
    Progress(Option<f32>),
    Installed,
    InstallFailed,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Scroll {
    pub offset: f32,
    pub content: f32,
    pub viewport: Rect,
}

impl Scroll {
    pub fn max(&self) -> f32 {
        (self.content - self.viewport.h).max(0.0)
    }
    pub fn clamp(&mut self) {
        self.offset = self.offset.clamp(0.0, self.max());
    }
    pub fn fades(&self) -> (bool, bool) {
        (self.offset > 1.0, self.offset + self.viewport.h < self.content - 1.0)
    }
}

pub(super) struct ColorPicker {
    pub h: i32,
    pub s: i32,
    pub v: i32,
    pub dragging: Option<Target>,
    pub gradient: Rect,
    pub hue: Rect,
}

pub(super) struct Toast {
    pub html: Vec<ToastPart>,
    pub until: f64,
    pub anim: Anim,
}

#[derive(Debug, Clone)]
pub(super) enum ToastPart {
    Text(&'static str),
    Kbd(&'static str),
}

pub(super) struct Changelog {
    pub version_line: String,
    pub title: Option<String>,
    pub items: Vec<String>,
}

pub struct Widget {
    pub(super) engine: TransliterationEngine,
    pub(super) prefs: UserPreferences,
    pub(super) data_dir: PathBuf,
    pub settings: Settings,
    pub session: Session,
    pub(super) field: Field,
    pub(super) is_mac: bool,
    pub(super) system_dark: bool,
    pub(super) system_accent: String,
    pub(super) theme: Theme,
    pub version: String,

    // View state.
    pub(super) settings_open: bool,
    pub(super) page: Page,
    pub(super) hits: Vec<(Rect, Target)>,
    /// Static text for screen readers, collected while painting.
    pub(super) ax_text: Vec<(Rect, String)>,
    last_announced: Option<String>,
    pub(super) mouse: Option<Point>,
    pub(super) pressed: Option<Target>,
    pub(super) scroll_tips: Scroll,
    pub(super) scroll_preview: Scroll,
    pub(super) scroll_suggestions: Scroll,
    pub(super) scroll_settings: Scroll,
    pub(super) scroll_changelog: Scroll,
    pub(super) preview_stick_bottom: bool,
    pub(super) suggestion_rows: Vec<Rect>,
    pub(super) color_picker: Option<ColorPicker>,
    pub(super) hex_text: String,
    pub(super) toast: Option<Toast>,
    pub(super) changelog: Option<Changelog>,
    pub(super) update: UpdateState,
    pub(super) update_status: String,
    pub(super) checking_manual: bool,
    pub(super) dismissed_update: Option<String>,
    pub(super) banner: Anim,
    pub(super) reset_armed_until: Option<f64>,
    pub(super) resetting: bool,
    pub(super) autostart: bool,
    pub(super) recording: Option<Vec<&'static str>>,
    pub(super) recording_hint: Option<String>,
    pub(super) settings_reveal: Option<Anim>,
    pub(super) toggles: Vec<(Target, Anim)>,
    pub(super) cmdg_tip: Option<Rect>,
    pub(super) cmdg_anim: Anim,
    pub(super) cmdg_pending: Option<(f64, Rect)>,
    pub(super) cmdg_hide_at: Option<f64>,
    pub(super) shortcuts_tip: bool,
    pub(super) shortcuts_anim: Anim,
    pub(super) shortcuts_tip_at: Option<f64>,
    pub(super) shortcuts_suppress_until: f64,
    pub(super) last_check: f64,
    pub(super) now: f64,
    commands: Vec<Command>,
}

pub struct WidgetInit {
    pub engine: TransliterationEngine,
    pub data_dir: PathBuf,
    pub settings: Settings,
    pub prefs: UserPreferences,
    pub is_mac: bool,
    pub system_dark: bool,
    pub system_accent: String,
    pub autostart: bool,
    pub now: f64,
}

impl Widget {
    pub fn new(init: WidgetInit) -> Widget {
        let starters = init.settings.starters.clone().unwrap_or_else(session::default_starters);
        let mut w = Widget {
            engine: init.engine,
            prefs: init.prefs,
            data_dir: init.data_dir,
            settings: init.settings,
            session: Session::new(starters),
            field: Field::default(),
            is_mac: init.is_mac,
            system_dark: init.system_dark,
            system_accent: init.system_accent,
            theme: Theme::new(true, false, false, false, color::DEFAULT_ACCENT),
            version: crate::VERSION.to_string(),
            settings_open: false,
            page: Page::Appearance,
            hits: Vec::new(),
            ax_text: Vec::new(),
            last_announced: None,
            mouse: None,
            pressed: None,
            scroll_tips: Scroll::default(),
            scroll_preview: Scroll::default(),
            scroll_suggestions: Scroll::default(),
            scroll_settings: Scroll::default(),
            scroll_changelog: Scroll::default(),
            preview_stick_bottom: true,
            suggestion_rows: Vec::new(),
            color_picker: None,
            hex_text: String::new(),
            toast: None,
            changelog: None,
            update: UpdateState::Idle,
            update_status: String::new(),
            checking_manual: false,
            dismissed_update: None,
            banner: Anim::fixed(0.0),
            reset_armed_until: None,
            resetting: false,
            autostart: init.autostart,
            recording: None,
            recording_hint: None,
            settings_reveal: None,
            toggles: Vec::new(),
            cmdg_tip: None,
            cmdg_anim: Anim::fixed(0.0),
            cmdg_pending: None,
            cmdg_hide_at: None,
            shortcuts_tip: false,
            shortcuts_anim: Anim::fixed(0.0),
            shortcuts_tip_at: None,
            shortcuts_suppress_until: 0.0,
            last_check: init.now,
            now: init.now,
            commands: Vec::new(),
        };
        for t in [Target::CompactToggle, Target::StartupToggle, Target::LargeText, Target::HighContrast, Target::Dyslexia] {
            let on = w.toggle_on(t);
            w.toggles.push((t, Anim::fixed(if on { 1.0 } else { 0.0 })));
        }
        w.rebuild_theme();
        w
    }

    /// Commands for the platform to carry out, in order.
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }

    fn cmd(&mut self, c: Command) {
        self.commands.push(c);
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn is_dark(&self) -> bool {
        match self.settings.theme {
            ThemePref::Dark => true,
            ThemePref::Light => false,
            ThemePref::System => self.system_dark,
        }
    }

    pub fn accent_hex(&self) -> String {
        if let Some(cp) = &self.color_picker {
            return color::hsv_to_rgb(cp.h, cp.s, cp.v).to_hex();
        }
        self.settings.accent.clone().unwrap_or_else(|| self.system_accent.clone())
    }

    pub(super) fn rebuild_theme(&mut self) {
        let s = &self.settings;
        self.theme = Theme::new(self.is_dark(), s.high_contrast, s.large_text, s.dyslexia_font, &self.accent_hex());
    }

    pub fn window_height(&self) -> f32 {
        if self.settings.compact { WINDOW_H_COMPACT } else { WINDOW_H }
    }

    pub fn save_settings(&self) {
        self.settings.save(&self.data_dir);
    }

    // ── Platform → widget ─────────────────────────────────────────────────

    /// The launch shortcut / tray opened the widget: start fresh.
    pub fn shown(&mut self) {
        self.session.reset();
        self.settings_open = false;
        self.color_picker = None;
        self.field = Field::default();
        self.hide_cmdg_tip();
        self.cancel_recording();
        self.cmd(Command::SetField { field: FieldId::Input, text: String::new(), select_all: false });
        self.cmd(Command::FocusField(FieldId::Input));
    }

    pub fn system_appearance(&mut self, dark: bool, accent_hex: String) {
        self.system_dark = dark;
        self.system_accent = accent_hex;
        self.rebuild_theme();
    }

    pub fn set_autostart_state(&mut self, on: bool) {
        self.autostart = on;
        self.animate_toggle(Target::StartupToggle);
    }

    /// The platform's answer to `RegisterShortcut`.
    pub fn shortcut_result(&mut self, shortcut: &str, ok: bool) {
        if ok {
            self.settings.shortcut = Some(shortcut.to_string());
            self.save_settings();
        }
    }

    pub fn default_shortcut(&self) -> &'static str {
        if self.is_mac { "Cmd+Shift+A" } else { "Ctrl+Shift+A" }
    }

    pub fn shortcut(&self) -> String {
        self.settings.shortcut.clone().unwrap_or_else(|| self.default_shortcut().to_string())
    }

    pub(super) fn with_session<R>(&mut self, f: impl FnOnce(&mut Session, &Ctx) -> R) -> R {
        let ctx = Ctx {
            engine: &self.engine,
            prefs: &self.prefs,
            ctrl_backspace_hint_shown: self.settings.ctrl_backspace_hint_shown,
        };
        f(&mut self.session, &ctx)
    }

    pub(super) fn apply(&mut self, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::SetField { text, select_all } => {
                    let n = text.chars().count();
                    self.field = Field { text: text.clone(), sel: if select_all { (0, n) } else { (n, n) } };
                    self.cmd(Command::SetField { field: FieldId::Input, text, select_all });
                    if select_all {
                        self.selection_changed();
                    }
                }
                Effect::Paste(text) => self.cmd(Command::Paste(text)),
                Effect::Hide => self.cmd(Command::Hide),
                Effect::Record { input, arabic } => {
                    self.prefs.record(&input, &arabic);
                    crate::save_prefs(&self.data_dir, &self.prefs);
                }
                Effect::PickerOpened => self.mark_cmdg_used(),
            }
        }
        self.preview_stick_bottom = true;
    }

    /// The input field's text or selection changed.
    pub fn field_changed(&mut self, id: FieldId, text: &str, sel: (usize, usize)) {
        match id {
            FieldId::Hex => self.hex_changed(text),
            FieldId::Input => {
                let text_changed = text != self.field.text;
                self.field = Field { text: text.to_string(), sel };
                if text_changed {
                    if !text.is_empty() {
                        self.dismiss_tutorial();
                    }
                    self.hide_cmdg_tip();
                    let effects = self.with_session(|s, ctx| s.input_changed(ctx, text));
                    self.apply(effects);
                }
                self.selection_changed();
            }
        }
    }

    pub fn field_blurred(&mut self, id: FieldId) {
        match id {
            FieldId::Input => self.hide_shortcuts_tip(),
            FieldId::Hex => self.hex_commit(),
        }
    }

    /// A key pressed while the widget has focus. Returns true when handled
    /// (the platform must then swallow it).
    pub fn key(&mut self, key: Key, mods: Mods) -> bool {
        if self.recording.is_some() {
            return true;
        }
        if self.settings_open {
            if key == Key::Escape {
                if self.color_picker.is_some() {
                    self.close_color_picker();
                } else {
                    self.close_settings();
                }
                return true;
            }
            return false;
        }
        if self.changelog.is_some() && key == Key::Escape {
            self.changelog = None;
            return true;
        }
        let field = self.field.clone();
        let result = self.with_session(|s, ctx| s.key(ctx, key, mods, &field));
        if result.is_some() {
            let now = self.ax_focus_text();
            let moved = matches!(key, Key::Up | Key::Down | Key::Left | Key::Right | Key::Tab | Key::Command('g') | Key::Command('t'));
            if moved && now.is_some() && now != self.last_announced {
                self.cmd(Command::Announce(now.clone().unwrap()));
            }
            self.last_announced = now;
        }
        match result {
            Some(effects) => {
                if matches!(key, Key::Up | Key::Down) {
                    self.schedule_cmdg_for_selected();
                }
                self.apply(effects);
                true
            }
            None => {
                if key == Key::Backspace {
                    let show = self.with_session(|s, ctx| s.take_backspace_tip(ctx));
                    if show {
                        self.settings.ctrl_backspace_hint_shown = true;
                        self.save_settings();
                        self.show_toast(
                            vec![ToastPart::Text("Tip: "), ToastPart::Kbd("Ctrl"), ToastPart::Text("+"), ToastPart::Kbd("⌫"), ToastPart::Text(" deletes the entire word")],
                            3.0,
                        );
                    }
                }
                false
            }
        }
    }

    // ── Shortcut recording ───────────────────────────────────────────────

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Modifier keys changed while recording (`names` like "Cmd", "Shift").
    pub fn record_modifiers(&mut self, names: Vec<&'static str>) {
        if let Some(r) = &mut self.recording {
            *r = names;
            self.recording_hint = None;
        }
    }

    /// A key while recording: `code` is "A", "5", "F3", "Space" or "Escape".
    pub fn record_key(&mut self, code: &str, mods: Mods) {
        if self.recording.is_none() {
            return;
        }
        if code == "Escape" {
            self.cancel_recording();
            return;
        }
        let mut parts: Vec<&str> = Vec::new();
        if mods.ctrl {
            parts.push("Ctrl");
        }
        if mods.shift {
            parts.push("Shift");
        }
        if mods.alt {
            parts.push("Alt");
        }
        if mods.meta {
            parts.push(if self.is_mac { "Cmd" } else { "Meta" });
        }
        if parts.is_empty() {
            if let Some(r) = &mut self.recording {
                r.clear();
            }
            self.recording_hint = Some(format!(
                "Need {}",
                if self.is_mac { "Cmd, Ctrl, Shift, or Alt" } else { "Ctrl, Shift, Alt, or Meta" }
            ));
            return;
        }
        let valid = code.len() == 1 && code.chars().all(|c| c.is_ascii_alphanumeric())
            || code == "Space"
            || (code.starts_with('F') && code[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)));
        if !valid {
            return;
        }
        let combo = format!("{}+{}", parts.join("+"), code);
        self.recording = None;
        self.recording_hint = None;
        self.cmd(Command::RegisterShortcut(combo));
    }

    pub(super) fn cancel_recording(&mut self) {
        self.recording = None;
        self.recording_hint = None;
    }

    // ── Updates ──────────────────────────────────────────────────────────

    pub fn update_event(&mut self, e: UpdateEvent) {
        match e {
            UpdateEvent::Checked { manual, available } => {
                if manual {
                    self.checking_manual = false;
                }
                match available {
                    None => {
                        if manual {
                            self.update_status = "You're up to date".into();
                        }
                    }
                    Some(v) => {
                        if manual {
                            self.update_status.clear();
                        }
                        let installing = matches!(self.update, UpdateState::Downloading { .. } | UpdateState::Installed { .. });
                        if installing {
                            if manual {
                                self.update_status = "Update is installing…".into();
                            }
                        } else if manual || (self.banner.target() == 0.0 && Some(&v) != self.dismissed_update.as_ref()) {
                            self.update = UpdateState::Available { version: v };
                            self.banner.set(1.0, self.now, 0.25);
                        }
                    }
                }
            }
            UpdateEvent::CheckFailed { manual } => {
                if manual {
                    self.checking_manual = false;
                    self.update_status = "Couldn't reach update server".into();
                }
            }
            UpdateEvent::Progress(p) => {
                if let UpdateState::Downloading { progress, .. } = &mut self.update {
                    *progress = p;
                }
            }
            UpdateEvent::Installed => {
                if let UpdateState::Downloading { version, .. } = &self.update {
                    self.update = UpdateState::Installed { version: version.clone() };
                }
            }
            UpdateEvent::InstallFailed => self.update = UpdateState::Failed,
        }
    }

    /// Shows "What's new" if this version hasn't been seen. `changelog_json`
    /// is the bundled CHANGELOG.json.
    pub fn maybe_show_changelog(&mut self, changelog_json: &str, returning_user: bool) {
        let current = self.version.clone();
        let last = self.settings.last_seen_version.clone();
        self.settings.last_seen_version = Some(current.clone());
        self.save_settings();
        match last {
            None if !returning_user => return,
            Some(v) if v == current => return,
            _ => {}
        }
        let Ok(data) = serde_json::from_str::<serde_json::Value>(changelog_json) else { return };
        let Some(entry) = data
            .get("versions")
            .and_then(|v| v.as_array())
            .and_then(|a| a.iter().find(|e| e.get("version").and_then(|v| v.as_str()) == Some(current.as_str())))
        else {
            return;
        };
        let date = entry.get("date").and_then(|d| d.as_str());
        self.changelog = Some(Changelog {
            version_line: format!("v{current}") + &date.map(|d| format!(" · {d}")).unwrap_or_default(),
            title: entry.get("title").and_then(|t| t.as_str()).map(str::to_string),
            items: entry
                .get("items")
                .and_then(|i| i.as_array())
                .map(|a| a.iter().filter_map(|i| i.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
        });
    }

    // ── Timers & animation ────────────────────────────────────────────────

    /// Advances timers. Call on every frame / timer fire.
    pub fn tick(&mut self, now: f64) {
        self.now = now;
        if let Some(t) = &self.toast {
            if now >= t.until && t.anim.target() > 0.0 {
                let mut t = self.toast.take().unwrap();
                t.anim.set(0.0, now, 0.2);
                self.toast = Some(t);
            } else if t.anim.target() == 0.0 && !t.anim.running(now) {
                self.toast = None;
            }
        }
        if let Some(until) = self.reset_armed_until {
            if now >= until {
                self.reset_armed_until = None;
            }
        }
        if let Some((at, rect)) = self.cmdg_pending {
            if now >= at {
                self.cmdg_pending = None;
                self.show_cmdg_tip(rect);
            }
        }
        if let Some(at) = self.cmdg_hide_at {
            if now >= at {
                self.hide_cmdg_tip();
            }
        }
        if let Some(at) = self.shortcuts_tip_at {
            if now >= at {
                self.shortcuts_tip_at = None;
                let (s, e) = self.field.sel;
                if e > s && !self.settings.shortcuts_tip_dismissed && self.session.picker.is_none() {
                    self.shortcuts_tip = true;
                    self.shortcuts_anim.set(1.0, now, 0.12);
                }
            }
        }
        // Background update check every 6 hours.
        if now - self.last_check >= 6.0 * 3600.0 {
            self.last_check = now;
            self.cmd(Command::CheckForUpdates { manual: false });
        }
    }

    /// Whether the platform should keep redrawing every frame.
    pub fn animating(&self) -> bool {
        let n = self.now;
        self.banner.running(n)
            || self.cmdg_anim.running(n)
            || self.shortcuts_anim.running(n)
            || self.toast.as_ref().is_some_and(|t| t.anim.running(n))
            || self.settings_reveal.as_ref().is_some_and(|a| a.running(n))
            || self.toggles.iter().any(|(_, a)| a.running(n))
    }

    /// Seconds until the next timer, if any (so an idle widget can sleep).
    pub fn next_wakeup(&self) -> Option<f64> {
        let mut at: Vec<f64> = Vec::new();
        if let Some(t) = &self.toast {
            at.push(t.until);
        }
        at.extend(self.reset_armed_until);
        at.extend(self.cmdg_pending.map(|p| p.0));
        at.extend(self.cmdg_hide_at);
        at.extend(self.shortcuts_tip_at);
        at.push(self.last_check + 6.0 * 3600.0);
        at.into_iter().map(|t| (t - self.now).max(0.0)).reduce(f64::min)
    }

    pub(super) fn show_toast(&mut self, html: Vec<ToastPart>, secs: f64) {
        let mut anim = Anim::fixed(0.0);
        anim.set(1.0, self.now, 0.2);
        self.toast = Some(Toast { html, until: self.now + secs, anim });
    }

    // ── Tutorial ─────────────────────────────────────────────────────────

    fn dismiss_tutorial(&mut self) {
        if !self.settings.tutorial_seen {
            self.settings.tutorial_seen = true;
            self.save_settings();
            self.settings_reveal = Some(Anim::fixed(0.0).with_ease(Ease::OutExpo));
            if let Some(a) = &mut self.settings_reveal {
                a.start_from(0.0, 1.0, self.now, 0.35);
            }
        }
    }

    // ── Settings actions ─────────────────────────────────────────────────

    pub(super) fn toggle_on(&self, t: Target) -> bool {
        match t {
            Target::CompactToggle => self.settings.compact,
            Target::StartupToggle => self.autostart,
            Target::LargeText => self.settings.large_text,
            Target::HighContrast => self.settings.high_contrast,
            Target::Dyslexia => self.settings.dyslexia_font,
            _ => false,
        }
    }

    pub(super) fn toggle_value(&self, t: Target) -> f32 {
        self.toggles.iter().find(|(k, _)| *k == t).map(|(_, a)| a.value(self.now)).unwrap_or(0.0)
    }

    fn animate_toggle(&mut self, t: Target) {
        let on = self.toggle_on(t);
        let now = self.now;
        if let Some((_, a)) = self.toggles.iter_mut().find(|(k, _)| *k == t) {
            a.set(if on { 1.0 } else { 0.0 }, now, 0.15);
        }
    }

    pub fn open_settings(&mut self) {
        self.settings_open = true;
        self.switch_page(Page::Appearance);
        self.hide_cmdg_tip();
        self.hide_shortcuts_tip();
    }

    pub(super) fn close_settings(&mut self) {
        self.settings_open = false;
        self.color_picker = None;
        self.cancel_recording();
        self.rebuild_theme();
        self.cmd(Command::FocusField(FieldId::Input));
    }

    pub(super) fn switch_page(&mut self, p: Page) {
        self.page = p;
        self.scroll_settings.offset = 0.0;
        self.close_color_picker();
    }

    pub(super) fn set_theme(&mut self, t: ThemePref) {
        self.settings.theme = t;
        self.save_settings();
        self.rebuild_theme();
    }

    pub(super) fn set_accent(&mut self, hex: Option<String>) {
        self.settings.accent = hex;
        self.save_settings();
        self.close_color_picker();
        self.rebuild_theme();
    }

    pub(super) fn open_color_picker(&mut self) {
        let hex = self.settings.accent.clone().unwrap_or_else(|| self.accent_hex());
        let (h, s, v) = color::hex_to_hsv(Rgb::from_hex(&hex).unwrap_or(Rgb(0x58, 0xA1, 0xAC)));
        self.color_picker = Some(ColorPicker { h, s, v, dragging: None, gradient: Rect::default(), hue: Rect::default() });
        self.hex_text = color::hsv_to_rgb(h, s, v).to_hex();
        self.cmd(Command::SetField { field: FieldId::Hex, text: self.hex_text.clone(), select_all: false });
        self.rebuild_theme();
    }

    pub(super) fn close_color_picker(&mut self) {
        if self.color_picker.take().is_some() {
            self.rebuild_theme();
        }
    }

    pub(super) fn picker_changed(&mut self) {
        if let Some(cp) = &self.color_picker {
            self.hex_text = color::hsv_to_rgb(cp.h, cp.s, cp.v).to_hex();
            self.cmd(Command::SetField { field: FieldId::Hex, text: self.hex_text.clone(), select_all: false });
        }
        self.rebuild_theme();
    }

    pub(super) fn commit_picker_color(&mut self) {
        let Some(cp) = &self.color_picker else { return };
        let hex = color::hsv_to_rgb(cp.h, cp.s, cp.v).to_hex();
        self.settings.accent = Some(hex.clone());
        self.settings.recent_colors.retain(|c| *c != hex);
        self.settings.recent_colors.insert(0, hex);
        self.settings.recent_colors.truncate(6);
        self.save_settings();
    }

    fn hex_changed(&mut self, text: &str) {
        self.hex_text = text.to_string();
        let t = text.trim();
        if t.len() == 7 {
            if let (Some(rgb), Some(cp)) = (Rgb::from_hex(t), self.color_picker.as_mut()) {
                let (h, s, v) = color::hex_to_hsv(rgb);
                (cp.h, cp.s, cp.v) = (h, s, v);
                self.rebuild_theme();
            }
        }
    }

    fn hex_commit(&mut self) {
        let Some(cp) = &self.color_picker else { return };
        if Rgb::from_hex(self.hex_text.trim()).is_some() && self.hex_text.trim().len() == 7 {
            self.commit_picker_color();
        } else {
            self.hex_text = color::hsv_to_rgb(cp.h, cp.s, cp.v).to_hex();
            self.cmd(Command::SetField { field: FieldId::Hex, text: self.hex_text.clone(), select_all: false });
        }
    }

    pub(super) fn toggle_setting(&mut self, t: Target) {
        match t {
            Target::CompactToggle => {
                self.settings.compact = !self.settings.compact;
                let h = self.window_height();
                self.cmd(Command::ResizeWindow(h));
            }
            Target::StartupToggle => {
                let next = !self.autostart;
                self.cmd(Command::SetAutostart(next));
                return;
            }
            Target::LargeText => self.settings.large_text = !self.settings.large_text,
            Target::HighContrast => self.settings.high_contrast = !self.settings.high_contrast,
            Target::Dyslexia => self.settings.dyslexia_font = !self.settings.dyslexia_font,
            _ => return,
        }
        self.save_settings();
        self.rebuild_theme();
        self.animate_toggle(t);
    }

    pub(super) fn reset_clicked(&mut self) {
        if self.resetting {
            return;
        }
        if self.reset_armed_until.is_none() {
            self.reset_armed_until = Some(self.now + 4.0);
            return;
        }
        self.reset_armed_until = None;
        self.resetting = true;
        self.cmd(Command::ResetAppData);
    }

    pub(super) fn update_banner_action(&mut self, later: bool) {
        match self.update.clone() {
            UpdateState::Available { version } => {
                if later {
                    self.dismissed_update = Some(version);
                    self.banner.set(0.0, self.now, 0.25);
                } else {
                    self.update = UpdateState::Downloading { version, progress: Some(0.0) };
                    self.cmd(Command::InstallUpdate);
                }
            }
            UpdateState::Installed { .. } => {
                if later {
                    self.banner.set(0.0, self.now, 0.25);
                } else {
                    self.cmd(Command::Relaunch);
                }
            }
            UpdateState::Failed => self.banner.set(0.0, self.now, 0.25),
            _ => {}
        }
    }

    pub(super) fn check_updates_clicked(&mut self) {
        if self.checking_manual {
            return;
        }
        self.checking_manual = true;
        self.update_status = "Checking…".into();
        self.cmd(Command::CheckForUpdates { manual: true });
    }

    // ── Tips ─────────────────────────────────────────────────────────────

    pub(super) fn should_show_cmdg_tip(&self) -> bool {
        let s = &self.settings.cmdg_tip;
        if s.perm_dismissed {
            return false;
        }
        let Some(last) = s.last_shown else { return true };
        let age_days = (settings::now_unix() - last) / 86400;
        !(s.seen && age_days < TIP_RESURFACE_DAYS)
    }

    fn mark_cmdg_used(&mut self) {
        self.settings.cmdg_tip.seen = true;
        self.settings.cmdg_tip.last_shown = Some(settings::now_unix());
        self.save_settings();
        self.hide_cmdg_tip();
    }

    pub(super) fn show_cmdg_tip(&mut self, anchor: Rect) {
        if self.session.current_input.is_empty() || self.session.picker.is_some() || self.settings_open {
            return;
        }
        if self.engine.letter_slots(&self.session.current_input).is_empty() {
            return;
        }
        self.cmdg_tip = Some(anchor);
        self.cmdg_hide_at = None;
        self.cmdg_anim.set(1.0, self.now, 0.12);
        self.settings.cmdg_tip.last_shown = Some(settings::now_unix());
        self.save_settings();
    }

    pub(super) fn hide_cmdg_tip(&mut self) {
        self.cmdg_pending = None;
        self.cmdg_hide_at = None;
        if self.cmdg_tip.is_some() {
            self.cmdg_anim.set(0.0, self.now, 0.12);
        }
    }

    fn schedule_cmdg_for_selected(&mut self) {
        self.hide_cmdg_tip();
        if !self.should_show_cmdg_tip() {
            return;
        }
        if let Some(r) = self.suggestion_rows.get(self.session.selected).copied() {
            self.cmdg_pending = Some((self.now + 1.0, r));
        }
    }

    fn selection_changed(&mut self) {
        let (s, e) = self.field.sel;
        if self.settings.shortcuts_tip_dismissed || self.session.picker.is_some() || self.now < self.shortcuts_suppress_until || e <= s {
            self.hide_shortcuts_tip();
            return;
        }
        self.shortcuts_tip_at = Some(self.now + 0.25);
    }

    pub(super) fn hide_shortcuts_tip(&mut self) {
        self.shortcuts_tip_at = None;
        if self.shortcuts_tip {
            self.shortcuts_tip = false;
            self.shortcuts_anim.set(0.0, self.now, 0.12);
        }
    }

    // ── Mouse ────────────────────────────────────────────────────────────

    pub(super) fn hit(&self, p: Point) -> Option<Target> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, t)| *t)
    }

    pub fn cursor(&self) -> Cursor {
        if self.color_picker.as_ref().is_some_and(|c| c.dragging == Some(Target::CpGradient)) {
            return Cursor::Crosshair;
        }
        self.mouse.and_then(|p| self.hit(p)).map(Target::cursor).unwrap_or(Cursor::Arrow)
    }

    pub(super) fn hovered(&self, t: Target) -> bool {
        self.mouse.and_then(|p| self.hit(p)) == Some(t)
    }

    pub fn mouse_moved(&mut self, p: Point) {
        let before = self.mouse.and_then(|m| self.hit(m));
        self.mouse = Some(p);
        let now_t = self.hit(p);
        if before == now_t {
            return;
        }
        // Hovering a starter selects it (like mouseenter in the web UI).
        if let Some(Target::Starter(i)) = now_t {
            self.session.hover_starter(i);
        }
        // Hovering a suggestion for a second explains Cmd+G.
        match now_t {
            Some(Target::Suggestion(i)) => {
                if self.should_show_cmdg_tip() {
                    if let Some(r) = self.suggestion_rows.get(i).copied() {
                        self.cmdg_hide_at = None;
                        self.cmdg_pending = Some((self.now + 1.0, r));
                    }
                }
            }
            Some(Target::CmdgTip) | Some(Target::CmdgTipDismiss) => {
                self.cmdg_hide_at = None;
            }
            _ => {
                if matches!(before, Some(Target::Suggestion(_))) {
                    self.cmdg_pending = None;
                    if self.cmdg_tip.is_some() {
                        self.cmdg_hide_at = Some(self.now + 0.2);
                    }
                } else if matches!(before, Some(Target::CmdgTip) | Some(Target::CmdgTipDismiss)) {
                    self.hide_cmdg_tip();
                }
            }
        }
    }

    pub fn mouse_exited(&mut self) {
        self.mouse = None;
    }

    pub fn scroll(&mut self, p: Point, dy: f32) {
        if self.changelog.is_some() {
            self.scroll_changelog.offset += dy;
            self.scroll_changelog.clamp();
            return;
        }
        let targets = [
            (&mut self.scroll_settings, self.settings_open),
            (&mut self.scroll_preview, !self.settings_open),
            (&mut self.scroll_suggestions, !self.settings_open),
            (&mut self.scroll_tips, !self.settings_open),
        ];
        for (s, active) in targets {
            if active && s.viewport.contains(p) {
                s.offset += dy;
                s.clamp();
                break;
            }
        }
        if self.scroll_preview.viewport.contains(p) {
            self.preview_stick_bottom = self.scroll_preview.offset >= self.scroll_preview.max() - 1.0;
        }
    }

    pub fn mouse_down(&mut self, p: Point, clicks: u32) {
        self.mouse = Some(p);
        let Some(t) = self.hit(p) else { return };
        self.pressed = Some(t);
        match t {
            Target::Drag => self.cmd(Command::StartDrag),
            Target::CpGradient | Target::CpHue => {
                if let Some(cp) = &mut self.color_picker {
                    cp.dragging = Some(t);
                }
                self.drag_color(p);
            }
            Target::Suggestion(i) if clicks >= 2 => {
                let e = self.session.confirm_candidate(i);
                self.apply(e);
            }
            Target::Starter(i) if clicks >= 2 => {
                let e = self.session.paste_starter(i);
                self.apply(e);
            }
            _ => {}
        }
    }

    pub fn mouse_dragged(&mut self, p: Point) {
        self.mouse = Some(p);
        if self.color_picker.as_ref().is_some_and(|c| c.dragging.is_some()) {
            self.drag_color(p);
        }
    }

    fn drag_color(&mut self, p: Point) {
        let Some(cp) = &mut self.color_picker else { return };
        match cp.dragging {
            Some(Target::CpGradient) => {
                let r = cp.gradient;
                cp.s = (((p.x - r.x) / r.w).clamp(0.0, 1.0) * 100.0).round() as i32;
                cp.v = ((1.0 - ((p.y - r.y) / r.h).clamp(0.0, 1.0)) * 100.0).round() as i32;
            }
            Some(Target::CpHue) => {
                let r = cp.hue;
                cp.h = (((p.x - r.x) / r.w).clamp(0.0, 1.0) * 360.0).round() as i32;
            }
            _ => return,
        }
        self.picker_changed();
    }

    pub fn mouse_up(&mut self, p: Point, clicks: u32) {
        self.mouse = Some(p);
        if let Some(cp) = &mut self.color_picker {
            if cp.dragging.take().is_some() {
                self.commit_picker_color();
                self.pressed = None;
                return;
            }
        }
        let pressed = self.pressed.take();
        let Some(t) = self.hit(p) else { return };
        if pressed != Some(t) {
            return;
        }
        self.click(t, clicks);
    }

    pub(super) fn click(&mut self, t: Target, clicks: u32) {
        match t {
            Target::SettingsBtn => {
                if self.settings_open {
                    self.close_settings();
                } else {
                    self.open_settings();
                }
            }
            Target::Back => self.close_settings(),
            Target::HintExample => {
                let e = self.with_session(|s, ctx| s.load_example(ctx));
                self.dismiss_tutorial();
                self.apply(e);
                self.cmd(Command::FocusField(FieldId::Input));
            }
            Target::Starter(i) if clicks < 2 => self.session.hover_starter(i),
            Target::Suggestion(i) if clicks < 2 => {
                self.with_session(|s, ctx| s.pick_candidate(ctx, i));
            }
            Target::Emoji(i) => {
                let e = self.session.pick_emoji(i);
                self.apply(e);
            }
            Target::PreviewWord(i) => {
                let e = self.session.edit_word(i);
                self.apply(e);
                self.cmd(Command::FocusField(FieldId::Input));
            }
            Target::PickerChip(i) => {
                self.with_session(|s, ctx| s.commit_picker(ctx, Some(i)));
                self.cmd(Command::FocusField(FieldId::Input));
            }
            Target::Nav(p) => self.switch_page(p),
            Target::ThemeOption(t) => self.set_theme(t),
            Target::AccentSystem => self.set_accent(None),
            Target::AccentPreset(i) => self.set_accent(Some(color::ACCENT_PRESETS[i].to_string())),
            Target::AccentCustom => {
                if self.color_picker.is_some() {
                    self.close_color_picker();
                } else {
                    self.open_color_picker();
                }
            }
            Target::CpBackdrop => self.close_color_picker(),
            Target::CpRecent(i) => {
                if let Some(hex) = self.settings.recent_colors.get(i).cloned() {
                    if let (Some(rgb), Some(cp)) = (Rgb::from_hex(&hex), self.color_picker.as_mut()) {
                        (cp.h, cp.s, cp.v) = color::hex_to_hsv(rgb);
                    }
                    self.picker_changed();
                    self.commit_picker_color();
                }
            }
            Target::CompactToggle | Target::StartupToggle | Target::LargeText | Target::HighContrast | Target::Dyslexia => {
                self.toggle_setting(t)
            }
            Target::RecordShortcut => {
                if self.recording.is_some() {
                    self.cancel_recording();
                } else {
                    self.recording = Some(Vec::new());
                }
            }
            Target::CheckUpdates => self.check_updates_clicked(),
            Target::ResetApp => self.reset_clicked(),
            Target::ChangelogClose => self.changelog = None,
            Target::BannerLater => self.update_banner_action(true),
            Target::BannerAction => self.update_banner_action(false),
            Target::CmdgTipDismiss => {
                self.settings.cmdg_tip = settings::CmdgTipState { perm_dismissed: true, seen: true, last_shown: Some(settings::now_unix()) };
                self.save_settings();
                self.hide_cmdg_tip();
            }
            Target::ShortcutsTipDismiss => {
                self.settings.shortcuts_tip_dismissed = true;
                self.save_settings();
                self.hide_shortcuts_tip();
            }
            Target::ShortcutsTipMore => {
                self.shortcuts_suppress_until = self.now + 0.6;
                self.hide_shortcuts_tip();
                if !self.settings_open {
                    self.open_settings();
                }
                self.switch_page(Page::Shortcuts);
            }
            _ => {}
        }
    }

    /// Open the letter picker from the platform (e.g. a menu item).
    pub fn open_picker(&mut self, mode: Mode) {
        let field = self.field.clone();
        let e = self.with_session(|s, ctx| s.open_picker(ctx, mode, &field));
        self.apply(e);
    }

    // ── Fields ───────────────────────────────────────────────────────────

    pub fn fields(&self) -> Vec<FieldSpec> {
        let t = &self.theme;
        let mut out = vec![FieldSpec {
            id: FieldId::Input,
            rect: INPUT_RECT,
            style: TextStyle { family: Family::Latin, weight: 500, size: t.input_size, color: t.text_primary, tracking: 0.0 },
            visible: !self.settings_open,
            enabled: !self.settings_open,
            caret: t.accent,
        }];
        if self.color_picker.is_some() {
            out.push(FieldSpec {
                id: FieldId::Hex,
                rect: self.hex_rect(),
                style: TextStyle { family: Family::Mono, weight: 400, size: t.font_size_min, color: t.text_primary, tracking: 0.0 },
                visible: true,
                enabled: true,
                caret: t.accent,
            });
        }
        out
    }

    /// Paints the whole window and records hit regions.
    pub fn paint(&mut self, p: &mut dyn Painter, now: f64) {
        self.now = now;
        self.hits.clear();
        self.ax_text.clear();
        self.paint_main(p);
        self.paint_floating(p);
    }
}
