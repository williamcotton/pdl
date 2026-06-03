use pdl_driver::prepare_source;

pub fn check_source(source: &str) -> Vec<pdl_core::Diagnostic> {
    prepare_source("memory.pdl", source).diagnostics()
}
