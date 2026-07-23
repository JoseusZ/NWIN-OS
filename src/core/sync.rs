// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Task-level synchronisation primitives: counting [`Semaphore`],
//! [`TaskMutex`] (a mutex that actually parks the current task
//! instead of spinning) and [`CondVar`].
//!
//! Every blocking path follows the same protocol:
//!
//! 1. Disable hardware interrupts to prevent scheduler preemption
//!    while the task is being enqueued.
//! 2. Re-evaluate the wakeup condition to rule out a lost wakeup
//!    race.
//! 3. Enqueue the current [`TaskId`] and call
//!    [`crate::task::TaskManager::block_current_task`].
//! 4. Atomically re-enable interrupts and halt the CPU via
//!    [`interrupts::enable_and_hlt`] so the next timer tick can
//!    resume the task.

use core::sync::atomic::{AtomicU32, Ordering};
use core::cell::UnsafeCell;
use spin::Mutex as SpinMutex;
use alloc::collections::VecDeque;
use crate::task::TaskId;
use x86_64::instructions::interrupts;

/// Counting semaphore for inter-task synchronisation.
///
/// Internally holds a permit counter and a FIFO of [`TaskId`]s
/// blocked on `wait`.
pub struct Semaphore {
    count: AtomicU32,
    waiting_tasks: SpinMutex<VecDeque<TaskId>>,
}

impl Semaphore {
    /// Builds a semaphore seeded with `initial_count` permits.
    pub const fn new(initial_count: u32) -> Self {
        Self {
            count: AtomicU32::new(initial_count),
            waiting_tasks: SpinMutex::new(VecDeque::new()),
        }
    }

    /// Decrements the semaphore, blocking the current task until a
    /// permit becomes available.
    ///
    /// The fast path uses a single `compare_exchange`; the slow path
    /// disables interrupts, re-checks the counter (lost-wakeup
    /// fix), enqueues the current [`TaskId`] into
    /// `waiting_tasks` and parks via `enable_and_hlt`.
    pub fn wait(&self) {
        loop {
            let current = self.count.load(Ordering::SeqCst);
            if current > 0 {
                // Fast path: take a permit atomically.
                if self.count.compare_exchange(
                    current,
                    current - 1,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ).is_ok() {
                    return; // permit acquired; exit the loop.
                }
            } else {
                // Disable hardware interrupts to prevent scheduler
                // preemption while the task is being enqueued.
                interrupts::disable();

                // Re-check the count after disabling interrupts to rule
                // out a lost wakeup race: a concurrent `signal` on
                // another task may have incremented the counter between
                // the initial load and `interrupts::disable`.
                let current_retry = self.count.load(Ordering::SeqCst);
                if current_retry > 0 {
                    if self.count.compare_exchange(
                        current_retry,
                        current_retry - 1,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    ).is_ok() {
                        interrupts::enable();
                        return;
                    }
                }

                crate::task::with_task_manager(|tm| {
                    if let Some(current_id) = tm.current_task {
                        self.waiting_tasks.lock().push_back(current_id);
                        tm.block_current_task();
                    }
                });

                // Atomically re-enable interrupts and halt the CPU so
                // the next timer tick can resume the task.
                interrupts::enable_and_hlt();
            }
        }
    }

    /// Increments the semaphore and wakes up one enqueued task.
    pub fn signal(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);

        // Critical section: dequeue + unblock must be atomic with
        // respect to the scheduler to avoid a half-applied wakeup.
        interrupts::without_interrupts(|| {
            if let Some(task_id) = self.waiting_tasks.lock().pop_front() {
                crate::task::with_task_manager(|tm| {
                    tm.unblock_task(task_id);
                });
            }
        });
    }
}

/// Mutex with real blocking semantics (avoids 100% CPU usage).
pub struct TaskMutex<T> {
    data: UnsafeCell<T>,
    semaphore: Semaphore,
}

unsafe impl<T: Send> Send for TaskMutex<T> {}
unsafe impl<T: Send> Sync for TaskMutex<T> {}

impl<T> TaskMutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            semaphore: Semaphore::new(1),
        }
    }

    /// Locks the mutex. It does not need to be told the TaskId, it discovers it automatically.
    pub fn lock(&self) -> TaskMutexGuard<'_, T> {
        self.semaphore.wait();
        TaskMutexGuard { mutex: self }
    }
}

pub struct TaskMutexGuard<'a, T> {
    mutex: &'a TaskMutex<T>,
}

impl<'a, T> Drop for TaskMutexGuard<'a, T> {
    fn drop(&mut self) {
        self.mutex.semaphore.signal();
    }
}

impl<'a, T> core::ops::Deref for TaskMutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T> core::ops::DerefMut for TaskMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
    }
}

/// Condition Variable for complex signalling.
pub struct CondVar {
    waiting_tasks: SpinMutex<VecDeque<TaskId>>,
}

impl CondVar {
    pub const fn new() -> Self {
        Self {
            waiting_tasks: SpinMutex::new(VecDeque::new()),
        }
    }

    /// Atomically releases `guard`, suspends the current task on this
    /// condition variable and re-acquires the original mutex before
    /// returning.
    ///
    /// `guard` MUST be supplied: parking the task while still holding
    /// the lock would deadlock, because no other task could acquire
    /// the mutex to issue the matching `notify`.
    pub fn wait<'a, T>(&self, guard: TaskMutexGuard<'a, T>) -> TaskMutexGuard<'a, T> {
        let mutex = guard.mutex;

        interrupts::disable();

        crate::task::with_task_manager(|tm| {
            if let Some(current_id) = tm.current_task {
                self.waiting_tasks.lock().push_back(current_id);
                tm.block_current_task();
            }
        });

        // Drop the guard (atomic with respect to the scheduler) before
        // halting the CPU, otherwise the lock would still be held
        // while the task is parked.
        drop(guard);

        // Atomically re-enable interrupts and halt the CPU until the
        // next timer tick wakes the task.
        interrupts::enable_and_hlt();

        // On resume, compete for the original mutex again.
        mutex.lock()
    }

    pub fn notify_one(&self) {
        interrupts::without_interrupts(|| {
            if let Some(task_id) = self.waiting_tasks.lock().pop_front() {
                crate::task::with_task_manager(|tm| {
                    tm.unblock_task(task_id);
                });
            }
        });
    }

    pub fn notify_all(&self) {
        interrupts::without_interrupts(|| {
            let mut queue = self.waiting_tasks.lock();
            crate::task::with_task_manager(|tm| {
                while let Some(task_id) = queue.pop_front() {
                    tm.unblock_task(task_id);
                }
            });
        });
    }
}