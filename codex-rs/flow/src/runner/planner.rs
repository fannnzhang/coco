use crate::config::WorkflowSpec;

use super::WorkflowRunState;

#[derive(Debug, Clone, Copy)]
pub struct ResumePlan {
    pub next_step: usize,
    pub remaining_steps: usize,
    pub total_steps: usize,
}

impl ResumePlan {
    pub fn is_complete(&self) -> bool {
        self.remaining_steps == 0
    }
}

pub struct ResumePlanner<'a> {
    workflow: &'a WorkflowSpec,
}

impl<'a> ResumePlanner<'a> {
    pub fn new(workflow: &'a WorkflowSpec) -> Self {
        Self { workflow }
    }

    pub fn plan(&self, state: &WorkflowRunState) -> ResumePlan {
        let total_steps = self.workflow.steps.len();
        let pointer = state.resume_pointer.min(total_steps);
        ResumePlan {
            next_step: pointer,
            remaining_steps: total_steps.saturating_sub(pointer),
            total_steps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorkflowSpec;
    use crate::runner::state_store::WorkflowRunState;

    fn workflow_with_steps(count: usize) -> WorkflowSpec {
        WorkflowSpec {
            steps: vec![crate::config::StepSpec::default(); count],
            ..WorkflowSpec::default()
        }
    }

    #[test]
    fn detects_completed_plan() {
        let wf = workflow_with_steps(3);
        let mut state = WorkflowRunState {
            schema_version: super::super::state_store::WORKFLOW_STATE_SCHEMA_VERSION,
            workflow_name: "test".to_string(),
            run_id: "run".to_string(),
            resume_pointer: 3,
            steps: Vec::new(),
            token_usage: None,
        };
        let planner = ResumePlanner::new(&wf);
        let plan = planner.plan(&state);
        assert!(plan.is_complete());
        assert_eq!(plan.remaining_steps, 0);

        state.resume_pointer = 1;
        let pending = planner.plan(&state);
        assert!(!pending.is_complete());
        assert_eq!(pending.next_step, 1);
        assert_eq!(pending.remaining_steps, 2);
    }
}
