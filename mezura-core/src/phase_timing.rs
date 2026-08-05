use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

// Diagnostic scaffolding behind MEZURA_PHASE_TIMING, to answer where the consumers spend their time
// without a profiler. The flag is resolved once, so a normal run pays one predictable branch per
// file. Per file is affordable at roughly 25 ns a reading; per line it would not be.
pub static ENABLED : LazyLock<bool> =
        LazyLock::new(|| enabled_by(std::env::var_os("MEZURA_PHASE_TIMING").as_deref()));

// The value and not merely the presence of the name. Asking 'is_some' meant that
// 'MEZURA_PHASE_TIMING=0' turned the report **on**, which is the opposite of what everything else a
// person has ever typed does: RUST_BACKTRACE, RUST_LOG and every tool of that shape read 0 as off.
// Setting it to zero is also the obvious way to turn it off without knowing how a shell unsets a
// variable, and it is what somebody reaches for first.
fn enabled_by(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| !matches!(value.to_string_lossy().trim().to_ascii_lowercase().as_str(),
            "" | "0" | "no" | "false" | "off"))
}

static OPEN_NANOS    : AtomicU64 = AtomicU64::new(0);
static READ_NANOS    : AtomicU64 = AtomicU64::new(0);
static PARSE_NANOS   : AtomicU64 = AtomicU64::new(0);
static BYTES         : AtomicU64 = AtomicU64::new(0);
static FILES         : AtomicU64 = AtomicU64::new(0);
static STARVED       : AtomicU64 = AtomicU64::new(0);
static STARVED_NANOS : AtomicU64 = AtomicU64::new(0);

// Accumulated by one thread, so the hot path touches no shared memory. The atomics above are
// written once per thread, at its exit.
#[derive(Debug, Default)]
pub struct Totals {
    pub open_nanos: u64,
    pub read_nanos: u64,
    pub parse_nanos: u64,
    pub bytes: u64,
    pub files: u64,
    pub starved: u64,
    pub starved_nanos: u64,
}

impl Totals {
    pub fn publish(&self) {
        OPEN_NANOS.fetch_add(self.open_nanos, Ordering::Relaxed);
        READ_NANOS.fetch_add(self.read_nanos, Ordering::Relaxed);
        PARSE_NANOS.fetch_add(self.parse_nanos, Ordering::Relaxed);
        BYTES.fetch_add(self.bytes, Ordering::Relaxed);
        FILES.fetch_add(self.files, Ordering::Relaxed);
        STARVED.fetch_add(self.starved, Ordering::Relaxed);
        STARVED_NANOS.fetch_add(self.starved_nanos, Ordering::Relaxed);
    }
}

pub fn now() -> Instant {
    Instant::now()
}

pub fn nanos_since(start: Instant) -> u64 {
    start.elapsed().as_nanos() as u64
}

// Shares of the consumers' own time, and never the milliseconds behind them. Those are summed over
// every thread, so each one is larger than the whole run by roughly the number of consumers: 165,012
// ms of starvation printed next to a run of 3,482 ms is arithmetic nobody can do while reading, and
// no caveat rescues it. As a percentage the same figure needs no caveat at all, which is why the
// thread count and the duration are arguments here: the statics below know neither, and without
// them there is no denominator.
//
// Elapsed and not CPU: a thread blocked in 'open' counts there while it runs no instructions.
pub fn report(consumers: usize, run_millis: u128) -> String {
    format_report(&Totals {
        open_nanos: OPEN_NANOS.load(Ordering::Relaxed),
        read_nanos: READ_NANOS.load(Ordering::Relaxed),
        parse_nanos: PARSE_NANOS.load(Ordering::Relaxed),
        bytes: BYTES.load(Ordering::Relaxed),
        files: FILES.load(Ordering::Relaxed),
        starved: STARVED.load(Ordering::Relaxed),
        starved_nanos: STARVED_NANOS.load(Ordering::Relaxed)
    }, consumers, run_millis)
}

// Split from the reading of the statics so that the arithmetic can be asserted. Those are global to
// the process and every test that counts anything adds to them, so a test calling 'report' would be
// asserting on whatever the rest of the suite happened to leave behind.
fn format_report(totals: &Totals, consumers: usize, run_millis: u128) -> String {
    let consumers = consumers.max(1);
    // Every consumer is alive for the whole run, so this is what there was to spend. The four shares
    // below are of it and close on 100%, which is what makes each of them mean something on its own:
    // shares of the busy time alone said 'open 55.5%' while the threads were working a quarter of
    // their life, and hid the three quarters they spent waiting.
    let thread_nanos = (consumers as f64 * run_millis as f64 * 1_000_000.0).max(1.0);
    let share = |x: u64| 100.0 * x as f64 / thread_nanos;

    // Bytes over nanoseconds is bytes per nanosecond, which is gigabytes per second already. Against
    // the summed time inside a read and not against the run, so it is a rate one thread saw while it
    // was reading. There is no honest aggregate to print beside it: that would need the wall time
    // during which any reading was happening, and nothing here measures it. Dividing the bytes by
    // the whole run instead produced a second "GB/s" that is not a read rate at all, and the two
    // then crossed over depending on how many threads happened to be reading at once.
    let read_gb_per_second = if totals.read_nanos > 0 {totals.bytes as f64 / totals.read_nanos as f64} else {0.0};

    format!("[phase] {consumers} consumers: starved {:.1}% ({} waits) | open {:.1}% | read {:.1}% | parse {:.1}%\n\
[phase] {} files, {:.1} MB, read at {:.2} GB/s per thread",
        share(totals.starved_nanos), totals.starved, share(totals.open_nanos),
        share(totals.read_nanos), share(totals.parse_nanos),
        totals.files, totals.bytes as f64 / 1_048_576.0, read_gb_per_second)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The figures of a real run over a whole drive. Every one of them was misread off the wording
    // this replaces, which printed the summed milliseconds: 165,012 ms of starvation beside a run of
    // 3,482 ms is not a number anybody can place, and no caveat next to it helped.
    #[test]
    fn the_four_shares_are_of_the_consumers_own_time_and_close_on_a_hundred() {
        let totals = Totals {
            open_nanos: 30_228_000_000, read_nanos: 9_763_000_000, parse_nanos: 14_442_000_000,
            bytes: 7_633_832_346, files: 308_744,
            starved: 131_681, starved_nanos: 165_012_000_000
        };
        // 64 threads alive for 3,482 ms is 222,848 ms to spend, and the four below account for 98.5%
        let report = format_report(&totals, 64, 3482);

        assert!(report.contains("64 consumers: starved 74.0% (131681 waits) | open 13.6% | read 4.4% | parse 6.5%"),
                "{report}");
        // The rate is against the time spent reading and nothing else, so it is what one thread saw
        assert!(report.contains("308744 files, 7280.2 MB, read at 0.78 GB/s per thread"), "{report}");
    }

    // The count of waits is not the same statement as the share, and that is why both are printed.
    // 74% over 131,681 waits is a queue that trickles, at 1.25 ms a wait, which is the 2 ms sleep the
    // consumer falls back on; the same 74% over a handful of waits is a queue that stalled. Nothing
    // else in the report tells the two apart.
    #[test]
    fn the_share_and_the_count_of_waits_are_two_different_statements() {
        let trickling = Totals {starved: 131_681, starved_nanos: 165_012_000_000, ..Default::default()};
        let stalled = Totals {starved: 12, starved_nanos: 165_012_000_000, ..Default::default()};

        assert!(format_report(&trickling, 64, 3482).contains("starved 74.0% (131681 waits)"));
        assert!(format_report(&stalled, 64, 3482).contains("starved 74.0% (12 waits)"));
    }

    // The name being present used to be the whole of the test, so 'MEZURA_PHASE_TIMING=0' switched
    // the report on. Nothing else anybody types behaves that way, and setting it to zero is the
    // first thing somebody tries who does not know how their shell removes a variable.
    #[test]
    fn the_value_decides_and_not_merely_the_presence_of_the_name() {
        let set_to = |value: &str| enabled_by(Some(std::ffi::OsStr::new(value)));

        assert!(!enabled_by(None), "the report is on without the variable at all");
        for off in ["0", "no", "false", "off", "", "  ", "OFF", "False"] {
            assert!(!set_to(off), "'{off}' left the report on");
        }
        for on in ["1", "yes", "true", "on", "please"] {
            assert!(set_to(on), "'{on}' did not turn the report on");
        }
    }

    // A run that counted nothing reaches this with every counter at zero, so neither the shares nor
    // the rate has a denominator. It is a diagnostic and must not be the thing that takes the process
    // down; a zero thread count cannot arrive from 'run', which clamps it, but this takes one.
    #[test]
    fn a_run_with_nothing_in_it_reports_zeroes_rather_than_dividing_by_zero() {
        let report = format_report(&Totals::default(), 0, 0);

        assert!(report.contains("1 consumers: starved 0.0% (0 waits) | open 0.0% | read 0.0% | parse 0.0%"), "{report}");
        assert!(report.contains("0 files, 0.0 MB, read at 0.00 GB/s per thread"), "{report}");
    }
}
