//! The typing session behind the widget: the words committed so far, the word
//! being typed and its candidates, editing an earlier word, and the letter
//! picker. Platform UIs feed it text and key events and draw what it holds.
//!
//! Ported from the web UI's script; comments note where behaviour differs.

use crate::picker::{self, Chip, Mode, Overrides, Picker, Scope};
use crate::settings::Starter;
use crate::text::vocalize_allah;
use crate::translit::{transliterate, WordResult};
use arabizi_engine::{LetterSlot, TransliterationEngine, UserPreferences};

/// Starter phrases shown on the landing view (most common chat openers).
pub fn default_starters() -> Vec<Starter> {
    [
        ("سلام", "salam"),
        ("أهلاً", "ahlan"),
        ("مرحبًا", "marhaba"),
        ("صباح الخير", "sabah el kheir"),
        ("مساء الخير", "masa el kheir"),
        ("شكرًا", "shukran"),
        ("إن شاء الله", "inshallah"),
        ("ما شاء الله", "mashallah"),
        ("الحمد لله", "alhamdulillah"),
        ("يلا", "yalla"),
    ]
    .into_iter()
    .map(|(ar, input)| Starter { ar: ar.into(), input: Some(input.into()) })
    .collect()
}

pub const HINT_EXAMPLE: &str = "ahlan wa sahlan";

#[derive(Debug, Clone, PartialEq)]
pub struct CommittedWord {
    pub input: String,
    pub arabic: String,
    pub candidates: Vec<String>,
    pub emojis: Vec<String>,
    pub overrides: Overrides,
}

/// What the input field shows: its text and selection, in chars.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Field {
    pub text: String,
    pub sel: (usize, usize),
}

impl Field {
    pub fn new(text: &str) -> Field {
        let n = text.chars().count();
        Field { text: text.to_string(), sel: (n, n) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Space,
    Tab,
    Enter,
    Escape,
    Backspace,
    /// A letter pressed together with Cmd (macOS) or Ctrl.
    Command(char),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    /// Cmd on macOS, the Windows/Super key elsewhere.
    pub meta: bool,
    pub alt: bool,
}

/// Something the platform layer must do after an event.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Replace the field's text. `select_all` selects it (editing a word).
    SetField { text: String, select_all: bool },
    /// Hide the widget, return focus to the previous app and paste this text.
    Paste(String),
    /// Hide the widget.
    Hide,
    /// Remember that the user picked `arabic` for `input`.
    Record { input: String, arabic: String },
    /// The letter picker opened (counts as using the Cmd+G tip).
    PickerOpened,
}

pub struct Ctx<'a> {
    pub engine: &'a TransliterationEngine,
    pub prefs: &'a UserPreferences,
    pub ctrl_backspace_hint_shown: bool,
}

/// One row of the preview, right to left.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewWord {
    Committed { idx: usize, text: String },
    /// The committed word currently being edited, showing the live choice.
    Editing { idx: usize, text: String },
    /// The word being typed (not yet committed).
    Current { text: String },
}

#[derive(Default)]
pub struct Session {
    pub committed: Vec<CommittedWord>,
    pub candidates: Vec<String>,
    pub emojis: Vec<String>,
    pub current_input: String,
    pub selected: usize,
    pub emoji_mode: bool,
    pub selected_emoji: usize,
    pub editing: Option<usize>,
    pub editing_pin: Option<String>,
    pub letter_slots: Vec<LetterSlot>,
    pub overrides: Overrides,
    pub picker_history: Vec<Overrides>,
    pub picker: Option<Picker>,
    pub selected_starter: Option<usize>,
    pub starters: Vec<Starter>,
    backspace_count: u32,
}

impl Session {
    pub fn new(starters: Vec<Starter>) -> Session {
        Session { starters, ..Default::default() }
    }

    /// Clears everything, as when the widget is summoned.
    pub fn reset(&mut self) {
        let starters = std::mem::take(&mut self.starters);
        *self = Session::new(starters);
    }

    // ── Queries for the views ─────────────────────────────────────────────

    pub fn is_typing(&self) -> bool {
        !self.candidates.is_empty() || !self.committed.is_empty()
    }

    pub fn is_landing(&self, field: &Field) -> bool {
        self.current_input.is_empty() && field.text.trim().is_empty() && self.committed.is_empty()
    }

    /// The lock badge shows on the first suggestion while a pick is pinned.
    pub fn show_lock(&self) -> bool {
        !self.overrides.is_empty() || self.editing_pin.is_some()
    }

    /// Where the emoji row sits among the suggestions: after this many rows.
    pub fn emoji_row_after(&self) -> Option<usize> {
        if self.emojis.is_empty() {
            None
        } else {
            Some(self.candidates.len().min(2))
        }
    }

    /// Text shown for a candidate or emoji (الله gets its tashkeel).
    pub fn display(text: &str) -> String {
        vocalize_allah(text)
    }

    fn current_choice(&self) -> Option<String> {
        if self.emoji_mode && !self.emojis.is_empty() {
            self.emojis.get(self.selected_emoji).or(self.emojis.first()).cloned()
        } else {
            self.candidates.get(self.selected).or(self.candidates.first()).cloned()
        }
    }

    pub fn preview(&self) -> Vec<PreviewWord> {
        let mut out = Vec::new();
        for (i, w) in self.committed.iter().enumerate() {
            if self.editing == Some(i) && !self.candidates.is_empty() {
                let text = self.current_choice().unwrap_or_default();
                out.push(PreviewWord::Editing { idx: i, text: vocalize_allah(&text) });
            } else {
                out.push(PreviewWord::Committed { idx: i, text: vocalize_allah(&w.arabic) });
            }
        }
        if self.editing.is_none() && (!self.candidates.is_empty() || self.emoji_mode) {
            if let Some(text) = self.current_choice() {
                out.push(PreviewWord::Current { text: vocalize_allah(&text) });
            }
        }
        out
    }

    /// The text Enter pastes.
    pub fn output_text(&self) -> String {
        vocalize_allah(&self.committed.iter().map(|w| w.arabic.as_str()).collect::<Vec<_>>().join(" "))
    }

    // ── Typing ────────────────────────────────────────────────────────────

    /// The field's text changed (typing, paste, cut).
    pub fn input_changed(&mut self, ctx: &Ctx, raw: &str) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.current_input = raw.trim().to_string();
        if self.current_input.is_empty() {
            self.candidates.clear();
            self.emojis.clear();
            // Emptying the input ends the lock: it was tied to the old spelling.
            self.editing_pin = None;
            self.reset_selection();
            return effects;
        }
        self.reconcile_overrides(ctx);
        if self.picker.is_some() {
            self.close_picker();
        }
        let mut words = self.run(ctx);
        if words.len() > 1 {
            let done = words.len() - 1;
            let finished: Vec<CommittedWord> = words
                .drain(..done)
                .map(|w| CommittedWord {
                    input: w.input,
                    arabic: w.candidates.first().cloned().unwrap_or_default(),
                    candidates: w.candidates,
                    emojis: w.emojis,
                    overrides: Overrides::new(),
                })
                .collect();
            let n = finished.len();
            match self.editing {
                // Editing mid-sentence: finished words go in front of the
                // edited one, which the last word keeps replacing.
                Some(e) => {
                    self.committed.splice(e..e, finished);
                    self.editing = Some(e + n);
                }
                None => self.committed.extend(finished),
            }
            // Drop the committed words from the field.
            let rest = drop_words(raw, done);
            effects.push(Effect::SetField { text: rest, select_all: false });
        }
        match words.pop() {
            Some(last) => {
                self.candidates = last.candidates;
                self.emojis = last.emojis;
                self.current_input = last.input;
            }
            None => {
                self.candidates.clear();
                self.emojis.clear();
            }
        }
        self.apply_editing_pin();
        self.reset_selection();
        effects
    }

    fn run(&self, ctx: &Ctx) -> Vec<WordResult> {
        let ov = picker::to_engine(&self.overrides);
        transliterate(ctx.engine, ctx.prefs, &self.current_input, Some(&ov))
    }

    fn reset_selection(&mut self) {
        self.selected = 0;
        self.emoji_mode = false;
        self.selected_emoji = 0;
    }

    /// Re-runs the engine for the current word (after a picker change).
    pub fn refresh(&mut self, ctx: &Ctx) {
        if self.current_input.is_empty() {
            self.candidates.clear();
            self.emojis.clear();
            return;
        }
        if let Some(last) = self.run(ctx).pop() {
            self.candidates = last.candidates;
            self.emojis = last.emojis;
        }
        self.apply_editing_pin();
        self.reset_selection();
    }

    /// The example in the first-run hint was clicked.
    pub fn load_example(&mut self, ctx: &Ctx) -> Vec<Effect> {
        let mut effects = vec![Effect::SetField { text: HINT_EXAMPLE.into(), select_all: false }];
        effects.extend(self.input_changed(ctx, HINT_EXAMPLE));
        effects
    }

    fn apply_editing_pin(&mut self) {
        let Some(pin) = self.editing_pin.clone() else { return };
        match self.candidates.iter().position(|c| *c == pin) {
            Some(0) => {}
            Some(i) => {
                self.candidates.remove(i);
                self.candidates.insert(0, pin);
            }
            None => {
                self.candidates.insert(0, pin);
                self.candidates.truncate(8);
            }
        }
    }

    /// Pins the highlighted suggestion so later spelling tweaks keep it on
    /// top, and turns its letters into picker overrides where they align.
    fn pin_selected(&mut self, ctx: &Ctx) {
        if self.emoji_mode {
            return;
        }
        let Some(candidate) = self.candidates.get(self.selected).cloned() else { return };
        let slots = ctx.engine.letter_slots(&self.current_input);
        if let Some(derived) = picker::derive_overrides_from_candidate(&slots, &candidate) {
            self.overrides = derived;
            self.letter_slots = slots;
        }
        self.editing_pin = Some(candidate);
    }

    fn reconcile_overrides(&mut self, ctx: &Ctx) {
        if self.overrides.is_empty() || self.current_input.is_empty() {
            self.clear_overrides();
            return;
        }
        self.letter_slots = ctx.engine.letter_slots(&self.current_input);
        self.overrides = picker::reconcile(&self.letter_slots, &self.overrides);
    }

    fn clear_overrides(&mut self) {
        self.overrides.clear();
        self.letter_slots.clear();
        self.picker_history.clear();
    }

    fn clear_current(&mut self) {
        self.current_input.clear();
        self.candidates.clear();
        self.emojis.clear();
        self.reset_selection();
    }

    /// Commits the highlighted suggestion (Space, double-click, emoji click).
    pub fn confirm_current_word(&mut self) -> Vec<Effect> {
        let arabic = self.current_choice().unwrap_or_else(|| self.current_input.clone());
        let effects = vec![
            Effect::Record { input: self.current_input.clone(), arabic: arabic.clone() },
            Effect::SetField { text: String::new(), select_all: false },
        ];
        // Snapshot the picks so re-editing this word restores them.
        let word = CommittedWord {
            input: self.current_input.clone(),
            arabic,
            candidates: self.candidates.clone(),
            emojis: self.emojis.clone(),
            overrides: self.overrides.clone(),
        };
        match self.editing.take() {
            Some(i) if i < self.committed.len() => self.committed[i] = word,
            _ => self.committed.push(word),
        }
        self.clear_current();
        self.editing_pin = None;
        self.clear_overrides();
        self.close_picker();
        effects
    }

    /// Starts editing committed word `idx` (clicked in the preview, or
    /// Backspace at an empty field).
    ///
    /// Unlike the web UI, a word that is half-typed is committed first rather
    /// than thrown away, so you can jump back to fix an earlier word and keep
    /// everything you've written.
    pub fn edit_word(&mut self, idx: usize) -> Vec<Effect> {
        let mut effects = Vec::new();
        if idx >= self.committed.len() || self.editing == Some(idx) {
            return effects;
        }
        if !self.current_input.is_empty() && (!self.candidates.is_empty() || self.emoji_mode) {
            effects.extend(self.confirm_current_word());
        } else if !self.current_input.is_empty() {
            // Nothing to pick from (shouldn't happen): keep the Latin text.
            let raw = self.current_input.clone();
            self.commit_latin(&raw);
        }
        self.editing = None;
        let w = self.committed[idx].clone();
        self.editing = Some(idx);
        self.current_input = w.input.clone();
        self.candidates = w.candidates.clone();
        self.emojis = w.emojis.clone();
        // Highlighting a word locks its current spelling on top.
        self.editing_pin = Some(w.arabic.clone());
        self.apply_editing_pin();
        self.reset_selection();
        self.overrides = w.overrides.clone();
        self.letter_slots.clear();
        self.picker_history.clear();
        self.close_picker();
        effects.retain(|e| !matches!(e, Effect::SetField { .. }));
        effects.push(Effect::SetField { text: w.input, select_all: true });
        effects
    }

    fn commit_latin(&mut self, raw: &str) {
        let word = CommittedWord {
            input: raw.into(),
            arabic: raw.into(),
            candidates: vec![raw.into()],
            emojis: vec![],
            overrides: Overrides::new(),
        };
        match self.editing.take() {
            Some(i) if i < self.committed.len() => self.committed[i] = word,
            _ => self.committed.push(word),
        }
        self.editing_pin = None;
        self.clear_current();
    }

    /// Single click on a suggestion selects it and pins it.
    pub fn pick_candidate(&mut self, ctx: &Ctx, idx: usize) {
        self.selected = idx;
        self.emoji_mode = false;
        self.pin_selected(ctx);
    }

    /// Double click on a suggestion commits it.
    pub fn confirm_candidate(&mut self, idx: usize) -> Vec<Effect> {
        self.selected = idx;
        self.emoji_mode = false;
        self.confirm_current_word()
    }

    pub fn pick_emoji(&mut self, idx: usize) -> Vec<Effect> {
        self.emoji_mode = true;
        self.selected_emoji = idx;
        self.confirm_current_word()
    }

    // ── Starters (landing view) ───────────────────────────────────────────

    pub fn hover_starter(&mut self, idx: usize) {
        self.selected_starter = Some(idx);
    }

    pub fn paste_starter(&self, idx: usize) -> Vec<Effect> {
        match self.starters.get(idx) {
            Some(s) => vec![Effect::Paste(vocalize_allah(&s.ar))],
            None => vec![],
        }
    }

    pub fn seed_starter(&mut self, idx: usize) -> Vec<Effect> {
        let Some(s) = self.starters.get(idx).cloned() else { return vec![] };
        self.committed.push(CommittedWord {
            input: s.input.clone().unwrap_or_else(|| s.ar.clone()),
            arabic: s.ar.clone(),
            candidates: vec![s.ar],
            emojis: vec![],
            overrides: Overrides::new(),
        });
        self.selected_starter = None;
        self.clear_current();
        vec![Effect::SetField { text: String::new(), select_all: false }]
    }

    // ── Picker ────────────────────────────────────────────────────────────

    pub fn open_picker(&mut self, ctx: &Ctx, mode: Mode, field: &Field) -> Vec<Effect> {
        if self.current_input.is_empty() {
            return vec![];
        }
        let slots = ctx.engine.letter_slots(&self.current_input);
        if slots.is_empty() {
            return vec![];
        }
        self.letter_slots = slots;
        let mut scope = Scope::Letter;
        if mode == Mode::Tashkeel {
            let leading = field.text.chars().take_while(|c| c.is_whitespace()).count();
            let start = field.sel.0.saturating_sub(leading);
            let end = field.sel.1.saturating_sub(leading);
            if start == end && start >= self.current_input.chars().count() {
                scope = Scope::Word;
            }
        }
        let mut slot_indices = match scope {
            Scope::Word => (0..self.letter_slots.len()).collect(),
            Scope::Letter => picker::slot_indices_from_selection(&self.letter_slots, &field.text, field.sel),
        };
        let chips = self.build_chips(ctx, mode, scope, &mut slot_indices);
        let alt_idx = picker::focused_chip(&chips, &self.letter_slots, &self.overrides);
        self.picker = Some(Picker { mode, scope, slot_indices, chips, alt_idx });
        vec![Effect::PickerOpened]
    }

    fn build_chips(&self, ctx: &Ctx, mode: Mode, scope: Scope, slot_indices: &mut Vec<usize>) -> Vec<Chip> {
        picker::build_chips(
            mode,
            scope,
            slot_indices,
            &self.letter_slots,
            &self.overrides,
            self.candidates.first().map(|s| s.as_str()),
            |a| ctx.engine.lookup_vocalized(a.trim()),
        )
    }

    pub fn close_picker(&mut self) {
        self.picker = None;
    }

    /// Applies the chip at `idx` (click) or the focused chip.
    pub fn commit_picker(&mut self, ctx: &Ctx, idx: Option<usize>) {
        let Some(p) = self.picker.take() else { return };
        let Some(chip) = p.chips.get(idx.unwrap_or(p.alt_idx)) else { return };
        self.picker_history.push(self.overrides.clone());
        for (pos, val) in &chip.picks {
            self.overrides.insert(*pos, val.clone());
        }
        self.refresh(ctx);
    }

    fn picker_key(&mut self, ctx: &Ctx, key: Key, mods: Mods) -> Option<Vec<Effect>> {
        let p = self.picker.as_mut()?;
        let total = p.chips.len();
        match key {
            Key::Left | Key::Right => {
                if total > 0 {
                    // The picker is right-to-left: Right moves to a lower index.
                    let dir: isize = if key == Key::Right { -1 } else { 1 };
                    p.alt_idx = ((p.alt_idx as isize + dir).rem_euclid(total as isize)) as usize;
                }
                Some(vec![])
            }
            Key::Tab => {
                if p.slot_indices.len() == 1 && self.letter_slots.len() > 1 {
                    let n = self.letter_slots.len() as isize;
                    let dir = if mods.shift { -1 } else { 1 };
                    let idx = ((p.slot_indices[0] as isize + dir).rem_euclid(n)) as usize;
                    let mode = p.mode;
                    let mut indices = vec![idx];
                    let chips = self.build_chips(ctx, mode, Scope::Letter, &mut indices);
                    let alt_idx = picker::focused_chip(&chips, &self.letter_slots, &self.overrides);
                    self.picker = Some(Picker { mode, scope: Scope::Letter, slot_indices: indices, chips, alt_idx });
                }
                Some(vec![])
            }
            Key::Enter | Key::Space => {
                if total > 0 {
                    self.commit_picker(ctx, None);
                } else {
                    self.close_picker();
                }
                Some(vec![])
            }
            Key::Escape => {
                self.close_picker();
                Some(vec![])
            }
            _ => None,
        }
    }

    // ── Keys ──────────────────────────────────────────────────────────────

    /// Handles a key pressed in the input field. Returns `None` when the
    /// field should process the key itself (typing, caret moves).
    pub fn key(&mut self, ctx: &Ctx, key: Key, mods: Mods, field: &Field) -> Option<Vec<Effect>> {
        let cmd = mods.meta || mods.ctrl;

        match key {
            Key::Command('g') if cmd => return Some(self.open_picker(ctx, Mode::Letter, field)),
            Key::Command('t') if cmd => return Some(self.open_picker(ctx, Mode::Tashkeel, field)),
            _ => {}
        }

        if self.is_landing(field) && !self.starters.is_empty() && self.picker.is_none() {
            let n = self.starters.len();
            let cols = 2;
            match key {
                Key::Left | Key::Right | Key::Up | Key::Down => {
                    self.selected_starter = Some(match (self.selected_starter, key) {
                        (None, _) => 0,
                        // The grid is right-to-left.
                        (Some(i), Key::Right) => i.saturating_sub(1),
                        (Some(i), Key::Left) => (i + 1).min(n - 1),
                        (Some(i), Key::Down) => (i + cols).min(n - 1),
                        (Some(i), _) => i.saturating_sub(cols),
                    });
                    return Some(vec![]);
                }
                Key::Enter => return Some(self.paste_starter(self.selected_starter.unwrap_or(0))),
                Key::Tab => return Some(self.seed_starter(self.selected_starter.unwrap_or(0))),
                _ => {}
            }
        }

        if key == Key::Command('z') && cmd && !mods.shift {
            if let Some(prev) = self.picker_history.pop() {
                self.overrides = prev;
                self.close_picker();
                self.refresh(ctx);
                return Some(vec![]);
            }
            return None;
        }

        if self.picker.is_some() {
            if let Some(e) = self.picker_key(ctx, key, mods) {
                return Some(e);
            }
        }

        match key {
            Key::Escape => {
                if self.editing.is_some() {
                    self.editing = None;
                    self.editing_pin = None;
                    self.clear_current();
                    return Some(vec![Effect::SetField { text: String::new(), select_all: false }]);
                }
                Some(vec![Effect::Hide])
            }
            Key::Down => {
                if !self.candidates.is_empty() || self.emoji_mode {
                    let emoji_pos = self.emoji_pos();
                    if self.emoji_mode {
                        if let Some(p) = emoji_pos {
                            if self.candidates.len() > p + 1 {
                                self.emoji_mode = false;
                                self.selected = p + 1;
                            }
                        }
                    } else if Some(self.selected) == emoji_pos {
                        self.emoji_mode = true;
                        self.selected_emoji = 0;
                    } else if self.selected + 1 < self.candidates.len() {
                        self.selected += 1;
                    }
                    self.pin_selected(ctx);
                }
                Some(vec![])
            }
            Key::Up => {
                if self.emoji_mode {
                    self.emoji_mode = false;
                } else if !self.candidates.is_empty() {
                    let emoji_pos = self.emoji_pos();
                    if emoji_pos.is_some_and(|p| self.selected == p + 1) {
                        self.emoji_mode = true;
                        self.selected_emoji = 0;
                        self.selected = emoji_pos.unwrap();
                    } else if self.selected > 0 {
                        self.selected -= 1;
                    }
                    self.pin_selected(ctx);
                }
                Some(vec![])
            }
            Key::Left if self.emoji_mode => {
                self.selected_emoji = self.selected_emoji.saturating_sub(1);
                Some(vec![])
            }
            Key::Right if self.emoji_mode => {
                self.selected_emoji = (self.selected_emoji + 1).min(self.emojis.len().saturating_sub(1));
                Some(vec![])
            }
            Key::Space if (!self.candidates.is_empty() || self.emoji_mode) && !self.current_input.is_empty() => {
                Some(self.confirm_current_word())
            }
            Key::Tab if !self.current_input.is_empty() => {
                let raw = self.current_input.clone();
                self.commit_latin(&raw);
                Some(vec![Effect::SetField { text: String::new(), select_all: false }])
            }
            Key::Tab => Some(vec![]),
            Key::Enter => {
                let mut effects = Vec::new();
                if !self.candidates.is_empty() && !self.current_input.is_empty() {
                    effects.extend(self.confirm_current_word());
                }
                if !self.committed.is_empty() {
                    effects.push(Effect::Paste(self.output_text()));
                }
                Some(effects)
            }
            Key::Backspace if cmd && !self.committed.is_empty() => {
                match self.editing.take() {
                    Some(i) if i < self.committed.len() => {
                        self.committed.remove(i);
                    }
                    _ => {
                        self.committed.pop();
                    }
                }
                self.editing_pin = None;
                self.clear_current();
                self.backspace_count = 0;
                Some(vec![Effect::SetField { text: String::new(), select_all: false }])
            }
            Key::Backspace if field.text.is_empty() && !self.committed.is_empty() => {
                if let Some(i) = self.editing.take() {
                    if i < self.committed.len() {
                        self.committed.remove(i);
                    }
                    self.editing_pin = None;
                    self.clear_current();
                    if self.committed.is_empty() {
                        return Some(vec![]);
                    }
                }
                let last = self.committed.len() - 1;
                Some(self.edit_word(last))
            }
            Key::Backspace => {
                // The field deletes the character; `take_backspace_tip` then
                // says whether to show the Ctrl+⌫ tip.
                if !field.text.is_empty() {
                    self.backspace_count += 1;
                }
                None
            }
            _ => {
                self.backspace_count = 0;
                None
            }
        }
    }

    /// Whether a plain Backspace in a non-empty field should also surface the
    /// Ctrl+⌫ tip. Call after `key` returned `None` for Backspace.
    pub fn take_backspace_tip(&mut self, ctx: &Ctx) -> bool {
        if self.backspace_count >= 3 && !ctx.ctrl_backspace_hint_shown {
            self.backspace_count = 0;
            return true;
        }
        false
    }

    fn emoji_pos(&self) -> Option<usize> {
        if self.emojis.is_empty() {
            None
        } else {
            Some(1.min(self.candidates.len().saturating_sub(1)))
        }
    }
}

/// Removes the first `n` whitespace-separated words (and the whitespace after
/// them) from `raw`.
fn drop_words(raw: &str, n: usize) -> String {
    let mut rest = raw.trim_start();
    for _ in 0..n {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        rest = rest[end..].trim_start();
    }
    rest.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    fn engine() -> &'static TransliterationEngine {
        static E: OnceLock<TransliterationEngine> = OnceLock::new();
        E.get_or_init(TransliterationEngine::new)
    }

    fn cx(prefs: &UserPreferences) -> Ctx<'_> {
        Ctx { engine: engine(), prefs, ctrl_backspace_hint_shown: true }
    }

    struct T {
        s: Session,
        prefs: UserPreferences,
        field: Field,
    }

    impl T {
        fn new() -> T {
            T { s: Session::new(default_starters()), prefs: UserPreferences::new(), field: Field::default() }
        }
        fn apply(&mut self, effects: Vec<Effect>) -> Vec<Effect> {
            for e in &effects {
                if let Effect::SetField { text, select_all } = e {
                    let n = text.chars().count();
                    self.field = Field { text: text.clone(), sel: if *select_all { (0, n) } else { (n, n) } };
                }
            }
            effects
        }
        fn type_text(&mut self, s: &str) {
            for c in s.chars() {
                if c == ' ' {
                    if self.key(Key::Space).is_some() {
                        continue;
                    }
                }
                self.field.text.push(c);
                let n = self.field.text.chars().count();
                self.field.sel = (n, n);
                let raw = self.field.text.clone();
                let ctx = Ctx { engine: engine(), prefs: &self.prefs, ctrl_backspace_hint_shown: true };
                let e = self.s.input_changed(&ctx, &raw);
                self.apply(e);
            }
        }
        fn key(&mut self, k: Key) -> Option<Vec<Effect>> {
            let ctx = Ctx { engine: engine(), prefs: &self.prefs, ctrl_backspace_hint_shown: true };
            let field = self.field.clone();
            let r = self.s.key(&ctx, k, Mods::default(), &field);
            r.map(|e| self.apply(e))
        }
        fn preview_text(&self) -> String {
            self.s
                .preview()
                .iter()
                .map(|p| match p {
                    PreviewWord::Committed { text, .. } | PreviewWord::Editing { text, .. } | PreviewWord::Current { text } => text.clone(),
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
    }

    #[test]
    fn space_commits_and_enter_pastes() {
        let mut t = T::new();
        t.type_text("ezayak ");
        assert_eq!(t.s.committed.len(), 1);
        assert_eq!(t.field.text, "");
        t.type_text("ya");
        let e = t.key(Key::Enter).unwrap();
        assert!(matches!(e.last(), Some(Effect::Paste(s)) if s.split(' ').count() == 2));
    }

    #[test]
    fn multi_word_paste_commits_finished_words() {
        let mut t = T::new();
        let p = t.prefs.clone();
        let ctx = cx(&p);
        let e = t.s.input_changed(&ctx, "ahlan wa sahlan");
        assert_eq!(t.s.committed.len(), 2);
        assert_eq!(e, vec![Effect::SetField { text: "sahlan".into(), select_all: false }]);
    }

    #[test]
    fn clicking_an_earlier_word_keeps_the_word_being_typed() {
        let mut t = T::new();
        t.type_text("ezayak ya habibi 3amel");
        assert_eq!(t.s.committed.len(), 3);
        let e = t.s.edit_word(1);
        t.apply(e);
        // 3amel was committed, not lost.
        assert_eq!(t.s.committed.len(), 4);
        assert_eq!(t.s.committed[3].input, "3amel");
        assert_eq!(t.s.editing, Some(1));
        assert_eq!(t.field.text, "ya");
        assert_eq!(t.field.sel, (0, 2));
        // Retyping and committing replaces only the edited word.
        t.field = Field::new("");
        t.type_text("yaa ");
        assert_eq!(t.s.committed.len(), 4);
        assert_eq!(t.s.committed[1].input, "yaa");
        assert_eq!(t.s.editing, None);
    }

    #[test]
    fn backspace_on_empty_field_edits_last_word() {
        let mut t = T::new();
        t.type_text("salam ");
        t.key(Key::Backspace).unwrap();
        assert_eq!(t.s.editing, Some(0));
        assert_eq!(t.field.text, "salam");
    }

    #[test]
    fn tab_keeps_latin() {
        let mut t = T::new();
        t.type_text("hello");
        t.key(Key::Tab).unwrap();
        assert_eq!(t.s.committed[0].arabic, "hello");
    }

    #[test]
    fn allah_is_shown_and_pasted_vocalized() {
        let mut t = T::new();
        t.type_text("allah ");
        assert_eq!(t.s.committed[0].arabic, "الله");
        assert_eq!(t.preview_text(), "اللّٰه");
        assert_eq!(t.s.output_text(), "اللّٰه");
    }

    #[test]
    fn letter_picker_swaps_a_letter() {
        let mut t = T::new();
        t.type_text("zaki");
        // Caret before "z" (a caret right after a letter targets the next slot, as in the web UI).
        t.field.sel = (0, 0);
        let p = t.prefs.clone();
        let ctx = cx(&p);
        let field = t.field.clone();
        t.s.open_picker(&ctx, Mode::Letter, &field);
        let p = t.s.picker.clone().unwrap();
        assert_eq!(p.label(&t.s.letter_slots), "z");
        assert!(p.chips.len() > 1);
        let alt = p.chips.iter().position(|c| c.display != "ز").unwrap();
        let want = p.chips[alt].display.clone();
        let p = t.prefs.clone();
        let ctx = cx(&p);
        t.s.commit_picker(&ctx, Some(alt));
        assert!(t.s.candidates[0].starts_with(&want), "{:?}", t.s.candidates);
        assert!(t.s.show_lock());
        // Undo restores the previous letter.
        let field = t.field.clone();
        let p = t.prefs.clone();
        let ctx = cx(&p);
        t.s.key(&ctx, Key::Command('z'), Mods { meta: true, ..Default::default() }, &field).unwrap();
        assert!(t.s.overrides.is_empty());
    }

    #[test]
    fn landing_navigation_is_rtl() {
        let mut t = T::new();
        t.key(Key::Down).unwrap();
        assert_eq!(t.s.selected_starter, Some(0));
        t.key(Key::Left).unwrap();
        assert_eq!(t.s.selected_starter, Some(1));
        t.key(Key::Down).unwrap();
        assert_eq!(t.s.selected_starter, Some(3));
        let e = t.key(Key::Enter).unwrap();
        assert_eq!(e, vec![Effect::Paste("صباح الخير".into())]);
    }

    #[test]
    fn drop_words_keeps_the_tail() {
        assert_eq!(drop_words("  ahlan wa sahlan", 2), "sahlan");
        assert_eq!(drop_words("a b ", 2), "");
    }
}
