# Franconvert

A small, keyboard-first desktop utility that converts Franco-Arabic (a.k.a. Arabizi — Arabic written with Latin letters and digits, e.g. `ahlan`, `3arabi`, `7abibi`) into proper Arabic script and pastes it into whatever app you were just using.

Lives in the menu bar / system tray. Press a global shortcut, type, hit Enter, and the Arabic lands in the field you were focused on.

Built with Tauri 2 + Rust. Runs on macOS, Windows, and Linux.

## Install

Grab the latest installer for your platform from the [**Releases page**](https://github.com/islamozayed/arabizi/releases/latest).

### macOS (Apple Silicon)

1. Download `Franconvert_<version>_aarch64.dmg`
2. Open the DMG and drag **Franconvert** into Applications
3. **First launch only:** macOS will warn that it can't verify the developer. **Right-click the app → Open → Open** in the second dialog. macOS remembers your choice and won't ask again.
4. The app lives in your menu bar — press `⌘⇧A` to summon it

> _Why the warning?_ Franconvert is signed with a Developer ID but not yet notarized by Apple. Notarization will be added in a future release; the right-click bypass is a one-time step.

### Windows

1. Download `Franconvert_<version>_x64-setup.exe` (or the `.msi` if you prefer)
2. Run the installer
3. Press `Ctrl+Shift+A` anywhere to summon the overlay

If Windows SmartScreen flags the installer ("Windows protected your PC"), click **More info → Run anyway**.

### Linux

Three formats — pick the one that matches your distro:

- **AppImage** (any distro): download `franconvert_<version>_amd64.AppImage`, `chmod +x` it, and run it
- **Debian / Ubuntu**: download `Franconvert_<version>_amd64.deb` and `sudo apt install ./Franconvert_<version>_amd64.deb`
- **Fedora / RHEL**: download `Franconvert-<version>-1.x86_64.rpm` and `sudo dnf install ./Franconvert-<version>-1.x86_64.rpm`

Press `Ctrl+Shift+A` to summon the overlay. Note: tray icon visibility depends on your desktop environment.

### Updates

Franconvert checks for updates on each launch. When a new release is published, you'll see a banner with **Install** / **Later** — clicking Install downloads + verifies the new version in the background, then offers a one-click Restart. You can also manually check anytime via **Settings → System → Check for updates**.



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
