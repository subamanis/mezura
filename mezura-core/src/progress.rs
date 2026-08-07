use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// The counters a caller can watch from another thread while 'run' blocks. Everything is relaxed:
// the figures are read for display, nothing synchronises through them, and a reader may be a file
// behind at any moment.
#[derive(Debug, Default)]
pub struct ScanProgress {
    files_found: AtomicUsize,
    files_parsed: AtomicUsize,
    lines_counted: AtomicUsize,
    walk_done: AtomicBool
}

impl ScanProgress {
    pub fn get_files_found(&self) -> usize {
        self.files_found.load(Ordering::Relaxed)
    }

    pub fn get_files_parsed(&self) -> usize {
        self.files_parsed.load(Ordering::Relaxed)
    }

    pub fn get_lines_counted(&self) -> usize {
        self.lines_counted.load(Ordering::Relaxed)
    }

    pub fn is_walk_done(&self) -> bool {
        self.walk_done.load(Ordering::Relaxed)
    }

    pub(crate) fn record_file_found(&self) {
        self.files_found.fetch_add(1, Ordering::Relaxed);
    }

    // A file that could not be parsed still moves 'files_parsed', with no lines: the figure is how
    // far the counting has come through the queue, and a bar over it must be able to reach the end.
    pub(crate) fn record_file_parsed(&self, lines: usize) {
        self.files_parsed.fetch_add(1, Ordering::Relaxed);
        self.lines_counted.fetch_add(lines, Ordering::Relaxed);
    }

    pub(crate) fn mark_walk_done(&self) {
        self.walk_done.store(true, Ordering::Relaxed);
    }
}
