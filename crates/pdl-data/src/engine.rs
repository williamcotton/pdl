#[cfg(feature = "polars-engine")]
pub fn native_engine_name() -> &'static str {
    let _ = std::any::type_name::<polars::prelude::DataFrame>();
    "polars"
}

#[cfg(not(feature = "polars-engine"))]
pub fn native_engine_name() -> &'static str {
    "in-memory"
}
