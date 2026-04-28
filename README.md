# Franconvert

A small, keyboard-first desktop utility that converts Franco-Arabic (a.k.a. Arabizi — Arabic written with Latin letters and digits, e.g. `ahlan`, `3arabi`, `7abibi`) into proper Arabic script and pastes it into whatever app you were just using.

Lives in the menu bar / system tray. Press a global shortcut, type, hit Enter, and the Arabic lands in the field you were focused on.

Built with Tauri 2 + Rust. Runs on macOS and Windows.

## Features

### Transliteration

- Real-time franco → Arabic conversion as you type.
- Per-word **suggestions panel** with ranked candidates — pick the spelling you actually meant.
- Live **preview panel** showing the full RTL output.
- **Number mappings**: `2 → ء`, `3 → ع`, `5 → خ`, `6 → ط`, `7 → ح`, `8 → ق`, `9 → ص`.
- **Letter combos**: `sh → ش`, `kh → خ`, `gh → غ`, `th → ث`, `dh → ذ`, `3' → غ`.
- Punctuation auto-mapped to Arabic equivalents (`? → ؟`, `, → ،`, `; → ؛`) — including punctuation attached to a word like `ya3ni?`.
- **Emoticon → emoji** conversion (e.g. `:)` becomes a smiley).
- **Edit committed words** by clicking them in the preview to swap candidates after you've moved on.

### Learning

- The engine **remembers which candidates you pick** for each input and boosts them to the top next time.
- Selection history persists across launches in the app data directory.

### Workflow

- **Global shortcut** to summon the overlay (default: `⌘⇧A` on macOS, `Ctrl+Shift+A` on Windows). Configurable in Settings.
- **Keyboard navigation**: arrow keys to pick a suggestion, Space to commit a word, Tab to skip, Enter to paste.
- **Paste-through**: pressing Enter hides the overlay, restores focus to the app you were just in, and synthesizes the OS paste shortcut so the Arabic drops into that field.
- **Draggable widget** — click and hold anywhere in the suggestions / preview / tips area to move the overlay around the screen. Position is remembered across launches.
- **First-run onboarding** that walks you through the shortcut, then dismisses itself.

### Appearance

- **Light, dark, and system themes**.
- **Accent color** — pick from presets, paste a hex code, or use the system accent (on Windows it reads the DWM accent automatically; on macOS the system accent is the default).
- **Compact mode** for a smaller overlay footprint.
- Native window backdrops:
  - macOS: vibrancy (FullScreenUI on Tahoe+, HudWindow on earlier versions).
  - Windows: acrylic + DWM rounded corners.

### Accessibility

- Large text mode.
- High contrast mode.
- Dyslexia-friendly font option.

### System integration

- **Tray-only / Accessory mode** — no dock icon, no taskbar entry.
- **Run on startup** toggle.
- Tray icon auto-switches between light and dark variants on Windows; macOS uses an OS template icon.

## Build

```bash
cargo tauri dev      # development
cargo tauri build    # release bundle
```

The Tauri config lives in [`app/tauri.conf.json`](app/tauri.conf.json); the transliteration engine is a separate crate at [`engine/`](engine/).
