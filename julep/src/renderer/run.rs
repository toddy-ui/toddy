//! Renderer entry point. Parses CLI flags, reads the initial Settings
//! message, spawns the stdin reader, and starts the iced daemon.

use std::sync::Mutex;

use iced::Task;

use julep_core::codec::Codec;
use julep_core::message::{Message, StdinEvent};
use julep_core::protocol::IncomingMessage;

use super::App;
use super::emitters::emit_hello;
use super::stdin::{STDIN_RX, read_initial_settings, spawn_stdin_reader};

pub(crate) fn run(builder: julep_core::app::JulepAppBuilder) -> iced::Result {
    let args: Vec<String> = std::env::args().collect();

    // Levelled logging via RUST_LOG. Default: warn (quiet). Use
    // RUST_LOG=julep=debug (or =info, =trace) for more output.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    // Parse codec flags early so all modes (headless, test, normal) can use them.
    let has_flag = |flag: &str| args.iter().any(|a| a == flag);
    let forced_codec = if has_flag("--msgpack") {
        Some(Codec::MsgPack)
    } else if has_flag("--json") {
        Some(Codec::Json)
    } else {
        None
    };

    // Parse --max-sessions N for concurrent session support.
    let max_sessions = args
        .windows(2)
        .find(|w| w[0] == "--max-sessions")
        .and_then(|w| w[1].parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);

    if has_flag("--mock") {
        crate::headless::run(
            forced_codec,
            builder.build_dispatcher(),
            crate::headless::Mode::Mock,
            max_sessions,
        );
        return Ok(());
    }
    if has_flag("--headless") {
        crate::headless::run(
            forced_codec,
            builder.build_dispatcher(),
            crate::headless::Mode::Headless,
            max_sessions,
        );
        return Ok(());
    }

    // Read the first message synchronously to get iced settings and font
    // data before the daemon starts. This must happen before the stdin
    // reader thread is spawned.
    let (initial_settings, iced_settings, font_bytes, reader) = read_initial_settings(forced_codec);

    // Send the hello handshake before any other output. The codec is set
    // inside read_initial_settings, so it's safe to emit framed messages now.
    if let Err(e) = emit_hello("windowed") {
        log::error!("failed to emit hello: {e}");
        return Ok(());
    }

    // Spawn stdin reader thread with tokio channel. The receiver goes into
    // STDIN_RX so the subscription (which is a fn pointer, not a closure)
    // can take it once on first call.
    let (tx, rx) = tokio::sync::mpsc::channel::<StdinEvent>(64);
    spawn_stdin_reader(tx, reader);
    *STDIN_RX.lock().expect("STDIN_RX lock poisoned") = Some(rx);

    let settings_slot: Mutex<Option<(serde_json::Value, Vec<Vec<u8>>)>> =
        Mutex::new(Some((initial_settings, font_bytes)));
    let builder_slot: Mutex<Option<julep_core::app::JulepAppBuilder>> = Mutex::new(Some(builder));

    iced::daemon(
        move || {
            let (settings, fonts) = settings_slot
                .lock()
                .expect("settings_slot lock poisoned")
                .take()
                .unwrap_or_default();

            let dispatcher = builder_slot
                .lock()
                .expect("builder_slot lock poisoned")
                .take()
                .expect("daemon init closure called more than once")
                .build_dispatcher();
            let mut app = App::new(dispatcher);

            // Extract scale_factor before applying settings to Core
            app.scale_factor = super::app::validate_scale_factor(
                settings
                    .get("scale_factor")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32)
                    .unwrap_or(1.0),
            );

            // Apply initial settings to Core. Handle any effects (e.g.
            // ExtensionConfig when the Settings includes extension_config).
            let effects = app.core.apply(IncomingMessage::Settings { settings });
            for effect in effects {
                match effect {
                    julep_core::engine::CoreEffect::ExtensionConfig(config) => {
                        app.dispatcher.init_all(&config);
                    }
                    other => {
                        log::warn!("unexpected effect from initial Settings: {other:?}");
                    }
                }
            }

            // Build font load tasks
            let font_tasks: Vec<Task<Message>> = fonts
                .into_iter()
                .map(|bytes| {
                    iced::font::load(bytes).map(|result| {
                        if let Err(e) = result {
                            log::error!("font load error: {e:?}");
                        }
                        Message::NoOp
                    })
                })
                .collect();

            let task = if font_tasks.is_empty() {
                Task::none()
            } else {
                Task::batch(font_tasks)
            };

            (app, task)
        },
        App::update,
        App::view_window,
    )
    .title(App::title_for_window)
    .subscription(App::subscription)
    .theme(App::theme_for_window)
    .scale_factor(App::scale_factor_for_window)
    .settings(iced_settings)
    .run()
}
