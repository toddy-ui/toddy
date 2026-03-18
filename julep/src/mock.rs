//! Mock mode (`--mock`): Core + wire protocol, no rendering.
//!
//! A lightweight mode for fast protocol-level testing. Processes
//! messages through [`Core`](julep_core::engine::Core) and responds
//! with synthetic events, but does not render widgets or maintain
//! iced widget state. Screenshots return empty stubs.
//!
//! This is the fastest way to test a julep app from any language
//! that speaks the wire protocol. For accurate visual screenshots,
//! use `--headless` (software rendering) or the default daemon mode
//! (GPU rendering).

use std::io::{self, BufRead};

use iced::Theme;

use julep_core::codec::Codec;
use julep_core::engine::Core;
use julep_core::extensions::{ExtensionCaches, ExtensionDispatcher};
use julep_core::image_registry::ImageRegistry;
use julep_core::protocol::IncomingMessage;

/// Run the mock event loop.
///
/// No renderer, no persistent UI, no widget state. Messages are
/// processed through Core and responses emitted to stdout. Scripting
/// messages (Query, Interact, etc.) produce synthetic events only.
pub fn run(forced_codec: Option<Codec>, mut dispatcher: ExtensionDispatcher) {
    let mut core = Core::new();
    let mut ext_caches = ExtensionCaches::new();
    let mut images = ImageRegistry::new();
    let mut theme = Theme::Dark;
    let stdin = io::stdin();
    let mut reader = io::BufReader::new(stdin.lock());

    let codec = match forced_codec {
        Some(c) => c,
        None => {
            let buf = match reader.fill_buf() {
                Ok(b) => b,
                Err(e) => {
                    log::error!("failed to read stdin: {e}");
                    return;
                }
            };
            if buf.is_empty() {
                log::error!("stdin closed before first message");
                return;
            }
            Codec::detect_from_first_byte(buf[0])
        }
    };
    log::info!("wire codec: {codec}");
    Codec::set_global(codec);

    crate::renderer::emit_hello();

    loop {
        match codec.read_message(&mut reader) {
            Ok(None) => break,
            Ok(Some(bytes)) => match codec.decode::<IncomingMessage>(&bytes) {
                Ok(msg) => handle_message(
                    &mut core,
                    &mut theme,
                    &mut dispatcher,
                    &mut ext_caches,
                    &mut images,
                    msg,
                ),
                Err(e) => {
                    log::error!("decode error: {e}");
                }
            },
            Err(e) => {
                log::error!("read error: {e}");
                break;
            }
        }
    }

    log::info!("stdin closed, exiting");
}

#[allow(clippy::too_many_arguments)]
fn handle_message(
    core: &mut Core,
    theme: &mut Theme,
    dispatcher: &mut ExtensionDispatcher,
    ext_caches: &mut ExtensionCaches,
    images: &mut ImageRegistry,
    msg: IncomingMessage,
) {
    let is_snapshot = matches!(msg, IncomingMessage::Snapshot { .. });
    let is_tree_change = is_snapshot || matches!(msg, IncomingMessage::Patch { .. });

    match msg {
        // Messages that go through Core::apply().
        IncomingMessage::Snapshot { .. }
        | IncomingMessage::Patch { .. }
        | IncomingMessage::EffectRequest { .. }
        | IncomingMessage::WidgetOp { .. }
        | IncomingMessage::SubscriptionRegister { .. }
        | IncomingMessage::SubscriptionUnregister { .. }
        | IncomingMessage::WindowOp { .. }
        | IncomingMessage::Settings { .. }
        | IncomingMessage::ImageOp { .. } => {
            let effects = core.apply(msg);

            for effect in effects {
                use julep_core::engine::CoreEffect;
                match effect {
                    CoreEffect::EmitEvent(event) => {
                        crate::scripting::emit_wire(&event);
                    }
                    CoreEffect::EmitEffectResponse(response) => {
                        crate::scripting::emit_wire(&response);
                    }
                    CoreEffect::SpawnAsyncEffect {
                        request_id,
                        effect_type,
                        ..
                    } => {
                        log::debug!(
                            "mock: async effect {effect_type} returning cancelled (no display)"
                        );
                        crate::scripting::emit_wire(&julep_core::protocol::EffectResponse::error(
                            request_id,
                            "cancelled".to_string(),
                        ));
                    }
                    CoreEffect::ThemeChanged(t) => {
                        *theme = t;
                    }
                    CoreEffect::ImageOp {
                        op,
                        handle,
                        data,
                        pixels,
                        width,
                        height,
                    } => {
                        if let Err(e) = images.apply_op(&op, &handle, data, pixels, width, height) {
                            log::warn!("mock: image_op {op} failed: {e}");
                        }
                    }
                    CoreEffect::ExtensionConfig(config) => {
                        dispatcher.init_all(&config);
                    }
                    // No-ops in mock mode (no windows, no iced widget tree).
                    CoreEffect::SyncWindows => {}
                    CoreEffect::WidgetOp { .. } => {}
                    CoreEffect::WindowOp { .. } => {}
                    CoreEffect::ThemeFollowsSystem => {}
                }
            }

            if is_tree_change {
                if is_snapshot {
                    dispatcher.clear_poisoned();
                }
                if let Some(root) = core.tree.root() {
                    dispatcher.prepare_all(root, ext_caches, theme);
                }
            }
        }

        // Scripting messages
        IncomingMessage::Query {
            id,
            target,
            selector,
        } => {
            crate::scripting::handle_query(core, id, target, selector);
        }
        IncomingMessage::Interact {
            id,
            action,
            selector,
            payload,
        } => {
            // Synthetic events only -- no iced event injection in mock mode.
            crate::scripting::handle_interact(core, id, action, selector, payload);
        }
        IncomingMessage::SnapshotCapture { id, name, .. } => {
            crate::scripting::handle_snapshot_capture(core, id, name);
        }
        IncomingMessage::ScreenshotCapture { id, name, .. } => {
            // Stub -- no rendering in mock mode.
            crate::renderer::emitters::emit_screenshot_response(&id, &name, "", 0, 0, &[]);
        }
        IncomingMessage::Reset { id } => {
            dispatcher.reset(ext_caches);
            *images = ImageRegistry::new();
            *theme = Theme::Dark;
            crate::scripting::handle_reset(core, id);
        }
        IncomingMessage::ExtensionCommand {
            node_id,
            op,
            payload,
        } => {
            let events = dispatcher.handle_command(&node_id, &op, &payload, ext_caches);
            for event in events {
                crate::scripting::emit_wire(&event);
            }
        }
        IncomingMessage::ExtensionCommandBatch { commands } => {
            for cmd in commands {
                let events =
                    dispatcher.handle_command(&cmd.node_id, &cmd.op, &cmd.payload, ext_caches);
                for event in events {
                    crate::scripting::emit_wire(&event);
                }
            }
        }
        IncomingMessage::AdvanceFrame { timestamp } => {
            if let Some(tag) = core.active_subscriptions.get("on_animation_frame") {
                crate::scripting::emit_wire(&julep_core::protocol::OutgoingEvent::animation_frame(
                    tag.clone(),
                    timestamp as u128,
                ));
            }
        }
    }
}
