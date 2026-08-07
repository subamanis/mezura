use std::io::{IsTerminal, Write};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mezura_core::ScanProgress;

use crate::config_manager::Configuration;

const WALK_HEADING : &str = "Analyzing directories";
// Like cargo's delayed bar: a run that ends before this never shows motion at all
const MOTION_APPEARS_AFTER_MS : u128 = 150;
// The step constants above and below land on the first tick at or after them, so they quantise to
// this grid: keep them multiples of it, or a 251 behaves as a 300
const TICK : Duration = Duration::from_millis(50);
const THREE_DOTS_STEP_MS : u128 = 300;
const NUMBER_REFRESH_MS : u128 = 300;
// The count sits right-aligned in this field, so a new digit fills reserved space to its left
// instead of pushing the text beside it. Wide enough that only ten million files overflow it.
const COUNT_FIELD_WIDTH : usize = "9,999,999".len();
const ERASE_LINE : &str = "\r\x1b[2K";

// The moving parts of a run on the terminal, drawn by a differnet thread of this crate.
// Everything transient goes to stderr and is erased before anything permanent prints, and none of it exists
// unless the output is a terminal: a piped run stays byte-identical with a build that had none of
// this. The gate is 'is_terminal' and never CLICOLOR_FORCE, which forces color into pipes and must
// not force motion into them.
pub struct LiveDisplay {
    should_stop: Arc<AtomicBool>,
    animator: Mutex<Option<JoinHandle<()>>>,
    settled_line: String
}

// Prints the walk heading, animated when both output streams are a terminal and static otherwise.
// Owning both forms is the point: the text exists once, and the piped form is untouched by the
// live one existing.
pub fn start_walk_display(config: &Configuration, progress: Arc<ScanProgress>) -> LiveDisplay {
    if config.view.hidden.directory_info || !config.view.prints_text() {
        return LiveDisplay::default();
    }
    let heading = crate::theme::get_active().heading.paint(WALK_HEADING).to_string();
    if !std::io::stdout().is_terminal() || !std::io::stderr().is_terminal() {
        println!("\n{heading}...");
        return LiveDisplay::default();
    }

    // The line begins on stdout, dotless and unfinished; the animator redraws it in place from
    // stderr, and 'finish' settles it into exactly the line the static branch above prints
    print!("\n{heading}");
    let _ = std::io::stdout().flush();
    let stop = Arc::new(AtomicBool::new(false));
    let animator = {
        let (progress, stop, heading) = (progress, stop.clone(), heading.clone());
        thread::Builder::new().name("live-progress".to_owned())
                .spawn(move || animate_walk_line(&progress, &stop, &heading)).ok()
    };
    if animator.is_none() {
        println!("...");
    }

    LiveDisplay {
        should_stop: stop,
        animator: Mutex::new(animator),
        settled_line: format!("{heading}...")
    }
}

impl LiveDisplay {
    // Idempotent, and called on every path out of the run: the report and the errors both print on
    // ground the animator has left for good.
    pub fn finish(&self) {
        let Some(animator) = self.animator.lock().unwrap().take() else { return };
        self.should_stop.store(true, Ordering::Relaxed);
        let _ = animator.join();
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(format!("{ERASE_LINE}{}\n", self.settled_line).as_bytes());
        let _ = stderr.flush();
    }
}

impl Default for LiveDisplay {
    fn default() -> Self {
        LiveDisplay {
            should_stop: Arc::new(AtomicBool::new(false)),
            animator: Mutex::new(None),
            settled_line: String::new()
        }
    }
}

fn animate_walk_line(progress: &ScanProgress, stop: &AtomicBool, heading: &str) {
    let started = Instant::now();
    let mut last_written = String::new();
    let mut previous_width = 0;
    let mut shown_files = 0;
    let mut last_number_slot = 0;
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(TICK);
        let elapsed = started.elapsed().as_millis();
        if elapsed < MOTION_APPEARS_AFTER_MS {
            continue;
        }
        let number_slot = elapsed / NUMBER_REFRESH_MS;
        if number_slot != last_number_slot {
            last_number_slot = number_slot;
            shown_files = progress.get_files_found();
        }
        let frame = format_walk_frame(heading, elapsed, shown_files);
        if frame != last_written {
            previous_width = overwrite_transient_line(&frame, previous_width);
            last_written = frame;
        }
    }
}

// Never an erase between frames, and one write per frame: an erase that reaches the screen before
// its rewrite is a blank the eye reads as flicker, and an unbuffered stderr sends every piece of a
// format string as its own write. Overwriting changes each cell once, and the padding covers what
// a shorter frame would leave behind. Returns the width it drew, which is the next frame's padding.
fn overwrite_transient_line(text: &str, previous_width: usize) -> usize {
    let width = crate::theme::calculate_visible_len(text);
    let padding = " ".repeat(previous_width.saturating_sub(width));
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(format!("\r{text}{padding}").as_bytes());
    let _ = stderr.flush();
    width
}

fn format_walk_frame(heading: &str, elapsed_ms: u128, files_found: usize) -> String {
    let dots = ".".repeat((elapsed_ms / THREE_DOTS_STEP_MS % 3) as usize + 1);
    if files_found == 0 {
        return format!("{heading}{dots}");
    }
    let count = crate::theme::Style::plain().dim()
            .paint(&format!("{:>width$} files discovered",
                    crate::number_formatter::format_with_separators(files_found), width = COUNT_FIELD_WIDTH));
    // The dots are padded to their widest, so the count does not shift as they cycle
    format!("{heading}{dots:<4}{count}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dots_cycle_through_one_to_three() {
        assert_eq!("h.", format_walk_frame("h", 0, 0));
        assert_eq!("h..", format_walk_frame("h", THREE_DOTS_STEP_MS, 0));
        assert_eq!("h...", format_walk_frame("h", 2 * THREE_DOTS_STEP_MS, 0));
        assert_eq!("h.", format_walk_frame("h", 3 * THREE_DOTS_STEP_MS, 0));
    }

    // Nothing found yet reads better as silence than as '0 files', and once the count is there it
    // sits in one column while the dots move beside it
    #[test]
    fn the_count_appears_once_files_were_found_and_holds_its_column() {
        assert!(!format_walk_frame("h", 0, 0).contains("files"));
        let one_dot = format_walk_frame("h", 0, 42);
        let three_dots = format_walk_frame("h", 2 * THREE_DOTS_STEP_MS, 42);
        assert!(one_dot.contains("42 files discovered") && three_dots.contains("42 files discovered"));
        assert_eq!(crate::theme::calculate_visible_len(&one_dot), crate::theme::calculate_visible_len(&three_dots));

        // and a count that grows a digit fills its reserved field instead of pushing the text
        let five_digits = format_walk_frame("h", 0, 99_999);
        let six_digits = format_walk_frame("h", 0, 100_000);
        assert_eq!(crate::theme::calculate_visible_len(&five_digits), crate::theme::calculate_visible_len(&six_digits));
    }
}
