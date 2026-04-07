use crate::Task;
use crate::state::with_state_mut;

#[derive(Eq, PartialEq, Debug)]
pub struct TimerGuard {
    task: Task,
}

impl TimerGuard {
    pub fn new(task: Task) -> Option<Self> {
        with_state_mut(|s| {
            if !s.active_tasks_mut().insert(task) {
                return None;
            }
            Some(Self { task })
        })
    }
}

impl Drop for TimerGuard {
    fn drop(&mut self) {
        with_state_mut(|s| {
            s.active_tasks_mut().remove(&self.task);
        });
    }
}
