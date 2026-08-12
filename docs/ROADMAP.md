# Roadmap

This roadmap keeps the project focused on workflows where a TUI is meaningfully better than raw Cargo commands.

## v0.1.x: Stabilize MVP

- Keep startup reliable in Cargo and non-Cargo directories.
- Improve release artifacts and smoke-test documentation.
- Fix navigation, rendering, terminal size, and search edge cases.
- Keep build/check/test output responsive and scrollable.

## v0.2.0: Dependency Investigation

- Highlight duplicate crate versions (done — `t` shows `cargo tree --duplicates`).
- Reverse dependency inspection on a selected dependency (done — `i`).
- Show clearer feature-source chains: why a feature is enabled and which parent dependency pulled it in.

## v0.3.0: Target Disk Analysis

- Improve per-crate target attribution.
- Separate debug, release, and target-triple cache views.
- Add safer cleanup previews.
- Make stale artifact cleanup explain exactly what will be removed.

## v0.4.0: Build Telemetry

- Improve build history summaries.
- Surface slow crates from `cargo build --timings`.
- Compare recent check/build durations.
- Add better failure diagnostics from compiler output.

## Non-goals

- Replace Cargo.
- Become a generic crates.io search client.
- Hide underlying commands from users.
- Add broad async/runtime complexity before the existing subprocess and UI model needs it.
