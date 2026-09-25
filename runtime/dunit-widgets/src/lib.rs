//! Dunit Widgets — the widget layer of the UI Runtime.
//!
//! This is M4 slice 4. It is the integration layer that turns a styled DUI tree
//! into an interactive widget set. It builds on the three earlier slices:
//! [`dunit_ui`] (the retained tree + layout), [`dunit_style`] (the DSS cascade)
//! and [`dunit_text`] (glyph metrics for sizing text).
//!
//! * [`kind`] — the concrete [`kind::Widget`] set (text, icon, button, input,
//!   list, menu, slider, progress, notification) mapped from element tags, with
//!   focusability and interactivity.
//! * [`state`] — per-widget [`state::Interaction`] (hover/focus/press/disable/
//!   select) that feeds the DSS state cascade, and [`state::WidgetValue`] for
//!   value-bearing widgets.
//! * [`measure`] — intrinsic content sizing: a [`measure::TextMeasure`] source
//!   (with an adapter over [`dunit_text::Font`]) drives
//!   [`measure::intrinsic_size`], suitable for [`dunit_ui::layout_measured`].
//! * [`event`] — the event/focus model: [`event::Event`] and [`event::Key`],
//!   hit-testing, capture/bubble [`event::event_path`], and focus navigation.
//!
//! Everything is pure `no_std + alloc`; the only dependencies are the sibling
//! runtime crates, so the same code runs in host tests and on
//! `x86_64-unknown-none`.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod event;
pub mod kind;
pub mod measure;
pub mod state;

pub use event::{event_path, focus_next, focus_order, focus_prev, hit_test, Event, Key};
pub use kind::Widget;
pub use measure::{intrinsic_size, TextMeasure};
pub use state::{Interaction, WidgetValue};
