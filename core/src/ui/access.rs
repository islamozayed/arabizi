//! Accessibility: describes what's on screen for VoiceOver (and later the
//! Windows/Linux screen-reader APIs). The canvas is custom-drawn, so each
//! platform turns these nodes into native accessibility elements.

use super::geom::Rect;
use super::widget::*;
use crate::color::PRESET_NAMES;
use crate::session::Session;
use crate::settings::Theme as ThemePref;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxRole {
    Button,
    /// One of a group where one is chosen (theme, settings page, suggestion).
    Radio,
    /// On/off switch.
    Switch,
    /// A list item that can be picked (suggestion, starter, picker chip).
    Option,
    Slider,
    Text,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AxNode {
    pub rect: Rect,
    pub role: AxRole,
    pub label: String,
    /// Extra spoken detail ("selected", "1 of 8"), already folded into
    /// `label` where screen readers lack a slot for it.
    pub selected: bool,
    pub checked: Option<bool>,
    /// What pressing it does; `None` for plain text.
    pub target: Option<Target>,
}

impl Widget {
    /// Everything a screen reader should find, in reading order.
    pub fn ax_nodes(&self) -> Vec<AxNode> {
        let s = &self.session;
        let mut out = Vec::new();
        for (rect, text) in &self.ax_text {
            out.push(AxNode { rect: *rect, role: AxRole::Text, label: text.clone(), selected: false, checked: None, target: None });
        }
        for (rect, t) in &self.hits {
            let (role, label, selected, checked) = match *t {
                Target::Drag | Target::Block | Target::CpBackdrop | Target::CpPanel | Target::ChangelogPanel | Target::CmdgTip | Target::ShortcutsRow => continue,
                Target::SettingsBtn => (AxRole::Button, "Settings".to_string(), false, None),
                Target::Back => (AxRole::Button, "Back".into(), false, None),
                Target::HintExample => (AxRole::Button, format!("Try the example: {}", crate::session::HINT_EXAMPLE), false, None),
                Target::Starter(i) => {
                    let Some(st) = s.starters.get(i) else { continue };
                    let sel = s.selected_starter == Some(i);
                    (AxRole::Option, format!("{}, quick start {} of {}", Session::display(&st.ar), i + 1, s.starters.len()), sel, None)
                }
                Target::Suggestion(i) => {
                    let Some(c) = s.candidates.get(i) else { continue };
                    let sel = !s.emoji_mode && s.selected == i;
                    let lock = if i == 0 && s.show_lock() { ", locked" } else { "" };
                    (AxRole::Option, format!("{}, suggestion {} of {}{lock}", Session::display(c), i + 1, s.candidates.len()), sel, None)
                }
                Target::Emoji(i) => {
                    let Some(e) = s.emojis.get(i) else { continue };
                    (AxRole::Option, format!("{e}, emoji"), s.emoji_mode && s.selected_emoji == i, None)
                }
                Target::PreviewWord(i) => {
                    let Some(w) = s.committed.get(i) else { continue };
                    let editing = s.editing == Some(i);
                    let label = format!("{}, word {} of {}{}", Session::display(&w.arabic), i + 1, s.committed.len(), if editing { ", editing" } else { ", press to edit" });
                    (AxRole::Button, label, editing, None)
                }
                Target::PickerChip(i) => {
                    let Some(p) = &s.picker else { continue };
                    let Some(c) = p.chips.get(i) else { continue };
                    (AxRole::Option, format!("{}, option {} of {}", Session::display(&c.display), i + 1, p.chips.len()), p.alt_idx == i, None)
                }
                Target::Nav(p) => (AxRole::Radio, format!("{} settings", p.title()), self.page == p, None),
                Target::ThemeOption(th) => {
                    let name = match th {
                        ThemePref::Light => "Light",
                        ThemePref::Dark => "Dark",
                        ThemePref::System => "System",
                    };
                    (AxRole::Radio, format!("{name} theme"), self.settings.theme == th, None)
                }
                Target::AccentSystem => (AxRole::Radio, "System accent colour".into(), self.settings.accent.is_none(), None),
                Target::AccentPreset(i) => {
                    let active = self.settings.accent.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(crate::color::ACCENT_PRESETS[i]));
                    (AxRole::Radio, format!("{} accent colour", PRESET_NAMES[i]), active, None)
                }
                Target::AccentCustom => (AxRole::Button, "Custom accent colour".into(), false, None),
                Target::CompactToggle => (AxRole::Switch, "Compact mode".into(), false, Some(self.settings.compact)),
                Target::StartupToggle => (AxRole::Switch, "Run on startup".into(), false, Some(self.autostart)),
                Target::LargeText => (AxRole::Switch, "Larger text".into(), false, Some(self.settings.large_text)),
                Target::HighContrast => (AxRole::Switch, "High contrast".into(), false, Some(self.settings.high_contrast)),
                Target::Dyslexia => (AxRole::Switch, "Dyslexia font".into(), false, Some(self.settings.dyslexia_font)),
                Target::RecordShortcut => {
                    let label = if self.recording.is_some() {
                        "Recording launch shortcut, press the new keys or Escape to cancel".to_string()
                    } else {
                        format!("Change launch shortcut, currently {}", self.shortcut().replace('+', " "))
                    };
                    (AxRole::Button, label, false, None)
                }
                Target::CheckUpdates => (AxRole::Button, "Check for updates".into(), false, None),
                Target::ResetApp => {
                    let label = if self.reset_armed_until.is_some() { "Reset app, press again to confirm" } else { "Reset app" };
                    (AxRole::Button, label.into(), false, None)
                }
                Target::CpGradient => (AxRole::Slider, "Saturation and brightness".into(), false, None),
                Target::CpHue => (AxRole::Slider, "Hue".into(), false, None),
                Target::CpRecent(i) => {
                    let Some(c) = self.settings.recent_colors.get(i) else { continue };
                    (AxRole::Button, format!("Recent colour {c}"), false, None)
                }
                Target::ChangelogClose => (AxRole::Button, "Got it".into(), false, None),
                Target::BannerLater => (AxRole::Button, "Later".into(), false, None),
                Target::BannerAction => {
                    let l = match &self.update {
                        UpdateState::Installed { .. } => "Restart",
                        UpdateState::Failed => "Dismiss",
                        _ => "Install update",
                    };
                    (AxRole::Button, l.into(), false, None)
                }
                Target::CmdgTipDismiss | Target::ShortcutsTipDismiss => (AxRole::Button, "Don't show this tip again".into(), false, None),
                Target::ShortcutsTipMore => (AxRole::Button, "See all shortcuts".into(), false, None),
            };
            out.push(AxNode { rect: *rect, role, label, selected, checked, target: Some(*t) });
        }
        out
    }

    /// Activates a node (VoiceOver's press action).
    pub fn ax_press(&mut self, t: Target) {
        self.click(t, 1);
    }

    /// What arrow keys currently highlight, for spoken announcements (the
    /// web UI exposed this through `aria-activedescendant`).
    pub(super) fn ax_focus_text(&self) -> Option<String> {
        let s = &self.session;
        if let Some(p) = &s.picker {
            return p.chips.get(p.alt_idx).map(|c| Session::display(&c.display));
        }
        if s.emoji_mode {
            return s.emojis.get(s.selected_emoji).cloned();
        }
        if !s.candidates.is_empty() {
            return s.candidates.get(s.selected).map(|c| format!("{}, {} of {}", Session::display(c), s.selected + 1, s.candidates.len()));
        }
        s.selected_starter.and_then(|i| s.starters.get(i)).map(|st| Session::display(&st.ar))
    }
}

/// Spoken names for the Shortcuts settings rows, in drawing order.
pub fn shortcut_row_label(text: &str, keys: &[&str]) -> String {
    format!("{text}: {}", keys.join(" "))
}
