use crate::Task;
use crate::order::TokenId;
use crate::state::with_state_mut;
use candid::Principal;

#[cfg(test)]
mod tests;

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

/// RAII guard that serializes async deposit/withdraw operations per
/// `(caller, token)`. While a guard is alive, [`UserOpGuard::new`] returns
/// `None` for the same key; on `Drop` the entry is released.
#[derive(Eq, PartialEq, Debug)]
pub struct UserOpGuard {
    key: (Principal, TokenId),
}

impl UserOpGuard {
    pub fn new(caller: Principal, token: TokenId) -> Option<Self> {
        let key = (caller, token);
        with_state_mut(|s| {
            if !s.in_flight_user_ops_mut().insert(key) {
                return None;
            }
            Some(Self { key })
        })
    }
}

impl Drop for UserOpGuard {
    fn drop(&mut self) {
        with_state_mut(|s| {
            s.in_flight_user_ops_mut().remove(&self.key);
        });
    }
}
