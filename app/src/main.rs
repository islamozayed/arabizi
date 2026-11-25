//! Franconvert: type Franco Arabic, get Arabic script.
//!
//! The widget's behaviour and layout live in `franconvert-core`; each
//! platform module here hosts it with that platform's native toolkit.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod migrate;
mod shortcut;
mod single_instance;
mod updater;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

fn main() {
    #[cfg(target_os = "macos")]
    macos::run();

    #[cfg(windows)]
    windows::run();

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        eprintln!("The native Linux front end is not written yet; see README.");
        std::process::exit(1);
    }
}
