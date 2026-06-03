use pdl_core::Diagnostic;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportPhase {
    Parse,
    SourceResolution,
    Schema,
    Semantic,
    Planning,
    Execute,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhaseDiagnostic {
    pub phase: ReportPhase,
    pub diagnostic: Diagnostic,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PreparationReport {
    pub diagnostics: Vec<PhaseDiagnostic>,
}

impl PreparationReport {
    pub fn push(&mut self, phase: ReportPhase, diagnostic: Diagnostic) {
        self.diagnostics.push(PhaseDiagnostic { phase, diagnostic });
    }

    pub fn extend(
        &mut self,
        phase: ReportPhase,
        diagnostics: impl IntoIterator<Item = Diagnostic>,
    ) {
        self.diagnostics.extend(
            diagnostics
                .into_iter()
                .map(|diagnostic| PhaseDiagnostic { phase, diagnostic }),
        );
    }

    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics
            .iter()
            .map(|entry| entry.diagnostic.clone())
            .collect()
    }
}
