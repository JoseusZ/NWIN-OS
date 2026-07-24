// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

use alloc::collections::{BTreeMap, VecDeque};
use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::task::{PrivilegeLevel, Task, TaskId};

// Centralised owner of every task, its state, the ready queue and the
// current-task pointer. The single instance lives in the
// [`TASK_MANAGER`] static and is mutated only through
// [`with_task_manager`] (which disables interrupts around the lock).

/// Logical lifecycle state of a task (Linux-style).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Ready to run on the CPU.
    Ready,
    /// Currently executing.
    Running,
    /// Sleeping, waiting on a mutex, I/O or event.
    Blocked,
    /// Terminated and waiting for the reaper to reclaim it.
    Dead,
}

/// Centralised task manager of the kernel.
pub struct TaskManager {
    /// FIFO queue of tasks in the [`TaskState::Ready`] state.
    pub ready_queue: VecDeque<TaskId>,

    /// Authoritative registry of every task and its metadata.
    pub task_registry: BTreeMap<TaskId, Task>,

    /// Per-task state map (the "semaphore").
    pub task_states: BTreeMap<TaskId, TaskState>,

    /// Task currently executing on the CPU (Ring 0).
    pub current_task: Option<TaskId>,

    /// Master tick counter of the system (uptime).
    pub ticks: AtomicU64,
}

impl TaskManager {
    /// `const` constructor used to initialise the [`TASK_MANAGER`]
    /// static without early dynamic allocation.
    pub const fn empty() -> Self {
        Self {
            ready_queue: VecDeque::new(),
            task_registry: BTreeMap::new(),
            task_states: BTreeMap::new(),
            current_task: None,
            ticks: AtomicU64::new(0),
        }
    }

    /// Appends a task to the back of the ready queue (idempotent).
    pub fn mark_ready(&mut self, id: TaskId) {
        if !self.ready_queue.contains(&id) {
            self.ready_queue.push_back(id);
        }
    }

    /// Pops the next task from the front of the ready queue.
    pub fn fetch_next_task(&mut self) -> Option<TaskId> {
        self.ready_queue.pop_front()
    }

    /// Creates a new task and immediately marks it as [`TaskState::Ready`].
    ///
    /// The current task becomes the parent (lineage tracking).
    pub fn spawn(
        &mut self,
        entry_point: fn() -> !,
        pml4_frame: x86_64::structures::paging::PhysFrame,
        privilege: PrivilegeLevel,
    ) -> Result<TaskId, crate::core::error::KernelError> {

        let parent_id = self.current_task;

        let task = Task::new(
            entry_point,
            pml4_frame,
            privilege,
            0,
            parent_id,
        )?;

        let task_id = task.id;
        self.task_registry.insert(task_id, task);
        self.set_task_state(task_id, TaskState::Ready);

        Ok(task_id)
    }

    /// Blocks the current task until `child_id` reaches [`TaskState::Dead`].
    ///
    /// Returns `true` if the parent was successfully parked, `false`
    /// if the child was already dead or does not exist.
    pub fn wait_for_child(&mut self, child_id: TaskId) -> bool {
        if let Some(&state) = self.task_states.get(&child_id) {
            if state != TaskState::Dead {
                self.block_current_task();
                return true; // parent slept successfully
            }
        }
        false // child already dead or does not exist
    }

    /// Linux-style "semaphore" transition: updates the task state and
    /// keeps the ready queue in sync (enqueue on Ready, dequeue on
    /// any other state).
    pub fn set_task_state(&mut self, task_id: TaskId, state: TaskState) {
        let old_state = self.task_states.insert(task_id, state).unwrap_or(TaskState::Dead);

        if state == TaskState::Ready && old_state != TaskState::Ready {
            self.ready_queue.push_back(task_id);
        } else if state != TaskState::Ready && old_state == TaskState::Ready {
            self.ready_queue.retain(|&id| id != task_id);
        }
    }

    /// AUDIT NOTE (BUG 3): this legacy function mixed the read of
    /// `current_task` with its mutation in a dangerous way. It is kept
    /// for compatibility, but callers should delegate fine-grained
    /// state control to `scheduler.rs` via [`fetch_next_task`].
    pub fn next_task(&mut self) -> Option<TaskId> {
        if let Some(prev_id) = self.current_task {
            if let Some(&state) = self.task_states.get(&prev_id) {
                if state == TaskState::Running {
                    self.set_task_state(prev_id, TaskState::Ready);
                }
            }
        }

        if let Some(next_id) = self.ready_queue.pop_front() {
            self.task_states.insert(next_id, TaskState::Running);
            self.current_task = Some(next_id);
            Some(next_id)
        } else {
            self.current_task = None;
            None
        }
    }

    /// Blocks the current task. The scheduler will skip it on the next cycle.
    pub fn block_current_task(&mut self) {
        if let Some(id) = self.current_task {
            self.set_task_state(id, TaskState::Blocked);
        }
    }

    /// Marks the current task as [`TaskState::Dead`] and detaches it
    /// from the CPU so the scheduler is forced to pick a new task on
    /// the next PIT tick.
    pub fn exit_current_task(&mut self) {
        if let Some(id) = self.current_task {
            self.set_task_state(id, TaskState::Dead);
            self.current_task = None;
        }
    }

    /// Wakes a specific task if it is currently [`TaskState::Blocked`].
    pub fn unblock_task(&mut self, task_id: TaskId) {
        if let Some(&state) = self.task_states.get(&task_id) {
            if state == TaskState::Blocked {
                self.set_task_state(task_id, TaskState::Ready);
            }
        }
    }

    /// Returns the saved kernel RSP of `task_id`, if it exists.
    pub fn get_rsp(&self, task_id: TaskId) -> Option<u64> {
        self.task_registry.get(&task_id).map(|t| t.rsp)
    }

    /// Updates the saved kernel RSP of `task_id`, no-op if unknown.
    pub fn set_rsp(&mut self, task_id: TaskId, rsp: u64) {
        if let Some(task) = self.task_registry.get_mut(&task_id) {
            task.rsp = rsp;
        }
    }

    /// Removes a task from the registry. If the killed task had a
    /// parent that was [`TaskState::Blocked`] (typically waiting via
    /// [`wait_for_child`]), the parent is woken up to [`TaskState::Ready`].
    pub fn kill(&mut self, task_id: TaskId) -> Option<Task> {
        self.set_task_state(task_id, TaskState::Dead);

        if let Some(task) = self.task_registry.get(&task_id) {
            if let Some(parent_id) = task.parent_id {
                if let Some(&parent_state) = self.task_states.get(&parent_id) {
                    if parent_state == TaskState::Blocked {
                        self.set_task_state(parent_id, TaskState::Ready);
                    }
                }
            }
        }

        self.task_states.remove(&task_id);
        self.task_registry.remove(&task_id)
    }

    /// Snapshot of high-level metrics (live + ready count, current task,
    /// tick counter).
    pub fn stats(&self) -> TaskManagerStats {
        TaskManagerStats {
            total_tasks: self.task_registry.len(),
            ready_tasks: self.ready_queue.len(),
            current_task: self.current_task,
            ticks: self.ticks.load(Ordering::Relaxed),
        }
    }

    /// Spawns a Ring 3 user task, inheriting the current task as its parent.
    pub fn spawn_dynamic(
        &mut self,
        entry_point: fn() -> !,
        target_pml4: x86_64::structures::paging::PhysFrame,
        user_stack_top: u64,
    ) -> Result<TaskId, crate::core::error::KernelError> {

        let parent_id = self.current_task;

        let task = Task::new(
            entry_point,
            target_pml4,
            PrivilegeLevel::UserMode,
            user_stack_top,
            parent_id,
        )?;

        let task_id = task.id;
        self.task_registry.insert(task_id, task);
        self.set_task_state(task_id, TaskState::Ready);

        Ok(task_id)
    }
}

/// Snapshot of [`TaskManager`] metrics returned by [`TaskManager::stats`].
pub struct TaskManagerStats {
    pub total_tasks: usize,
    pub ready_tasks: usize,
    pub current_task: Option<TaskId>,
    pub ticks: u64,
}

/// Global singleton. Mutated only through [`with_task_manager`].
pub static TASK_MANAGER: Mutex<TaskManager> = Mutex::new(TaskManager::empty());

/// Initialises the static task manager (currently a no-op placeholder).
pub fn init_task_manager() {
    crate::println!("[OK] TaskManager initialised.");
}

/// Acquires the [`TASK_MANAGER`] lock with interrupts disabled, which
/// is the only safe way to mutate task state from any context
/// (including interrupt handlers).
pub fn with_task_manager<F, R>(f: F) -> R
where
    F: FnOnce(&mut TaskManager) -> R,
{
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut tm = TASK_MANAGER.lock();
        f(&mut tm)
    })
}