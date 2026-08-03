// How a number reads to a person: the grouping of digits, the decimal mark, the rounding. None of it
// changes a count.
use std::sync::OnceLock;

use super::config_manager::{DecimalSeparator, NumberSeparator};

pub fn round_2(num: f64) -> f64 {
    (num * 100.0).round() / 100.0
}

// Reached from every printed figure, so it is set once instead of being threaded through the
// printer, the same way the active theme is
static NUMBER_SEPARATOR : OnceLock<NumberSeparator> = OnceLock::new();

static DECIMAL_SEPARATOR : OnceLock<DecimalSeparator> = OnceLock::new();

pub fn set_number_separator(separator: NumberSeparator) {
    let _ = NUMBER_SEPARATOR.set(separator);
}

pub fn set_decimal_separator(separator: DecimalSeparator) {
    let _ = DECIMAL_SEPARATOR.set(separator);
}

// Applied to text that is already rounded, so that every rule about rounding stays written with a
// dot and only the last step decides what the reader sees
pub fn with_decimal_separator(text: String) -> String {
    match DECIMAL_SEPARATOR.get().copied().unwrap_or_default().character() {
        '.' => text,
        separator => text.replace('.', &separator.to_string())
    }
}

pub fn with_seperators(i: usize) -> String {
    with_seperators_str(&i.to_string())
}

pub fn with_seperators_str(i_str: &str) -> String {
    let Some(separator) = NUMBER_SEPARATOR.get().copied().unwrap_or_default().character() else {
        return i_str.to_owned();
    };

    let mut s = String::new();
    let a = i_str.chars().rev().enumerate();
    for (idx, val) in a {
        if idx != 0 && idx % 3 == 0 {
            s.insert(0, separator);
        }
        s.insert(0, val);
    }
    s
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_with_seperators() {
        assert_eq!("123",with_seperators(123));
        assert_eq!("1,234",with_seperators(1234));
        assert_eq!("12,345",with_seperators(12345));
        assert_eq!("1,234,567",with_seperators(1234567));
    }
}
