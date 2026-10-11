//! where the transcripts of a project live: the folder, a fresh file per run,
//! the latest one to resume, and the whole list the memory box shows.

use crate::settings;
use std::path::{Path, PathBuf};

/// Locates the folder where this project's saved sessions live.
fn sessions_dir(root: &Path) -> PathBuf {
    root.join(settings::PROJECT_DIR).join("sessions")
}

/// Picks a fresh session file named by the time and the process, so sessions sort
/// and two runs never share a file.
pub(crate) fn new_session_path(root: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    sessions_dir(root).join(format!("{stamp}-{}.jsonl", std::process::id()))
}

/// the most recent transcript of this project.
pub fn latest_session(root: &Path) -> Option<PathBuf> {
    let dir = sessions_dir(root);
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .collect();
    all.sort();
    all.pop()
}

/// every transcript, newest first — what the memory box lists.
pub fn sessions(root: &Path) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = std::fs::read_dir(sessions_dir(root))
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    all.retain(|p| p.extension().and_then(|x| x.to_str()) == Some("jsonl"));
    all.sort();
    all.reverse();
    all
}
