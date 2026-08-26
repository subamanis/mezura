use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// How far a run has got, for a caller that needs real time feedback.
///
/// Hand one to [`crate::run_watched`] and read it from another thread. The counters may be a file
/// behind at any moment, since they are meant for a display and not for arithmetic.
// The counters are relaxed. The flag is release against acquire, so a reader who sees the walk
// finished also sees every file the walk found. Only a run that returns 'Ok' promises 'files_found'
// is final at that moment: one that panics out raises the flag on its way with the walk still moving.
#[derive(Debug, Default)]
pub struct ScanProgress {
    files_found: AtomicUsize,
    files_parsed: AtomicUsize,
    lines_counted: AtomicUsize,
    walk_done: AtomicBool
}

impl ScanProgress {
    /// Files the scan has queued for counting so far. Final once the walk is done, on a run that
    /// goes on to return `Ok`.
    pub fn get_files_found(&self) -> usize {
        self.files_found.load(Ordering::Relaxed)
    }

    /// How many of them have been read. A file that could not be parsed moves this too, so a bar
    /// drawn over the pair can reach the end.
    pub fn get_files_parsed(&self) -> usize {
        self.files_parsed.load(Ordering::Relaxed)
    }

    /// Their lines so far.
    pub fn get_lines_counted(&self) -> usize {
        self.lines_counted.load(Ordering::Relaxed)
    }

    /// Whether the directories have all been scanned. It also rises on a run that ends in an error.
    pub fn is_walk_done(&self) -> bool {
        self.walk_done.load(Ordering::Acquire)
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
        self.walk_done.store(true, Ordering::Release);
    }
}
