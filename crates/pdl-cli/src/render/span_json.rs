// Shared `SpannedJson<T>` wrapper extracted from `render.rs` as part of the
// v0.42 split. See `render.rs` for the cross-module layout overview.

use pdl_core::Span;
use pdl_syntax::Spanned;
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct SpannedJson<T>
where
    T: Serialize,
{
    pub(crate) value: T,
    pub(crate) span: Span,
}

pub(crate) fn spanned_json<T>(spanned: &Spanned<T>) -> SpannedJson<T>
where
    T: Clone + Serialize,
{
    SpannedJson {
        value: spanned.value.clone(),
        span: spanned.span,
    }
}
