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
    let forced_codec = if args.contains(&"--msgpack".to_string()) {
        Some(Codec::MsgPack)
    } else if args.contains(&"--json".to_string()) {
        Some(Codec::Json)
    } else {
        None
    };

    {
        if args.contains(&"--mock".to_string()) {
            crate::headless::run(
                forced_codec,
                builder.build_dispatcher(),
                crate::headless::Mode::Mock,
            );
            return Ok(());
        }
        if args.contains(&"--headless".to_string()) {
            crate::headless::run(
                forced_codec,
                builder.build_dispatcher(),
                crate::headless::Mode::Headless,
            );
            return Ok(());
        }
    }

    // Read the first message synchronously to get iced settings and font
    // data before the daemon starts. This must happen before the stdin
    // reader thread is spawned.
    let (initial_settings, iced_settings, font_bytes, reader) = read_initial_settings(forced_codec);

    // Send the hello handshake before any other output. The codec is set
    // inside read_initial_settings, so it's safe to emit framed messages now.
    if let Err(e) = emit_hello() {
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
            let sf = settings
                .get("scale_factor")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32)
                .unwrap_or(1.0);
            app.scale_factor = if sf <= 0.0 || !sf.is_finite() {
                log::warn!("invalid initial scale_factor {sf}, using 1.0");
                1.0
            } else {
                sf
            };

            // Apply initial settings to Core (Settings doesn't produce effects)
            let _ = app.core.apply(IncomingMessage::Settings { settings });

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
