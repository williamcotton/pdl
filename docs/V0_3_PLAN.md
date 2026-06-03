# PDL v0.3 Plan

Status: Shipped
Target version: 0.3.0

## Release Thesis

PDL v0.3 turns the diagnostic model into a first-class source-language surface.
The release replaces the early placeholder family with stable,
category-specific diagnostic codes before larger language and runtime expansion,
following the catalog-driven workflow that keeps diagnostics reliable
across parser, analyzer, CLI, editor, and runtime work.

## Must

### Lettered Diagnostics Catalog

Status: Landed in 0.3.0.

Introduce lettered diagnostic namespaces:

- `E0000`-style codes for errors.
- `W0000`-style codes for warnings.
- `H0000`-style codes for hints.
- `R0000`-style codes for implementation/runtime-internal diagnostics.

The placeholder namespace is removed before external release.

### Diagnostic Coverage

Status: Landed in 0.3.0.

Expand `docs/PDL_SPEC.md` so every normative author-facing parser, semantic,
CLI, data-source, runtime, interop, warning, and hint condition has a reserved
stable diagnostic code.

## Should

### Diagnostic Code Migration

Status: Landed in 0.3.0.

Move the Rust implementation away from stringly diagnostic code emissions toward
centralized registered diagnostic codes. `pdl-core` owns `DiagnosticCode`,
`codes::*`, and the registered-code list.

### Diagnostic Tests

Status: Landed in 0.3.0.

Add focused tests for diagnostic code stability, spans, severity, CLI stderr
rendering, LSP serialization, and non-ASCII source offsets.

## Deferred

- Full localization of diagnostic messages.
- Machine-readable diagnostic help URLs.
- Diagnostic-code localization metadata.
