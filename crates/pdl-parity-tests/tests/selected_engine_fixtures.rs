// Silent-demotion canary (v0.43). Each example pins its expected
// `PlanObservability.selected_engine` under `--engine auto` in
// `fixtures/selected_engine/<example>.txt`. A flip in either direction fails
// this test. Updating a fixture must travel in the same commit as the
// corresponding plan promotion entry (see CLAUDE.md, "selected_engine fixture
// update protocol").

mod common;

use std::collections::BTreeSet;

use common::{
    example_name, example_sources, expected_selected_engine, fixtures_dir, plan_observability,
};
use pdl_exec::NativeUnsupportedReason;

#[test]
fn selected_engine_fixtures() {
    let fixture_dir = fixtures_dir().join("selected_engine");
    let mut unmatched_fixtures = std::fs::read_dir(&fixture_dir)
        .expect("read selected_engine fixtures")
        .map(|entry| entry.expect("read fixture entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "txt"))
        .map(|path| {
            path.file_stem()
                .expect("fixture stem")
                .to_str()
                .expect("utf-8 fixture name")
                .to_string()
        })
        .collect::<BTreeSet<_>>();

    for source in example_sources() {
        let name = example_name(&source);
        let expected = expected_selected_engine(&name);
        let observability = plan_observability(&source);
        let selected = observability["selected_engine"]
            .as_str()
            .expect("selected_engine in plan observability");

        assert_eq!(
            selected, expected,
            "{name}: selected_engine flipped from `{expected}` to `{selected}` under \
             --engine auto. Engine flips must be explicit: update \
             crates/pdl-parity-tests/fixtures/selected_engine/{name}.txt in the same \
             commit as the corresponding plan promotion entry (docs/V0_<minor>_PLAN.md), \
             with a one-line reference to the plan section."
        );

        let fallback_reason = observability["fallback_reason"].as_str();
        if selected == "row" {
            // Every runnable row-only cell must carry a typed
            // NativeUnsupportedReason; free-form or absent reasons are a
            // v0.43 contract violation.
            let reason = fallback_reason
                .unwrap_or_else(|| panic!("{name}: row-only example reports no fallback_reason"));
            assert!(
                NativeUnsupportedReason::all_codes().contains(&reason),
                "{name}: fallback_reason `{reason}` is not a member of the refined \
                 NativeUnsupportedReason surface"
            );
        } else {
            assert_eq!(
                fallback_reason, None,
                "{name}: natively selected example must not report a fallback_reason"
            );
        }

        unmatched_fixtures.remove(&name);
    }

    assert!(
        unmatched_fixtures.is_empty(),
        "stale selected_engine fixtures without a matching example: {unmatched_fixtures:?}"
    );
}
