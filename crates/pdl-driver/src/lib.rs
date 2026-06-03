pub mod cache;
pub mod facts;
pub mod io;
pub mod path;
pub mod plan;
pub mod prepare;
pub mod report;
pub mod source;

pub use cache::{PreviewRequest, SchemaCacheEntry, SchemaCacheKey, SourceIdentity};
pub use facts::{ExternalFacts, InMemoryFacts};
pub use io::{DriverIo, DriverMetadata, InMemoryDriverIo, OsDriverIo};
pub use path::{resolve_input_path, resolve_output_path};
pub use plan::{
    DriverPlan, FormatDecision, PipelineLabel, PlanInputSource, PlanOutputSink, SinkDescriptor,
    SniffingDecision, SniffingReason, SourceDependency, SourceDescriptor, StreamDirection,
    StreamKind, StreamUse,
};
pub use prepare::{prepare_file, prepare_source, prepare_source_with_io, program, PreparedProgram};
pub use report::{PhaseDiagnostic, PreparationReport, ReportPhase};
pub use source::{SourceInput, SourceOrigin};
