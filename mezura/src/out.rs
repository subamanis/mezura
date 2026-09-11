use std::fmt::{Arguments, Write as FmtWrite};
use std::io::{IsTerminal, Write};
use std::sync::Mutex;

static HELD: Mutex<Option<String>> = Mutex::new(None);

// Writes what was held, so every way out of 'main' and a panic both write it.
pub struct HeldOutput;

impl Drop for HeldOutput {
    fn drop(&mut self) {
        let held = HELD.lock().unwrap().take();
        if let Some(text) = held && !text.is_empty() {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(text.as_bytes());
            let _ = out.flush();
        }
    }
}

// A terminal is left unheld, since the walk display draws over the line it started. Piped, the
// lines are held and written once, and stderr then lands ahead of them.
pub fn hold_unless_a_terminal() -> HeldOutput {
    if !std::io::stdout().is_terminal() {
        *HELD.lock().unwrap() = Some(String::new());
    }

    HeldOutput
}

pub fn write_line(args: Arguments) {
    let mut held = HELD.lock().unwrap();
    match held.as_mut() {
        Some(text) => {
            let _ = text.write_fmt(args);
            text.push('\n');
        },
        None => {
            let mut out = std::io::stdout().lock();
            let _ = out.write_fmt(args);
            let _ = out.write_all(b"\n");
        }
    }
}

pub fn write(args: Arguments) {
    let mut held = HELD.lock().unwrap();
    match held.as_mut() {
        Some(text) => { let _ = text.write_fmt(args); },
        None => { let _ = std::io::stdout().write_fmt(args); }
    }
}

macro_rules! outln {
    () => { $crate::out::write_line(format_args!("")) };
    ($($arg:tt)*) => { $crate::out::write_line(format_args!($($arg)*)) }
}

macro_rules! out {
    ($($arg:tt)*) => { $crate::out::write(format_args!($($arg)*)) }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    // A 'println!' anywhere else writes its line on the spot, and nothing else reports it.
    #[test]
    fn every_line_of_output_is_written_through_this_module() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|x| x != "rs") || path.file_name().unwrap() == "out.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for (at, line) in text.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                let hunted = line.replace("eprintln!(", "").replace("eprint!(", "");
                if hunted.contains("println!(") || hunted.contains("print!(") {
                    found.push(format!("{}:{}", path.file_name().unwrap().to_string_lossy(), at + 1));
                }
            }
        }

        assert!(found.is_empty(), "these lines print straight to stdout and belong in 'outln!' or \
                'out!': {}", found.join(", "));
    }
}
