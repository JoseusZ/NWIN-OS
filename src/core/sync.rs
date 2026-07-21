use core::sync::atomic::{AtomicU32, Ordering};
use core::cell::UnsafeCell;
use spin::Mutex as SpinMutex;
use alloc::collections::VecDeque;
use crate::task::TaskId;
use x86_64::instructions::interrupts;

/// Semáforo para sincronización entre tareas
pub struct Semaphore {
    count: AtomicU32,
    waiting_tasks: SpinMutex<VecDeque<TaskId>>,
}

impl Semaphore {
    pub const fn new(initial_count: u32) -> Self {
        Self {
            count: AtomicU32::new(initial_count),
            waiting_tasks: SpinMutex::new(VecDeque::new()),
        }
    }

    /// Intenta decrementar el semáforo. Si está ocupado, cede la CPU de forma segura.
    pub fn wait(&self) {
        loop {
            let current = self.count.load(Ordering::SeqCst);
            if current > 0 {
                // Intentamos tomar el candado atómicamente
                if self.count.compare_exchange(
                    current,
                    current - 1,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ).is_ok() {
                    return; // ¡Éxito! Tenemos el candado, salimos del bucle.
                }
            } else {
                // --- CORRECCIÓN CRÍTICA: Bloqueo Atómico Anti-Deadlock ---
                // Deshabilitamos las interrupciones de hardware para garantizar que 
                // el scheduler no nos quite la CPU en medio del proceso de bloqueo.
                interrupts::disable();

                // === CORRECCIÓN 1: SOLUCIÓN AL LOST WAKEUP ===
                // Si ocurrió un `signal()` en otra tarea justo antes de llamar a `interrupts::disable()`,
                // el contador habrá subido a > 0. Debemos re-verificarlo aquí (con interrupciones 
                // ya congeladas) para evitar bloquearnos permanentemente por perder un aviso legítimo.
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

                // enable_and_hlt() ejecuta "sti" y "hlt" como una operación atómica indivisible.
                // Asegura que no nos perdamos la interrupción del reloj que nos despertará.
                interrupts::enable_and_hlt();
            }
        }
    }

    /// Incrementa el semáforo y despierta a una tarea encolada
    pub fn signal(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
        
        // Protegemos la notificación para que no sea interrumpida a medias
        interrupts::without_interrupts(|| {
            if let Some(task_id) = self.waiting_tasks.lock().pop_front() {
                crate::task::with_task_manager(|tm| {
                    tm.unblock_task(task_id);
                });
            }
        });
    }
}

/// Mutex con semántica real de bloqueo (Evita el 100% de uso de CPU)
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

    /// Bloquea el mutex. No necesita pedir el TaskId, lo averigua automáticamente.
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
        // Al salir de alcance, liberamos el candado automáticamente
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

/// Condition Variable para señalización compleja
pub struct CondVar {
    waiting_tasks: SpinMutex<VecDeque<TaskId>>,
}

impl CondVar {
    pub const fn new() -> Self {
        Self {
            waiting_tasks: SpinMutex::new(VecDeque::new()),
        }
    }

    // === CORRECCIÓN 2: SOLUCIÓN AL DEADLOCK DE CONDVAR Y FIRMA CORRECTA ===
    // Una variable de condición real DEBE recibir el `TaskMutexGuard` que protege la condición.
    // De lo contrario, la tarea se iría a dormir reteniendo el cerrojo, provocando un
    // deadlock inmediato ya que ninguna otra tarea podría adquirir el mutex para lanzar el `notify`.
    pub fn wait<'a, T>(&self, guard: TaskMutexGuard<'a, T>) -> TaskMutexGuard<'a, T> {
        let mutex = guard.mutex;

        interrupts::disable();

        crate::task::with_task_manager(|tm| {
            if let Some(current_id) = tm.current_task {
                self.waiting_tasks.lock().push_back(current_id);
                tm.block_current_task();
            }
        });

        // Liberamos el mutex atómicamente antes de suspender la CPU (invoca a Drop)
        drop(guard);

        // Habilitamos interrupciones y enviamos la CPU a dormir de forma segura
        interrupts::enable_and_hlt();

        // Al despertar, la tarea está obligada a competir y readquirir el mutex original
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