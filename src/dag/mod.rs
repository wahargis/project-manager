use crate::store::{Phase, PhaseStatus, ExperimentStatus, Store};

/// DAG execution engine for phase management.
/// Provides: topological sort, impact propagation, next phase selection,
/// auto-transition, stagnation detection, review gates.

pub struct DagEngine<'a, S: Store> {
    store: &'a S,
    project_id: i64,
}

impl<'a, S: Store> DagEngine<'a, S> {
    pub fn new(store: &'a S, project_id: i64) -> Self {
        Self { store, project_id }
    }

    /// Return phases in topological order (dependencies before dependents).
    pub fn topological_sort(&self) -> crate::store::Result<Vec<Phase>> {
        let phases = self.store.list_phases(self.project_id)?;
        let mut sorted = Vec::new();
        let mut visited = std::collections::HashSet::new();

        fn visit(
            phase: &Phase,
            phases: &[Phase],
            visited: &mut std::collections::HashSet<i64>,
            sorted: &mut Vec<Phase>,
        ) {
            if visited.contains(&phase.id) {
                return;
            }
            visited.insert(phase.id);
            for dep_id in &phase.depends_on {
                if let Some(dep) = phases.iter().find(|p| p.id == *dep_id) {
                    visit(dep, phases, visited, sorted);
                }
            }
            sorted.push(phase.clone());
        }

        for phase in &phases {
            visit(phase, &phases, &mut visited, &mut sorted);
        }
        Ok(sorted)
    }

    /// Get the next actionable phase: deps satisfied, not complete/deprioritized,
    /// sorted by impact descending.
    pub fn next_phases(&self) -> crate::store::Result<Vec<Phase>> {
        let phases = self.store.list_phases(self.project_id)?;
        let mut actionable = Vec::new();

        for phase in &phases {
            if phase.status == PhaseStatus::Complete || phase.status == PhaseStatus::Deprioritized {
                continue;
            }
            let deps_satisfied = phase.depends_on.iter().all(|dep_id| {
                phases.iter().any(|p| p.id == *dep_id && p.status == PhaseStatus::Complete)
            });
            if deps_satisfied {
                actionable.push(phase.clone());
            }
        }

        actionable.sort_by(|a, b| b.impact.cmp(&a.impact));
        Ok(actionable)
    }

    /// Detect stagnation: N consecutive failed experiments in the project.
    pub fn stagnation_check(&self, threshold: usize) -> crate::store::Result<Option<usize>> {
        let phases = self.store.list_phases(self.project_id)?;
        let mut all_exps = Vec::new();
        for phase in &phases {
            let exps = self.store.list_experiments(Some(phase.id))?;
            all_exps.extend(exps);
        }
        // Sort by created_at descending
        all_exps.sort_by(|a, b| b.id.cmp(&a.id));

        let mut consecutive_fails = 0usize;
        for exp in &all_exps {
            match exp.status {
                ExperimentStatus::Fail | ExperimentStatus::Inconclusive => {
                    consecutive_fails += 1;
                }
                ExperimentStatus::Pending => continue, // skip pending
                _ => break, // pass breaks the streak
            }
        }

        if consecutive_fails >= threshold {
            Ok(Some(consecutive_fails))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sqlite::SqliteStore;

    fn setup() -> (SqliteStore, i64) {
        let store = SqliteStore::in_memory().unwrap();
        let proj = store.create_project("test", None).unwrap();
        (store, proj.id)
    }

    #[test]
    fn topological_sort_respects_dependencies() {
        let (store, pid) = setup();
        let p1 = store.create_phase(pid, "A", 10, &[]).unwrap();
        let p2 = store.create_phase(pid, "B", 20, &[p1.id]).unwrap();
        let _p3 = store.create_phase(pid, "C", 30, &[p2.id]).unwrap();

        let dag = DagEngine::new(&store, pid);
        let sorted = dag.topological_sort().unwrap();
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].name, "A");
        assert_eq!(sorted[1].name, "B");
        assert_eq!(sorted[2].name, "C");
    }

    #[test]
    fn next_phases_returns_impact_sorted_actionable() {
        let (store, pid) = setup();
        let p1 = store.create_phase(pid, "Low", 10, &[]).unwrap();
        let _p2 = store.create_phase(pid, "High", 50, &[]).unwrap();
        let _p3 = store.create_phase(pid, "Blocked", 100, &[p1.id]).unwrap();

        let dag = DagEngine::new(&store, pid);
        let next = dag.next_phases().unwrap();
        // p3 is blocked by p1 (not complete), so only p1 and p2 actionable
        assert_eq!(next.len(), 2);
        assert_eq!(next[0].name, "High"); // impact 50 first
        assert_eq!(next[1].name, "Low");  // impact 10 second
    }

    #[test]
    fn next_phases_excludes_complete_and_deprioritized() {
        let (store, pid) = setup();
        store.create_phase(pid, "Done", 10, &[]).unwrap();
        store.update_phase_status(1, PhaseStatus::Complete).unwrap();
        store.create_phase(pid, "Skip", 20, &[]).unwrap();
        store.update_phase_status(2, PhaseStatus::Deprioritized).unwrap();
        store.create_phase(pid, "Active", 30, &[]).unwrap();

        let dag = DagEngine::new(&store, pid);
        let next = dag.next_phases().unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].name, "Active");
    }

    #[test]
    fn next_phases_unblocks_when_dep_completes() {
        let (store, pid) = setup();
        let p1 = store.create_phase(pid, "Gate", 10, &[]).unwrap();
        let _p2 = store.create_phase(pid, "Blocked", 50, &[p1.id]).unwrap();

        let dag = DagEngine::new(&store, pid);
        let next = dag.next_phases().unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].name, "Gate");

        // Complete the gate
        store.update_phase_status(p1.id, PhaseStatus::Complete).unwrap();
        let next = dag.next_phases().unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].name, "Blocked");
    }

    #[test]
    fn stagnation_detected_after_threshold() {
        let (store, pid) = setup();
        let phase = store.create_phase(pid, "P1", 10, &[]).unwrap();
        
        // 3 consecutive fails
        for i in 0..3 {
            let exp = store.create_experiment(Some(phase.id), &format!("fail_{}", i)).unwrap();
            store.update_experiment_status(exp.id, ExperimentStatus::Fail, Some("failed")).unwrap();
        }

        let dag = DagEngine::new(&store, pid);
        assert_eq!(dag.stagnation_check(3).unwrap(), Some(3));
        assert_eq!(dag.stagnation_check(4).unwrap(), None);
    }

    #[test]
    fn stagnation_resets_on_pass() {
        let (store, pid) = setup();
        let phase = store.create_phase(pid, "P1", 10, &[]).unwrap();
        
        // fail, pass, fail, fail
        let e1 = store.create_experiment(Some(phase.id), "fail_old").unwrap();
        store.update_experiment_status(e1.id, ExperimentStatus::Fail, None).unwrap();
        let e2 = store.create_experiment(Some(phase.id), "pass").unwrap();
        store.update_experiment_status(e2.id, ExperimentStatus::Pass, None).unwrap();
        let e3 = store.create_experiment(Some(phase.id), "fail_1").unwrap();
        store.update_experiment_status(e3.id, ExperimentStatus::Fail, None).unwrap();
        let e4 = store.create_experiment(Some(phase.id), "fail_2").unwrap();
        store.update_experiment_status(e4.id, ExperimentStatus::Fail, None).unwrap();

        let dag = DagEngine::new(&store, pid);
        // Only 2 consecutive fails (after the pass)
        assert_eq!(dag.stagnation_check(3).unwrap(), None);
        assert_eq!(dag.stagnation_check(2).unwrap(), Some(2));
    }
}
