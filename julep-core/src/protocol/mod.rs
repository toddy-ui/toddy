//! Wire protocol types for host-renderer communication.
//!
//! The renderer reads [`IncomingMessage`]s from stdin and writes
//! [`OutgoingEvent`]s (plus response structs) to stdout.

mod incoming;
mod outgoing;
mod types;

/// Protocol version number. Sent in the `hello` handshake message on startup
/// and checked against the value the host embeds in Settings.
pub const PROTOCOL_VERSION: u32 = 1;

pub use incoming::{ExtensionCommandItem, IncomingMessage};
pub use outgoing::{
    EffectResponse, InteractResponse, KeyModifiers, OutgoingEvent, QueryResponse, ResetResponse,
    ScreenshotResponseEmpty, SnapshotCaptureResponse, emit_screenshot_response,
};
pub use types::{PatchOp, TreeNode};
