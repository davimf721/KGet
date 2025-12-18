// Shared types used by GUI and the worker thread. Kept separate so the
// binary can compile even when GUI feature is disabled.
use std::fmt;

/// Command sent from the GUI to the worker thread
#[derive(Debug, Clone)]
pub enum DownloadCommand {
    Start(String, String), // url, output_path
    Cancel,
}

/// Message sent from the worker thread back to the GUI
#[derive(Debug, Clone)]
pub enum WorkerToGuiMessage {
    Progress(f32),          // Progress value from 0.0 to 1.0
    StatusUpdate(String),   // General status message
    Completed(String),      // Successful completion message
    Error(String),          // Error message
}

impl fmt::Display for WorkerToGuiMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkerToGuiMessage::Progress(p) => write!(f, "Progress({:.2})", p),
            WorkerToGuiMessage::StatusUpdate(s) => write!(f, "Status: {}", s),
            WorkerToGuiMessage::Completed(s) => write!(f, "Completed: {}", s),
            WorkerToGuiMessage::Error(e) => write!(f, "Error: {}", e),
        }
    }
}
