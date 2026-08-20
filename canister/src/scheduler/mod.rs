use crate::Runtime;
use crate::Timestamp;
use crate::execute::ExecutionStatus;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::Duration;

#[cfg(test)]
mod tests;

pub const MATCHING_INTERVAL: Duration = Duration::from_mins(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskType {
    ProcessPendingOrders,
}

impl TaskType {
    /// The task's heartbeat: the dispatcher re-arms the task at this interval
    /// on every run. `None` for one-shot tasks.
    fn interval(self) -> Option<Duration> {
        match self {
            TaskType::ProcessPendingOrders => Some(MATCHING_INTERVAL),
        }
    }
}

#[derive(Default)]
pub struct Scheduler {
    deadlines: BTreeMap<TaskType, Timestamp>,
}

impl Scheduler {
    pub fn schedule_at(&mut self, task: TaskType, execute_at: Timestamp) -> Timestamp {
        let entry = self.deadlines.entry(task).or_insert(execute_at);
        if execute_at < *entry {
            *entry = execute_at;
        }
        self.next_deadline()
            .expect("BUG: queue must be non-empty after schedule_at")
    }

    pub fn pop_if_ready(&mut self, now: Timestamp) -> Option<TaskType> {
        let task = self
            .deadlines
            .iter()
            .find(|&(_, deadline)| *deadline <= now)
            .map(|(&task, _)| task)?;
        self.deadlines.remove(&task);
        Some(task)
    }

    pub fn next_deadline(&self) -> Option<Timestamp> {
        self.deadlines.values().copied().min()
    }
}

thread_local! {
    static SCHEDULER: RefCell<Scheduler> = RefCell::default();
}

pub fn schedule_now(task: TaskType, runtime: &impl Runtime) {
    schedule_after(Duration::ZERO, task, runtime);
}

pub fn schedule_after(delay: Duration, task: TaskType, runtime: &impl Runtime) {
    let execute_at = runtime.time().saturating_add(delay);
    let next = SCHEDULER.with(|s| s.borrow_mut().schedule_at(task, execute_at));
    runtime.global_timer_set(next);
}

pub fn run_task_if_ready(runtime: &impl Runtime) {
    let now = runtime.time();
    let task = SCHEDULER.with(|s| s.borrow_mut().pop_if_ready(now));
    match task {
        Some(task) => {
            if let Some(interval) = task.interval() {
                schedule_after(interval, task, runtime);
            }
            run_task(task, runtime);
        }
        None => {
            if let Some(next) = SCHEDULER.with(|s| s.borrow().next_deadline()) {
                runtime.global_timer_set(next);
            }
        }
    }
}

fn run_task(task: TaskType, runtime: &impl Runtime) {
    match task {
        TaskType::ProcessPendingOrders => {
            if let ExecutionStatus::MoreWork = crate::process_pending_orders(runtime) {
                schedule_now(TaskType::ProcessPendingOrders, runtime);
            }
        }
    }
}
