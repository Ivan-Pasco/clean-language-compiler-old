// Module for plugin implementations
pub mod console;
pub mod list;
pub mod math;
pub mod memory;
pub mod string;

// Re-export plugins
pub use console::ConsolePlugin;
pub use list::ListPlugin;
pub use math::MathPlugin;
pub use memory::MemoryPlugin;
pub use string::StringPlugin;
