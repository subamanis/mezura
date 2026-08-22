// The arithmetic is in 'mezura_core::render'; this is the one place that knows which grouping and
// decimal mark this run was started with.
use std::sync::OnceLock;

use mezura_core::render::NumberFormat;

use super::config_manager::{DecimalSeparator, NumberSeparator};

static FORMAT : OnceLock<NumberFormat> = OnceLock::new();

static NUMBER_SEPARATOR : OnceLock<NumberSeparator> = OnceLock::new();

static DECIMAL_SEPARATOR : OnceLock<DecimalSeparator> = OnceLock::new();

pub fn set_number_separator(separator: NumberSeparator) {
    let _ = NUMBER_SEPARATOR.set(separator);
}

pub fn set_decimal_separator(separator: DecimalSeparator) {
    let _ = DECIMAL_SEPARATOR.set(separator);
}

// Built on first use, by which time both separators have been set; they arrive one at a time.
pub fn get_active() -> &'static NumberFormat {
    FORMAT.get_or_init(|| NumberFormat::new(
            NUMBER_SEPARATOR.get().copied().unwrap_or_default().get_character(),
            DECIMAL_SEPARATOR.get().copied().unwrap_or_default().get_character()))
}

// Applied after rounding, so that every rounding rule stays written with a dot.
pub fn format_with_decimal_separator(text: String) -> String {
    get_active().with_decimal_mark(&text)
}

pub fn format_with_separators(i: usize) -> String {
    get_active().integer(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The grouping itself is 'NumberFormat::integer' and is asserted there. What is this crate's is
    // that a run where nobody called the two setters still formats with a comma and a dot.
    #[test]
    fn a_run_that_set_no_separators_groups_with_a_comma() {
        assert_eq!("1,234,567", format_with_separators(1234567));
    }
}
