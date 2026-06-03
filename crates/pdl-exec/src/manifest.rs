use crate::planning::ExecutionPlan;

#[derive(Clone, Debug, PartialEq)]
pub struct RunManifest {
    pub plan: ExecutionPlan,
}
