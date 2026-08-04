// Colour belongs to whoever is doing the showing, so the trait lives here and not beside the errors
// it describes. The library gives those a plain 'Display'; this is the same text with a style on it.
use colored::{ColoredString, Colorize};

use mezura::{RunError, language_file::LanguageDirParseError};

pub trait Formatted {
    fn formatted(&self) -> ColoredString;
}

impl Formatted for RunError {
    fn formatted(&self) -> ColoredString {
        super::theme::active().warning.paint(&self.to_string())
    }
}

impl Formatted for LanguageDirParseError {
    fn formatted(&self) -> ColoredString {
        format!("Error: {self}").red()
    }
}
