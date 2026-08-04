use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

// Diagnostic scaffolding behind MEZURA_PHASE_TIMING, to answer where the consumers spend their time
// without a profiler. The flag is resolved once, so a normal run pays one predictable branch per
// file. Per file is affordable at roughly 25 ns a reading; per line it would not be.
pub static ENABLED : LazyLock<bool> = LazyLock::new(|| std::env::var_os("MEZURA_PHASE_TIMING").is_some());

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

// Elapsed inside each phase, not CPU: a thread blocked in 'open' is counted there while it runs no
// instructions at all. Summed across every consumer thread, so the total far exceeds the wall clock
// of the run, and the shares between them are the point rather than their sum.
pub fn report() -> String {
    let (open, read, parse) = (OPEN_NANOS.load(Ordering::Relaxed), READ_NANOS.load(Ordering::Relaxed),
            PARSE_NANOS.load(Ordering::Relaxed));
    let (bytes, files) = (BYTES.load(Ordering::Relaxed), FILES.load(Ordering::Relaxed));
    let busy = (open + read + parse).max(1);
    let share = |x: u64| 100.0 * x as f64 / busy as f64;
    let ms = |x: u64| x as f64 / 1_000_000.0;

    let starved = STARVED.load(Ordering::Relaxed);
    let starved_ms = ms(STARVED_NANOS.load(Ordering::Relaxed));

    format!("[phase] elapsed per phase, summed over consumer threads: open {:.0} ms ({:.1}%) | read {:.0} ms ({:.1}%) | \
parse {:.0} ms ({:.1}%)\n[phase] {} files, {:.1} MB read, {:.2} GB/s while reading | \
starved {} times for {:.0} ms while producers were still alive",
        ms(open), share(open), ms(read), share(read), ms(parse), share(parse),
        files, bytes as f64 / 1_048_576.0,
        if read > 0 { bytes as f64 / read as f64 } else { 0.0 },
        starved, starved_ms)
}
