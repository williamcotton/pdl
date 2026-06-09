//! Row-vs-native parity harness for the PDL examples (v0.43).
//!
//! This crate intentionally has an empty library target. The harness lives in
//! integration tests so every dependency stays a dev-dependency and the crate
//! can never be reached from the `pdl-wasm` target graph:
//!
//! * `tests/parity_examples.rs` runs every example in `examples/` through
//!   `pdl run` on the row and native engines and diffs the outputs. The row
//!   engine is the parity spec.
//! * `tests/selected_engine_fixtures.rs` is the silent-demotion canary: it
//!   pins each example's `PlanObservability.selected_engine` under
//!   `--engine auto` to a fixture in `fixtures/selected_engine/`.
