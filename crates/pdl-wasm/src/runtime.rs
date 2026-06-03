use pdl_driver::{prepare_source_with_io, InMemoryDriverIo};

pub fn check_source(source: &str) -> Vec<pdl_core::Diagnostic> {
    let io = InMemoryDriverIo::default();
    prepare_source_with_io("memory.pdl", source, &io).diagnostics()
}
