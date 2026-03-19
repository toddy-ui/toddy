//! Common re-exports for widget extension authors.
//!
//! Import the entire prelude to get the types, traits, and helpers
//! needed to implement [`WidgetExtension`]:
//!
//! ```ignore
//! use toddy_core::prelude::*;
//! ```
//!
//! For iced types not covered here (e.g. `canvas::Path`, advanced
//! layout widgets), use `toddy_core::iced::*` instead of adding a
//! direct `iced` dependency. This avoids version conflicts when
//! toddy-core bumps its iced version.

// -- Extension trait and lifecycle types --
pub use crate::extensions::{
    EventResult, ExtensionCaches, GenerationCounter, RenderCtx, WidgetEnv, WidgetExtension,
};

// -- Wire protocol types --
pub use crate::message::Message;
pub use crate::protocol::{OutgoingEvent, TreeNode};

// -- Prop extraction helpers --
pub use crate::prop_helpers::*;

// -- Commonly needed iced types --
pub use crate::iced::widget::{
    button, canvas, checkbox, column, container, image, pick_list, progress_bar, row, rule,
    scrollable, slider, space, stack, text, toggler, tooltip,
};
pub use crate::iced::{Color, Element, Font, Length, Padding, Pixels, Theme};

// -- JSON (extensions parse props from serde_json::Value) --
pub use serde_json::Value;
