# Contributing

## Setup

1. Install [Rust](https://rustup.rs) (stable, version in `rust-toolchain.toml`)
2. Install system dependencies (see README)
3. Install [just](https://just.systems) and [cargo-nextest](https://nexte.st)

## Development Workflow

    just check           # Fast compile check
    just test            # Run tests (nextest)
    just test-cargo      # Run tests (standard cargo, if nextest unavailable)
    just test-filter foo # Run tests matching "foo"
    just test-crate name # Run tests for a specific crate

## Before Committing

    just preflight

This runs the same checks as CI: `check`, `clippy`, `fmt`, `test`.

## Code Style

- `cargo fmt` for formatting (run `just format` to auto-fix)
- `cargo clippy` for linting (warnings are errors via RUSTFLAGS)
- Follow existing patterns in the codebase

## Commit Conventions

Use conventional commits:

    feat(core): add gradient fill support
    fix(renderer): handle missing window on close
    docs: update feature flag table
    test(codec): add msgpack round-trip tests
    refactor(widgets): extract common prop parsing

## Pull Requests

1. Create a branch from `master`
2. Make changes, ensure `just preflight` passes
3. Submit PR with a clear description
