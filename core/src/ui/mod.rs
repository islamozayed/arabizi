//! The widget's view layer, shared by every platform. It lays out and paints
//! through [`paint::Painter`], tracks hover/scroll/animation state, and turns
//! input events into [`Command`]s for the platform shell to carry out.
//!
//! Sizes are CSS px from the original stylesheet (1 px = 1 pt on macOS).

pub mod access;
pub mod anim;
pub mod geom;
pub mod icons;
pub mod onboarding;
pub mod paint;
pub mod path;
pub mod theme;
mod draw_floating;
mod draw_main;
mod draw_settings;
mod widget;

pub use widget::*;
