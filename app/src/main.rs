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

fn main() {
    #[cfg(target_os = "macos")]
    macos::run();

    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("The native Windows and Linux front ends are not written yet; see README.");
        std::process::exit(1);
    }
}
