// Real File I/O Implementation for Clean Language
// Replaces mock/placeholder file operations with actual filesystem interactions

use crate::error::CompilerError;
use std::fs;
use std::path::Path;

/// File I/O operations manager
pub struct FileIO;

impl FileIO {
    /// Read file contents as string
    pub fn read_file(path: &str) -> Result<String, CompilerError> {
        println!("📁 [FILE READ] Reading file: {path}");

        match fs::read_to_string(path) {
            Ok(content) => {
                println!(
                    "✅ [FILE READ] Successfully read {} bytes from {path}",
                    content.len()
                );
                Ok(content)
            }
            Err(e) => {
                let error_msg = format!("Failed to read file '{path}': {e}");
                println!("❌ [FILE READ] {error_msg}");
                Err(CompilerError::runtime_error(error_msg, None, None))
            }
        }
    }

    /// Write content to file
    pub fn write_file(path: &str, content: &str) -> Result<(), CompilerError> {
        println!(
            "📁 [FILE WRITE] Writing {len} bytes to: {path}",
            len = content.len()
        );

        match fs::write(path, content) {
            Ok(()) => {
                println!("✅ [FILE WRITE] Successfully wrote to {path}");
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Failed to write file '{path}': {e}");
                println!("❌ [FILE WRITE] {error_msg}");
                Err(CompilerError::runtime_error(error_msg, None, None))
            }
        }
    }

    /// Append content to file
    pub fn append_file(path: &str, content: &str) -> Result<(), CompilerError> {
        println!(
            "📁 [FILE APPEND] Appending {} bytes to: {path}",
            content.len()
        );

        use std::fs::OpenOptions;
        use std::io::Write;

        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| file.write_all(content.as_bytes()))
        {
            Ok(()) => {
                println!("✅ [FILE APPEND] Successfully appended to {path}");
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Failed to append to file '{path}': {e}");
                println!("❌ [FILE APPEND] {error_msg}");
                Err(CompilerError::runtime_error(error_msg, None, None))
            }
        }
    }

    /// Check if file exists
    pub fn file_exists(path: &str) -> bool {
        let exists = Path::new(path).exists();
        println!("📁 [FILE EXISTS] File '{path}' exists: {exists}");
        exists
    }

    /// Delete file
    pub fn delete_file(path: &str) -> Result<(), CompilerError> {
        println!("📁 [FILE DELETE] Deleting file: {path}");

        match fs::remove_file(path) {
            Ok(()) => {
                println!("✅ [FILE DELETE] Successfully deleted {path}");
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Failed to delete file '{path}': {e}");
                println!("❌ [FILE DELETE] {error_msg}");
                Err(CompilerError::runtime_error(error_msg, None, None))
            }
        }
    }

    /// Get file size in bytes
    pub fn file_size(path: &str) -> Result<u64, CompilerError> {
        println!("📁 [FILE SIZE] Getting size of: {path}");

        match fs::metadata(path) {
            Ok(metadata) => {
                let size = metadata.len();
                println!("✅ [FILE SIZE] File '{path}' is {size} bytes");
                Ok(size)
            }
            Err(e) => {
                let error_msg = format!("Failed to get size of file '{path}': {e}");
                println!("❌ [FILE SIZE] {error_msg}");
                Err(CompilerError::runtime_error(error_msg, None, None))
            }
        }
    }

    /// List directory contents
    pub fn list_directory(path: &str) -> Result<Vec<String>, CompilerError> {
        println!("📁 [DIR LIST] Listing directory: {path}");

        match fs::read_dir(path) {
            Ok(entries) => {
                let mut files = Vec::new();
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            if let Some(name) = entry.file_name().to_str() {
                                files.push(name.to_string());
                            }
                        }
                        Err(e) => {
                            println!("⚠️  [DIR LIST] Error reading entry: {e}");
                        }
                    }
                }

                println!(
                    "✅ [DIR LIST] Found {count} entries in {path}",
                    count = files.len()
                );
                Ok(files)
            }
            Err(e) => {
                let error_msg = format!("Failed to list directory '{path}': {e}");
                println!("❌ [DIR LIST] {error_msg}");
                Err(CompilerError::runtime_error(error_msg, None, None))
            }
        }
    }

    /// Create directory
    pub fn create_directory(path: &str) -> Result<(), CompilerError> {
        println!("📁 [DIR CREATE] Creating directory: {path}");

        match fs::create_dir_all(path) {
            Ok(()) => {
                println!("✅ [DIR CREATE] Successfully created directory {path}");
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Failed to create directory '{path}': {e}");
                println!("❌ [DIR CREATE] {error_msg}");
                Err(CompilerError::runtime_error(error_msg, None, None))
            }
        }
    }
}
