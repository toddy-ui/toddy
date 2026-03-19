# toddy

Rust renderer binary for Toddy. Receives UI tree diffs over stdin
(MessagePack or JSONL), renders them via iced, and sends events back
over stdout.

## Before committing

Run `just preflight`. It mirrors CI exactly: check, clippy, fmt, test.

## Quick reference

```
just preflight                     # run all CI checks locally
just check                         # fast compile check
just clippy                        # lint (same flags as CI)
just test                          # run tests (nextest, CI profile)
just test-cargo                    # run tests (plain cargo test)
just test-filter <pattern>         # run matching tests (nextest)
just test-crate <crate>            # run tests for one crate (nextest)
just build                         # debug build (workspace)
just build-release                 # optimized release build (workspace)
just format                        # auto-format
just fmt                           # check formatting (CI mode)
just watch-check                   # cargo watch: check on save
just watch-test                    # cargo watch: test on save
just docs                          # build and open rustdocs
just audit                         # cargo audit
just outdated                      # cargo outdated
just coverage                      # code coverage (llvm-cov or tarpaulin)
cargo build                        # debug build
cargo build --release              # release build
cargo test                         # run all Rust tests (without nextest)
cargo fmt                          # auto-format
cargo fmt --check                  # check formatting
cargo clippy -- -D warnings        # lint
# Environment variables (not commands):
RUST_LOG=toddy=debug               # verbose binary logging
RUST_LOG=toddy_core=debug          # verbose core library logging
```

Nextest config: `.config/nextest.toml` (slow-timeout, CI profile).

## Project layout

```
toddy-core/          # library crate (the SDK for extensions)
  src/
    lib.rs               # public re-exports and module guide
    engine.rs            # Core struct: pure state management decoupled from iced
    widgets/             # tree node -> iced widget rendering
      mod.rs             # widget module declarations and public re-exports
      render.rs          # main render dispatch: maps TreeNode to iced Element
      caches.rs          # WidgetCaches, ensure_caches/render split for mutable state
      layout.rs          # layout widgets (column, row, container, stack, grid, etc.)
      display.rs         # display widgets (text, rich_text, image, svg, markdown, etc.)
      input.rs           # input widgets (text_input, text_editor, checkbox, slider, etc.)
      interactive.rs     # interactive wrappers (button, mouse_area, sensor, tooltip, etc.)
      canvas.rs          # canvas widget: 2D drawing surface with per-layer caching
      table.rs           # table widget: scrollable data grid with sortable columns
      overlay.rs         # custom overlay widget: positioned popup anchored to sibling
      a11y.rs            # A11yOverride wrapper for host-side a11y overrides + auto-inference
      helpers.rs         # internal widget helpers: parsing, style application, utilities
      validate.rs        # debug-mode prop validation against per-widget schemas
    protocol/            # wire message parsing and event serialization
      mod.rs             # wire protocol types, re-exports, session message wrapper
      incoming.rs        # IncomingMessage enum (Snapshot, Patch, Effect, etc.)
      outgoing.rs        # OutgoingEvent struct and constructor methods
      types.rs           # TreeNode and PatchOp wire types for the UI tree
    message.rs           # Message enum, keyboard/mouse serialization helpers
    tree.rs              # retained UI tree, snapshot replacement, patch application
    codec.rs             # wire codec: JSON + MessagePack encode/decode/framing
    theming.rs           # theme resolution, custom palette parsing, hex colors
    effects.rs           # platform effect handlers (file dialogs, clipboard, notifications)
    image_registry.rs    # in-memory image handle storage
    extensions.rs        # WidgetExtension trait, ExtensionDispatcher, ExtensionCaches
    app.rs               # ToddyAppBuilder for registering extensions
    prop_helpers.rs      # public prop extraction helpers for extensions
    prelude.rs           # common re-exports for extension authors
    testing.rs           # test factory helpers for extension authors
toddy/               # binary crate (thin main + iced daemon)
  src/
    main.rs              # binary entrypoint (delegates to toddy::run)
    lib.rs               # crate-level module declarations (three execution modes)
    renderer/            # iced::daemon App and supporting modules
      mod.rs             # module declarations and re-exports
      app.rs             # App struct definition, core utility methods (title, theme, scale)
      run.rs             # renderer entry point: CLI parsing, settings, stdin spawn, daemon start
      apply.rs           # incoming message processing: delegates to Core, handles effects
      update.rs          # message dispatcher: routes iced messages to handlers
      view.rs            # renders a window's UI tree into iced Elements
      events.rs          # subscription event handlers (keyboard, mouse, touch, IME, window)
      subscriptions.rs   # builds iced Subscription list from host-registered event types
      emitters.rs        # stdout emitters for events, handshake, effects, queries, screenshots
      stdin.rs           # stdin I/O: initial settings reader, background thread, subscription
      window_ops.rs      # window operations: open, close, resize, move, maximize, etc.
      window_map.rs      # bidirectional toddy ID <-> iced window::Id mapping
      widget_ops.rs      # widget operations: focus, scroll, cursor, pane, font, images
      constants.rs       # string constants for subscription keys and protocol values
    headless.rs          # headless and mock modes (--headless, --mock) with session multiplexing
    scripting.rs         # scripting protocol helpers, iced event construction, tree search
```

toddy-core is the public SDK. Extension authors depend on it to implement
the `WidgetExtension` trait. toddy is a thin binary that wires
everything together and runs `iced::daemon`.

## Architecture

- **stdin/stdout protocol.** The binary reads messages from stdin
  (MessagePack by default, JSONL available) and writes events to stdout.
  All log output goes to stderr.
- **iced::daemon.** The renderer runs as an `iced::daemon` application,
  which supports multiple windows without forcing a default window and
  keeps running when all windows are closed (important for the
  stdin-driven model).
- **Tree rendering.** A retained tree of UI nodes is maintained in Core
  (`engine.rs`). Snapshots replace the full tree; patches update it
  incrementally. The widget mapper in `widgets/` walks the tree and
  maps each node type to an iced widget.
- **Multi-window.** The host drives window open/close via tree nodes. The
  renderer maintains bidirectional `toddy_id <-> window::Id` maps.
- **Three modes.** Windowed (default, full iced rendering with real
  windows), `--headless` (real rendering via tiny-skia, no display
  server), `--mock` (protocol-only, no rendering, stub screenshots).
  The hello message reports the mode (`"windowed"`, `"headless"`,
  `"mock"`) so SDKs can adapt.
- **Session multiplexing.** Headless and mock modes support concurrent
  sessions via `--max-sessions N`. Each session runs in its own thread
  with isolated state. Messages are dispatched by the `session` field.
  Default is 1 (single session, no threading overhead).

## Non-obvious patterns

**Radio group prop.** Radio buttons use a `group` prop so all radios in the
same logical group emit select events with the group name as the event ID
(not the individual radio's node ID). Without this, the host's update handler
can't pattern-match on a single event for the group.

**ensure_caches / render split.** `text_editor` and `markdown` widgets require
mutable state (`Content` and `Vec<Item>`) that must persist across renders,
but iced's `view()` only has `&self`. Solution: `ensure_caches()` runs in
`apply()` (mutable context) to populate `HashMap`s on the `App` struct.
`render()` in `view()` reads them immutably. No `RefCell` needed.

**Canvas layer caching.** Canvas widgets use per-layer `canvas::Cache` for
efficient re-rendering. `ensure_caches()` walks canvas nodes, hashes each
layer's shape JSON, and only clears the cache (triggering re-tessellation)
when content changes. The caches live in `WidgetCaches.canvas_caches` as
`HashMap<String, HashMap<String, (u64, canvas::Cache)>>` keyed by node ID
and layer name. `CanvasProgram::draw()` returns one `Geometry` per layer
via `cache.draw()`. Background is drawn uncached (single fill).

**pending_tasks drain.** Widget ops (focus, scroll, close_window) return iced
`Task`s, but `apply()` doesn't return them. They're pushed to a
`Vec<Task<Message>>` on the App and drained via `Task::batch` in the `Tick`
handler.

**Window sync.** The host detects window nodes in the tree and sends open/close
ops. The renderer maintains bidirectional `toddy_id <-> window::Id` maps.
`iced::daemon` was chosen over `iced::application` because it provides
per-window `view(window_id)`, doesn't force a default window, and keeps
running when all windows are closed (important for toddy's stdin-driven model).

**Headless event capture.** In `--headless` mode, Interact messages
inject iced events one at a time and capture the Messages that widgets
produce (via `ui.update`). Captured Messages are converted to
OutgoingEvents using the same `message_to_event` + `process_captured_messages`
logic as the windowed mode's `update()`. For each iced event that
produces widget Messages, an `interact_step` is emitted and the
renderer blocks until the host sends a Snapshot back, matching
production's per-event round-trip. In `--mock` mode, all events are
synthetic (no iced renderer to capture from).

**Custom themes.** JSON objects with hex color fields (`background`, `text`,
`primary`, `success`, `danger`) parsed into `iced::Theme::custom()` with a
`Palette`. Optional `base` field selects a starting palette to override.

**Encode protocol.** On the host side, the message encoding layer handles type
conversions automatically. Widget node implementations don't need manual
serialization transforms -- the protocol handles structured types (Color,
Padding, etc.) and passthrough for strings/numbers.

**rmp-serde internally-tagged enum workaround.** rmp-serde cannot reliably
deserialize `#[serde(tag = "type")]` enums when msgpack comes from external
producers (e.g. Msgpax). The `Codec::decode` method routes msgpack
through `rmpv::Value` then `serde_json::Value` as intermediates: `msgpack
bytes -> rmpv::Value -> serde_json::Value -> serde_json::from_value::<T>`.
The `rmpv` step preserves native binary fields (`rmpv::Value::Binary`),
which are converted to JSON byte arrays for tag dispatch, then reconstructed
into `Vec<u8>` by a custom deserializer in `protocol.rs`. This is in
`codec.rs`.

**Structured logging.** The renderer uses `log` + `env_logger`. All output
goes to stderr. The renderer's built-in default level is `warn`. Control via
`RUST_LOG` env var (always wins) or a host-side log level option that sets
`RUST_LOG` on the process environment. Per-module filtering works:
`RUST_LOG=toddy_core::widgets=debug`.

**A11y auto-inference.** Image and SVG widgets with an `alt` prop
automatically flow the alt text into the accessible label. Text input and
text editor widgets flow their `placeholder` prop into the accessible
description. These inferred values are overridden if the host provides
explicit `a11y.label` or `a11y.description` props.

**Event capture status.** All subscription events (keyboard, mouse, touch,
IME) carry an optional `captured` boolean on the outgoing event. `true`
means an iced widget already consumed the event (e.g. a TextEditor captured
Tab). `false` or absent means no widget handled the event. Widget-level
events (click, input, etc.) never carry this field.

**StyleMap preset base.** StyleMap objects can include a `"base"` field
naming a preset to extend. The parsed style starts from the preset's
defaults, then the remaining fields override individual properties. This
lets hosts customise a single aspect of a preset without restating
everything.

**Prop validation mode.** In debug builds, `validate_props()` always runs
and logs warnings for unexpected prop names or type mismatches. In release
builds, validation is off by default. The host can enable it at startup by
setting `validate_props: true` in the Settings message. The flag is stored
in a `OnceLock<bool>` and can only be set once per process lifetime.

**Tree sync check.** The `tree_hash` widget op computes a SHA-256 hash of
the renderer's current tree (serialized as JSON) and emits it as a query
response. The host can use this to verify that its local tree state matches
the renderer's, catching desync bugs early.

**Runtime font loading.** The `load_font` widget op accepts base64-encoded
font data (TTF/OTF) and loads it into iced's font system at runtime. This
complements the `fonts` array in Settings (which only works at startup).

**Default font and text size.** The Settings message accepts `default_font`
(a font descriptor object, e.g. `{"family": "monospace"}`) and
`default_text_size` (a float). Both are runtime-updateable -- sending a new
Settings message with these fields updates them for subsequent renders.

## No feature flags

All capabilities are compiled unconditionally. There are no Cargo
feature flags. Headless mode (`--headless`) and mock mode (`--mock`)
are runtime flags, not compile-time features.

## Protocol version handshake

The renderer reads a Settings message from stdin on startup, then emits
a `hello` message on stdout confirming the protocol version and wire
codec. The host validates the protocol version to ensure compatibility.
See `docs/protocol.md` for the full specification.

## iced fork (toddy-iced)

The renderer depends on a fork of iced. Cargo.toml dependencies use
`package = "toddy-iced"` aliases so source code still writes `use iced::*`.
The fork source lives at `~/projects/toddy-iced`. The workspace `Cargo.toml`
has `[patch.crates-io]` entries pointing at the local checkout for
development against unpublished fork changes. Remove the patch section
when publishing to crates.io.

## Extension development

toddy-core is the SDK for writing widget extensions. A native extension
has two halves: an Elixir module using `Toddy.Extension` (declares
props, commands, Rust crate path) and a Rust crate implementing
`WidgetExtension`. Pure Elixir composite widgets use the same macro
but skip the Rust side.

Rust-side extension authors:

1. Create a Rust crate that depends on `toddy-core`.
2. Implement the `WidgetExtension` trait from `toddy_core::prelude::*`.
3. Three required methods: `type_names()`, `config_key()`, `render()`.
4. Optional methods: `init()`, `prepare()`, `handle_event()`,
   `handle_command()`, `cleanup()`, `new_instance()`.

Extensions that will be used with `--max-sessions > 1` must implement
`new_instance()` to produce a fresh instance for each session. The
default implementation panics.

The `prelude` module re-exports everything an extension needs: `TreeNode`,
`Message`, `WidgetEnv`, `Element`, `EventResult`, `OutgoingEvent`,
`ExtensionCaches`, `GenerationCounter`, prop helpers, and common iced types.

For iced types not in the prelude (e.g. `canvas::Path`, advanced layout
widgets), use `toddy_core::iced::*` instead of adding a direct `iced`
dependency. This avoids version conflicts -- when toddy-core bumps its
iced version, extensions get the upgrade automatically.

The `toddy_core::testing` module (`toddy-core/src/testing.rs`) provides
test factory helpers: `node()`, `node_with_props()`, `node_with_children()`,
and render context builders so extension tests don't need to import half
the crate.

Extensions automatically get accessibility support through composition --
the renderer's a11y layer wraps all widget output (including extensions)
with `A11yOverride`, so extension authors don't need to implement
accesskit integration themselves. Hosts set a11y props on extension
nodes the same way as built-in widgets.

See the `WidgetExtension` trait documentation and examples in
`toddy-core/src/extensions.rs` for the full API reference.
