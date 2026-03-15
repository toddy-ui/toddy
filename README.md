# julep-renderer

Rust renderer binary for [Julep](https://github.com/lincracy/julep). Receives
UI tree diffs over stdin (MessagePack or JSONL), renders native desktop windows
via [iced](https://iced.rs), and sends events back over stdout.

## Components

- **julep-core** -- Library crate and extension SDK. Wire protocol, tree
  diffing, widget rendering, theming, and platform effects.
- **julep-renderer** -- Binary that wires julep-core into an iced::daemon
  application.

## Prerequisites

### Linux (Debian/Ubuntu)

    sudo apt-get install libxkbcommon-dev libwayland-dev libx11-dev \
      cmake libfontconfig1-dev pkg-config

### Linux (Arch)

    sudo pacman -S libxkbcommon wayland libx11 cmake fontconfig pkgconf

### macOS

    xcode-select --install

### Windows

No additional dependencies required.

## Quick Start

    cargo build
    cargo test

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `builtin-all` | yes | All built-in widget features |
| `dialogs` | yes | File dialog support |
| `clipboard` | yes | Clipboard read/write |
| `notifications` | yes | OS notification support |
| `a11y` | yes | Accessibility via accesskit (uses iced fork) |
| `headless` | no | Headless test backend (no display) |
| `test-mode` | no | Full test backend (real windows + test protocol) |

Individual widget features: `widget-image`, `widget-svg`, `widget-canvas`,
`widget-markdown`, `widget-highlighter`, `widget-sysinfo`, `widget-qr-code`.

## Development

Install [just](https://just.systems) and
[cargo-nextest](https://nexte.st) for the best experience.

    just preflight      # Run all CI checks locally
    just check          # Fast compile check
    just test           # Run tests
    just build-release  # Release build

See `just --list` for all available recipes.

## Accessibility

The `a11y` feature (enabled by default) integrates with the platform
accessibility layer via [accesskit](https://accesskit.dev). It uses a
[fork of iced](https://github.com/lincracy/iced/tree/a11y-accesskit) with
accesskit support in `iced_winit`, resolved automatically via a
`[patch.crates-io]` entry in the workspace `Cargo.toml`.

To build without accessibility:

    cargo build --no-default-features --features builtin-all,dialogs,clipboard,notifications

## Extension Development

julep-core is the SDK for custom widget extensions. See the
[extension guide](https://github.com/lincracy/julep/blob/master/docs/extensions.md)
in the main Julep repository.

## License

MIT
