// How a number reads to a person: the grouping of digits, the decimal mark, the rounding. None of it
// changes a count, and none of the arithmetic is here: 'mezura_core::render' holds that, and this is
// the one place that knows which preferences this run was started with.
use std::sync::OnceLock;

use mezura_core::render::NumberFormat;

use super::config_manager::{DecimalSeparator, NumberSeparator};

// Reached from every printed figure, so it is set once instead of being threaded through the
// printer, the same way the active theme is. The library takes the format as a value, which is what
// lets this stay a decision of the command line and of nothing else.
static FORMAT : OnceLock<NumberFormat> = OnceLock::new();

static NUMBER_SEPARATOR : OnceLock<NumberSeparator> = OnceLock::new();

static DECIMAL_SEPARATOR : OnceLock<DecimalSeparator> = OnceLock::new();

pub fn set_number_separator(separator: NumberSeparator) {
    let _ = NUMBER_SEPARATOR.set(separator);
}

pub fn set_decimal_separator(separator: DecimalSeparator) {
    let _ = DECIMAL_SEPARATOR.set(separator);
}

// Built on first use, by which time both commands have been read. Two separators arriving one at a
// time is why they are not a single setter.
pub fn active() -> &'static NumberFormat {
    FORMAT.get_or_init(|| NumberFormat::new(
            NUMBER_SEPARATOR.get().copied().unwrap_or_default().character(),
            DECIMAL_SEPARATOR.get().copied().unwrap_or_default().character()))
}

// Applied to text that is already rounded, so that every rule about rounding stays written with a
// dot and only the last step decides what the reader sees
pub fn with_decimal_separator(text: String) -> String {
    active().with_decimal_mark(&text)
}

pub fn with_seperators(i: usize) -> String {
    active().integer(i)
}



#[cfg(test)]
mod tests {
    use super::*;

    // The grouping itself is the library's and is asserted there. What this holds is the wiring:
    // the format this crate hands out is the one the commands asked for.
    #[test]
    pub fn test_with_seperators() {
        assert_eq!("123",with_seperators(123));
        assert_eq!("1,234",with_seperators(1234));
        assert_eq!("12,345",with_seperators(12345));
        assert_eq!("1,234,567",with_seperators(1234567));
    }
}
