// SPDX-FileCopyrightText: 2026 NWIN OS
//
// SPDX-License-Identifier: GPL-3.0-or-later

use alloc::collections::{BTreeMap, VecDeque};
use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::task::{PrivilegeLevel, Task, TaskId};

// ========================================================
// ESTADOS LÓGICOS DE UNA TAREA (Estilo Linux)
// ========================================================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Lista para ejecutarse en la CPU
    Ready,
    /// Actualmente en ejecución
    Running,
    /// Durmiendo/Esperando un Mutex, E/S o evento
    Blocked,
    /// Terminada (Zombie/Dead) y esperando a ser limpiada
    Dead,
}

// ========================================================
// GESTOR DE TAREAS (TASK MANAGER)
// ========================================================
/// Gestor centralizado de todas las tareas del kernel
pub struct TaskManager {
    /// Cola de tareas LISTAS para ejecutarse (Solo tareas en estado 'Ready')
    pub ready_queue: VecDeque<TaskId>,
    
    /// Registro centralizado absoluto de todas las tareas y sus metadatos
    pub task_registry: BTreeMap<TaskId, Task>,
    
    /// Estado de cada tarea (El Semáforo)
    pub task_states: BTreeMap<TaskId, TaskState>,
    
    /// Tarea actualmente ejecutándose (Ring 0)
    pub current_task: Option<TaskId>,
    
    /// Contador maestro de tics del sistema (Uptime)
    pub ticks: AtomicU64,
}

impl TaskManager {
    /// Constructor constante para inicializar el STATIC global sin asignación dinámica temprana.
    pub const fn empty() -> Self {
        Self {
            ready_queue: VecDeque::new(),
            task_registry: BTreeMap::new(),
            task_states: BTreeMap::new(),
            current_task: None,
            ticks: AtomicU64::new(0),
        }
    }

    /// Agrega la tarea al final de la cola (Útil para re-encolar hilos vivos)
    pub fn mark_ready(&mut self, id: TaskId) {
        if !self.ready_queue.contains(&id) {
            self.ready_queue.push_back(id);
        }
    }

    /// Extrae la siguiente tarea de la cola de forma atómica (Para el Scheduler)
    pub fn fetch_next_task(&mut self) -> Option<TaskId> {
        self.ready_queue.pop_front()
    }

    /// Crea una nueva tarea y la enciende marcándola como 'Ready'
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
            parent_id, // <-- INYECTAMOS EL LINAJE
        )?; 
        
        let task_id = task.id;
        self.task_registry.insert(task_id, task);
        self.set_task_state(task_id, TaskState::Ready);
        
        Ok(task_id)
    }

    pub fn wait_for_child(&mut self, child_id: TaskId) -> bool {
        if let Some(&state) = self.task_states.get(&child_id) {
            if state != TaskState::Dead {
                self.block_current_task();
                return true; // El padre se durmió con éxito
            }
        }
        false // El hijo ya murió o no existe
    }

    /// Manejador seguro de estados (El Semáforo de Linux)
    pub fn set_task_state(&mut self, task_id: TaskId, state: TaskState) {
        let old_state = self.task_states.insert(task_id, state).unwrap_or(TaskState::Dead);
        
        if state == TaskState::Ready && old_state != TaskState::Ready {
            // Si despierta o nace, la ponemos en la cola de ejecución
            self.ready_queue.push_back(task_id);
        } else if state != TaskState::Ready && old_state == TaskState::Ready {
            // Si se bloquea, la buscamos y la sacamos de la cola de listos
            self.ready_queue.retain(|&id| id != task_id);
        }
    }

    /// NOTA DE AUDITORÍA (BUG 3): Esta función antigua mezclaba la obtención con la mutación
    /// de 'current_task' de forma peligrosa. Se conserva por compatibilidad, pero se recomienda
    /// delegar el control de estados finos directamente al `scheduler.rs` usando `fetch_next_task`.
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

    /// Bloquea la tarea actual. El scheduler la saltará en el próximo ciclo.
    pub fn block_current_task(&mut self) {
        if let Some(id) = self.current_task {
            self.set_task_state(id, TaskState::Blocked);
        }
    }

    pub fn exit_current_task(&mut self) {
        if let Some(id) = self.current_task {
            // 1. Cambiamos el estado a Dead (El semáforo la saca de la cola de Listos)
            self.set_task_state(id, TaskState::Dead);
            
            // 2. Desvinculamos la tarea de la CPU actual para que el Scheduler 
            // se vea obligado a elegir una nueva tarea en el próximo ciclo del PIT.
            self.current_task = None;
        }
    }

    /// Despierta una tarea específica
    pub fn unblock_task(&mut self, task_id: TaskId) {
        if let Some(&state) = self.task_states.get(&task_id) {
            if state == TaskState::Blocked {
                self.set_task_state(task_id, TaskState::Ready);
            }
        }
    }

    // --- MANEJO DE REGISTROS Y LIMPIEZA ---

    pub fn get_rsp(&self, task_id: TaskId) -> Option<u64> {
        self.task_registry.get(&task_id).map(|t| t.rsp)
    }

    pub fn set_rsp(&mut self, task_id: TaskId, rsp: u64) {
        if let Some(task) = self.task_registry.get_mut(&task_id) {
            task.rsp = rsp;
        }
    }

    pub fn kill(&mut self, task_id: TaskId) -> Option<Task> {
        self.set_task_state(task_id, TaskState::Dead);
        
        // --- EL DESPERTADOR IPC ---
        // Si la tarea que estamos matando tiene un padre, y ese padre 
        // está durmiendo (Blocked), lo despertamos pasándolo a Ready.
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

    pub fn stats(&self) -> TaskManagerStats {
        TaskManagerStats {
            total_tasks: self.task_registry.len(),
            ready_tasks: self.ready_queue.len(),
            current_task: self.current_task,
            ticks: self.ticks.load(Ordering::Relaxed),
        }
    }

    pub fn spawn_dynamic(
        &mut self,
        entry_point: fn() -> !,
        target_pml4: x86_64::structures::paging::PhysFrame,
        user_stack_top: u64,
    ) -> Result<TaskId, crate::core::error::KernelError> { 
        
        let parent_id = self.current_task; // <-- CAPTURAMOS AL PADRE (Ej. La Shell)

        let task = Task::new(
            entry_point,
            target_pml4,
            PrivilegeLevel::UserMode,
            user_stack_top,
            parent_id, // <-- INYECTAMOS EL LINAJE
        )?; 
        
        let task_id = task.id;
        self.task_registry.insert(task_id, task);
        self.set_task_state(task_id, TaskState::Ready);
        
        Ok(task_id) 
    }
}
// --- ESTRUCTURAS GLOBALES Y PÚBLICAS ---

pub struct TaskManagerStats {
    pub total_tasks: usize,
    pub ready_tasks: usize,
    pub current_task: Option<TaskId>,
    pub ticks: u64,
}

pub static TASK_MANAGER: Mutex<TaskManager> = Mutex::new(TaskManager::empty());

pub fn init_task_manager() {
    crate::println!("[OK] TaskManager inicializado.");
}

pub fn with_task_manager<F, R>(f: F) -> R
where
    F: FnOnce(&mut TaskManager) -> R,
{
   
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut tm = TASK_MANAGER.lock();
        f(&mut tm)
    })
}