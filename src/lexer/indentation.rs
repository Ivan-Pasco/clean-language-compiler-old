//! Indentation tracking for Clean Language lexer

/// Tracks tab-based indentation levels
#[derive(Debug, Clone)]
pub struct IndentationTracker {
    levels: Vec<usize>,
    current_level: usize,
}

impl IndentationTracker {
    pub fn new() -> Self {
        Self {
            levels: vec![0], // Start with base indentation level
            current_level: 0,
        }
    }

    /// Process a line's indentation and return indent/dedent tokens needed
    pub fn process_indentation(&mut self, tab_count: usize) -> Vec<IndentationChange> {
        let mut changes = Vec::new();

        if tab_count > self.current_level {
            // Increase indentation
            self.levels.push(tab_count);
            self.current_level = tab_count;
            changes.push(IndentationChange::Indent);
        } else if tab_count < self.current_level {
            // Decrease indentation - might need multiple dedents
            while let Some(&level) = self.levels.last() {
                if level <= tab_count {
                    break;
                }
                self.levels.pop();
                changes.push(IndentationChange::Dedent);
            }

            if self.levels.last() != Some(&tab_count) {
                // Indentation doesn't match any previous level
                changes.push(IndentationChange::IndentationError);
            }

            self.current_level = tab_count;
        }
        // If tab_count == current_level, no change needed

        changes
    }

    /// Get current indentation level
    pub fn current_level(&self) -> usize {
        self.current_level
    }

    /// Reset to base level (for new files or error recovery)
    pub fn reset(&mut self) {
        self.levels = vec![0];
        self.current_level = 0;
    }
}

/// Indentation change events
#[derive(Debug, Clone, PartialEq)]
pub enum IndentationChange {
    Indent,
    Dedent,
    IndentationError,
}

impl Default for IndentationTracker {
    fn default() -> Self {
        Self::new()
    }
}
