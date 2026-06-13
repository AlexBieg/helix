use std::path::PathBuf;

use notify::{Event, RecursiveMode, Watcher};
use tokio::sync::mpsc;

pub struct FileWatcher {
    _watcher: notify::RecommendedWatcher,
}

impl FileWatcher {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<PathBuf>) {
        let (events_tx, events_rx) = mpsc::unbounded_channel();

        let watcher =
            notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
                Ok(event) => {
                    if event.kind.is_modify() {
                        for path in event.paths {
                            let _ = events_tx.send(path);
                        }
                    }
                }
                Err(e) => {
                    log::error!("file watcher error: {:?}", e);
                }
            })
            .expect("failed to create file watcher");

        (FileWatcher { _watcher: watcher }, events_rx)
    }

    pub fn watch(&mut self, path: &std::path::Path) -> Result<(), notify::Error> {
        self._watcher.watch(path, RecursiveMode::NonRecursive)
    }

    pub fn watch_dir(&mut self, path: &std::path::Path) -> Result<(), notify::Error> {
        self._watcher.watch(path, RecursiveMode::Recursive)
    }

    pub fn unwatch(&mut self, path: &std::path::Path) -> Result<(), notify::Error> {
        self._watcher.unwatch(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;

    /// Helper to wait for a file modification event with a poll loop.
    /// This avoids tokio::time::timeout which can race with FSEvents latency.
    async fn poll_for_event(
        rx: &mut mpsc::UnboundedReceiver<PathBuf>,
        max_wait: Duration,
    ) -> Option<PathBuf> {
        let poll_interval = Duration::from_millis(100);
        let start = std::time::Instant::now();
        loop {
            match rx.try_recv() {
                Ok(path) => return Some(path),
                Err(mpsc::error::TryRecvError::Empty) => {
                    if start.elapsed() >= max_wait {
                        return None;
                    }
                    tokio::time::sleep(poll_interval).await;
                }
                Err(mpsc::error::TryRecvError::Disconnected) => return None,
            }
        }
    }

    #[tokio::test]
    async fn test_file_watcher_detects_write() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.as_file_mut().write_all(b"hello").unwrap();
        file.as_file_mut().flush().unwrap();

        let (mut watcher, mut rx) = FileWatcher::new();
        watcher.watch(file.path()).unwrap();

        // Allow watcher to initialize (FSEvents on macOS needs time to start)
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Write to the file externally
        std::fs::write(file.path(), "hello modified").unwrap();

        // FSEvents on macOS may take up to ~2 seconds to report
        let result = poll_for_event(&mut rx, Duration::from_secs(5)).await;
        assert!(result.is_some(), "expected a file modification event");
    }

    #[tokio::test]
    async fn test_file_watcher_no_event_without_watch() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.as_file_mut().write_all(b"hello").unwrap();
        file.as_file_mut().flush().unwrap();

        let (_watcher, mut rx) = FileWatcher::new();

        std::fs::write(file.path(), "hello modified").unwrap();

        let result = poll_for_event(&mut rx, Duration::from_secs(2)).await;
        assert!(
            result.is_none(),
            "should not receive event for unwatched file"
        );
    }

    #[tokio::test]
    async fn test_file_watcher_unwatch_stops_events() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.as_file_mut().write_all(b"hello").unwrap();
        file.as_file_mut().flush().unwrap();

        let (mut watcher, mut rx) = FileWatcher::new();
        watcher.watch(file.path()).unwrap();

        // Drain any initial events
        while rx.try_recv().is_ok() {}
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Unwatch
        watcher.unwatch(file.path()).unwrap();

        // Write again
        std::fs::write(file.path(), "hello modified").unwrap();

        let result = poll_for_event(&mut rx, Duration::from_secs(2)).await;
        assert!(result.is_none(), "should not receive event after unwatch");
    }
}
