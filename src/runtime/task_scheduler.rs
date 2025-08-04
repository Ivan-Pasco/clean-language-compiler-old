// Task Scheduler Module for Clean Language
// Manages background tasks, scheduling, and execution coordination

use crate::error::CompilerError;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Priority levels for tasks
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// Task execution state
#[derive(Debug, Clone)]
pub enum TaskState {
    Pending,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

/// Represents a scheduled task
#[derive(Debug)]
pub struct ScheduledTask {
    pub id: u32,
    pub name: String,
    pub priority: TaskPriority,
    pub state: TaskState,
    pub created_at: Instant,
    pub started_at: Option<Instant>,
    pub completed_at: Option<Instant>,
    pub dependencies: Vec<u32>, // Task IDs this task depends on
    pub handle: Option<JoinHandle<TaskResult>>,
}

/// Result of a task execution
#[derive(Debug, Clone)]
pub struct TaskResult {
    pub task_id: u32,
    pub success: bool,
    pub value: Option<i32>,
    pub error: Option<String>,
    pub execution_time: Duration,
}

/// Advanced task scheduler with priority queues and dependency management
pub struct TaskScheduler {
    tasks: Arc<Mutex<HashMap<u32, ScheduledTask>>>,
    pending_queues: Arc<Mutex<HashMap<TaskPriority, VecDeque<u32>>>>,
    running_tasks: Arc<Mutex<HashMap<u32, Instant>>>,
    completed_tasks: Arc<Mutex<Vec<TaskResult>>>,
    next_task_id: Arc<Mutex<u32>>,
    max_concurrent_tasks: usize,
    task_sender: mpsc::UnboundedSender<TaskMessage>,
    task_receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<TaskMessage>>>>,
}

/// Messages for task coordination
#[derive(Debug)]
pub enum TaskMessage {
    TaskScheduled { id: u32, priority: TaskPriority },
    TaskStarted { id: u32, started_at: Instant },
    TaskCompleted { id: u32, result: TaskResult },
    TaskFailed { id: u32, error: String },
    TaskCancelled { id: u32 },
    DependencyResolved { task_id: u32, dependency_id: u32 },
}

impl TaskScheduler {
    /// Create a new task scheduler
    pub fn new(max_concurrent_tasks: usize) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();

        let mut pending_queues = HashMap::new();
        pending_queues.insert(TaskPriority::Critical, VecDeque::new());
        pending_queues.insert(TaskPriority::High, VecDeque::new());
        pending_queues.insert(TaskPriority::Normal, VecDeque::new());
        pending_queues.insert(TaskPriority::Low, VecDeque::new());

        TaskScheduler {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            pending_queues: Arc::new(Mutex::new(pending_queues)),
            running_tasks: Arc::new(Mutex::new(HashMap::new())),
            completed_tasks: Arc::new(Mutex::new(Vec::new())),
            next_task_id: Arc::new(Mutex::new(1)),
            max_concurrent_tasks,
            task_sender: sender,
            task_receiver: Arc::new(Mutex::new(Some(receiver))),
        }
    }

    /// Schedule a new task
    pub async fn schedule_task<F, Fut>(
        &self,
        name: String,
        priority: TaskPriority,
        dependencies: Vec<u32>,
        task_fn: F,
    ) -> Result<u32, CompilerError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<i32, String>> + Send + 'static,
    {
        let task_id = {
            let mut id_counter = self.next_task_id.lock().unwrap();
            let id = *id_counter;
            *id_counter += 1;
            id
        };

        // Check if dependencies exist
        {
            let tasks = self.tasks.lock().unwrap();
            for dep_id in &dependencies {
                if !tasks.contains_key(dep_id) {
                    return Err(CompilerError::runtime_error(
                        format!("Dependency task {dep_id} does not exist"),
                        None,
                        None,
                    ));
                }
            }
        }

        let task = ScheduledTask {
            id: task_id,
            name: name.clone(),
            priority,
            state: TaskState::Pending,
            created_at: Instant::now(),
            started_at: None,
            completed_at: None,
            dependencies: dependencies.clone(),
            handle: None,
        };

        // Store the task
        {
            let mut tasks = self.tasks.lock().unwrap();
            tasks.insert(task_id, task);
        }

        // If no dependencies, add to pending queue
        if dependencies.is_empty() {
            self.add_to_pending_queue(task_id, priority).await;
        }

        println!("📋 Scheduled task '{name}' (ID: {task_id}, Priority: {priority:?})");

        // Send scheduling message
        let _ = self.task_sender.send(TaskMessage::TaskScheduled {
            id: task_id,
            priority,
        });

        // Store the task function for later execution
        self.store_task_function(task_id, task_fn).await;

        Ok(task_id)
    }

    /// Add a task to the appropriate pending queue
    async fn add_to_pending_queue(&self, task_id: u32, priority: TaskPriority) {
        let mut queues = self.pending_queues.lock().unwrap();
        if let Some(queue) = queues.get_mut(&priority) {
            queue.push_back(task_id);
        }
    }

    /// Store task function for execution
    async fn store_task_function<F, Fut>(&self, task_id: u32, task_fn: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<i32, String>> + Send + 'static,
    {
        let tasks = Arc::clone(&self.tasks);
        let sender = self.task_sender.clone();

        let handle = tokio::spawn(async move {
            let start_time = Instant::now();

            // Send task started message
            let _ = sender.send(TaskMessage::TaskStarted {
                id: task_id,
                started_at: start_time,
            });

            // Execute the task
            let result = match task_fn().await {
                Ok(value) => TaskResult {
                    task_id,
                    success: true,
                    value: Some(value),
                    error: None,
                    execution_time: start_time.elapsed(),
                },
                Err(error) => TaskResult {
                    task_id,
                    success: false,
                    value: None,
                    error: Some(error.clone()),
                    execution_time: start_time.elapsed(),
                },
            };

            // Update task state
            {
                let mut task_map = tasks.lock().unwrap();
                if let Some(task) = task_map.get_mut(&task_id) {
                    task.completed_at = Some(Instant::now());
                    task.state = if result.success {
                        TaskState::Completed
                    } else {
                        TaskState::Failed(result.error.clone().unwrap_or_default())
                    };
                }
            }

            // Send completion message
            if result.success {
                let _ = sender.send(TaskMessage::TaskCompleted {
                    id: task_id,
                    result: result.clone(),
                });
            } else {
                let _ = sender.send(TaskMessage::TaskFailed {
                    id: task_id,
                    error: result.error.clone().unwrap_or_default(),
                });
            }

            result
        });

        // Store the handle
        {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(&task_id) {
                task.handle = Some(handle);
            }
        }
    }

    /// Execute pending tasks based on priority and dependencies
    pub async fn execute_pending_tasks(&self) -> Result<(), CompilerError> {
        let priorities = [
            TaskPriority::Critical,
            TaskPriority::High,
            TaskPriority::Normal,
            TaskPriority::Low,
        ];

        for priority in priorities {
            while self.can_execute_more_tasks() {
                let task_id = {
                    let mut queues = self.pending_queues.lock().unwrap();
                    if let Some(queue) = queues.get_mut(&priority) {
                        queue.pop_front()
                    } else {
                        None
                    }
                };

                if let Some(id) = task_id {
                    if self.are_dependencies_satisfied(id).await {
                        self.start_task_execution(id).await?;
                    } else {
                        // Put back in queue if dependencies not satisfied
                        let mut queues = self.pending_queues.lock().unwrap();
                        if let Some(queue) = queues.get_mut(&priority) {
                            queue.push_back(id);
                        }
                        break; // Try next priority level
                    }
                } else {
                    break; // No more tasks at this priority
                }
            }
        }

        Ok(())
    }

    /// Check if we can execute more tasks
    fn can_execute_more_tasks(&self) -> bool {
        let running = self.running_tasks.lock().unwrap();
        running.len() < self.max_concurrent_tasks
    }

    /// Check if all dependencies for a task are satisfied
    async fn are_dependencies_satisfied(&self, task_id: u32) -> bool {
        let tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get(&task_id) {
            for dep_id in &task.dependencies {
                if let Some(dep_task) = tasks.get(dep_id) {
                    if !matches!(dep_task.state, TaskState::Completed) {
                        return false;
                    }
                } else {
                    return false; // Dependency doesn't exist
                }
            }
        }
        true
    }

    /// Start executing a task
    async fn start_task_execution(&self, task_id: u32) -> Result<(), CompilerError> {
        {
            let mut running = self.running_tasks.lock().unwrap();
            running.insert(task_id, Instant::now());
        }

        {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(&task_id) {
                task.state = TaskState::Running;
                task.started_at = Some(Instant::now());
                println!("🚀 Starting task '{}' (ID: {})", task.name, task.id);
            }
        }

        Ok(())
    }

    /// Wait for all tasks to complete
    pub async fn wait_for_all_tasks(&self) -> Result<Vec<TaskResult>, CompilerError> {
        // First, execute all pending tasks
        self.execute_pending_tasks().await?;

        // Then wait for running tasks to complete
        let receiver = {
            let mut recv_opt = self.task_receiver.lock().unwrap();
            recv_opt.take()
        };

        if let Some(mut recv) = receiver {
            loop {
                let running_count = {
                    let running = self.running_tasks.lock().unwrap();
                    running.len()
                };

                if running_count == 0 {
                    break;
                }

                match recv.recv().await {
                    Some(TaskMessage::TaskCompleted { id, result }) => {
                        println!("✅ Task {id} completed successfully");
                        {
                            let mut running = self.running_tasks.lock().unwrap();
                            running.remove(&id);
                        }
                        {
                            let mut completed = self.completed_tasks.lock().unwrap();
                            completed.push(result);
                        }

                        // Check if this completion unblocks other tasks
                        self.resolve_dependencies(id).await;
                    }
                    Some(TaskMessage::TaskFailed { id, error }) => {
                        println!("❌ Task {id} failed: {error}");
                        {
                            let mut running = self.running_tasks.lock().unwrap();
                            running.remove(&id);
                        }
                    }
                    Some(TaskMessage::TaskStarted { id, started_at: _ }) => {
                        println!("🔄 Task {id} started execution");
                    }
                    Some(_) => {
                        // Handle other message types
                    }
                    None => break,
                }
            }

            // Put the receiver back
            let mut recv_opt = self.task_receiver.lock().unwrap();
            *recv_opt = Some(recv);
        }

        let completed = self.completed_tasks.lock().unwrap();
        Ok(completed.clone())
    }

    /// Resolve dependencies when a task completes
    async fn resolve_dependencies(&self, completed_task_id: u32) {
        let task_ids_to_check: Vec<u32> = {
            let tasks = self.tasks.lock().unwrap();
            tasks.keys().cloned().collect()
        };

        for task_id in task_ids_to_check {
            let should_schedule = {
                let tasks = self.tasks.lock().unwrap();
                if let Some(task) = tasks.get(&task_id) {
                    matches!(task.state, TaskState::Pending)
                        && task.dependencies.contains(&completed_task_id)
                } else {
                    false
                }
            };

            if should_schedule && self.are_dependencies_satisfied(task_id).await {
                let priority = {
                    let tasks = self.tasks.lock().unwrap();
                    tasks
                        .get(&task_id)
                        .map(|t| t.priority)
                        .unwrap_or(TaskPriority::Normal)
                };

                self.add_to_pending_queue(task_id, priority).await;
                println!("🔓 Task {task_id} dependencies resolved, added to pending queue");
            }
        }
    }

    /// Cancel a task
    pub async fn cancel_task(&self, task_id: u32) -> Result<(), CompilerError> {
        {
            let mut tasks = self.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(&task_id) {
                match task.state {
                    TaskState::Pending => {
                        task.state = TaskState::Cancelled;
                        println!("🚫 Cancelled pending task {task_id} '{}'", task.name);
                    }
                    TaskState::Running => {
                        if let Some(handle) = &task.handle {
                            handle.abort();
                        }
                        task.state = TaskState::Cancelled;
                        println!("🚫 Cancelled running task {task_id} '{}'", task.name);
                    }
                    _ => {
                        return Err(CompilerError::runtime_error(
                            format!("Cannot cancel task {task_id} in state {:?}", task.state),
                            None,
                            None,
                        ));
                    }
                }
            } else {
                return Err(CompilerError::runtime_error(
                    format!("Task {task_id} not found"),
                    None,
                    None,
                ));
            }
        }

        let _ = self
            .task_sender
            .send(TaskMessage::TaskCancelled { id: task_id });
        Ok(())
    }

    /// Get task statistics
    pub fn get_statistics(&self) -> TaskStatistics {
        let tasks = self.tasks.lock().unwrap();
        let running = self.running_tasks.lock().unwrap();
        let completed = self.completed_tasks.lock().unwrap();

        let mut stats = TaskStatistics {
            total_tasks: tasks.len(),
            pending_tasks: 0,
            running_tasks: running.len(),
            completed_tasks: 0,
            failed_tasks: 0,
            cancelled_tasks: 0,
            average_execution_time: Duration::from_secs(0),
        };

        let mut total_execution_time = Duration::from_secs(0);
        let mut execution_count = 0;

        for task in tasks.values() {
            match task.state {
                TaskState::Pending => stats.pending_tasks += 1,
                TaskState::Completed => stats.completed_tasks += 1,
                TaskState::Failed(_) => stats.failed_tasks += 1,
                TaskState::Cancelled => stats.cancelled_tasks += 1,
                TaskState::Running => {} // Already counted
            }
        }

        for result in completed.iter() {
            total_execution_time += result.execution_time;
            execution_count += 1;
        }

        if execution_count > 0 {
            stats.average_execution_time = total_execution_time / execution_count as u32;
        }

        stats
    }
}

/// Task execution statistics
#[derive(Debug, Clone)]
pub struct TaskStatistics {
    pub total_tasks: usize,
    pub pending_tasks: usize,
    pub running_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub cancelled_tasks: usize,
    pub average_execution_time: Duration,
}

impl std::fmt::Display for TaskStatistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Task Statistics:\n\
             Total: {}, Pending: {}, Running: {}, Completed: {}, Failed: {}, Cancelled: {}\n\
             Average execution time: {:?}",
            self.total_tasks,
            self.pending_tasks,
            self.running_tasks,
            self.completed_tasks,
            self.failed_tasks,
            self.cancelled_tasks,
            self.average_execution_time
        )
    }
}
