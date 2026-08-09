// Every printed token and the style it carries. One list of tokens generates the struct, the
// defaults, the name lookup and the name listing, so a new one cannot be added to some of them and
// forgotten in the rest.
use std::sync::{LazyLock, OnceLock};

use colored::{Color, ColoredString, Colorize};
use unicode_width::UnicodeWidthChar;

// A sweep is a color per cell, so it says something only where a run of cells is painted one at a
// time. Today that is the live progress bar; the overview's own bar is the natural next one.
const SWEEPABLE_TOKENS : [&str; 2] = ["progress-bar-fill", "progress-bar-empty"];
const LABEL_GOLD: Color = Color::TrueColor { r: 181, g: 169, b: 138 };
const SIZE_GOLD: Color = Color::TrueColor { r: 125, g: 119, b: 105 };
// A step below the terminal's foreground, not a step above black. 'bright-black' and the 'dim'
// attribute both land far darker than this on most schemes.
const FAINT: Color = Color::TrueColor { r: 170, g: 170, b: 170 };
const FAINTER: Color = Color::TrueColor { r: 150, g: 150, b: 150 };
const KEYWORD_GREY: Color = Color::TrueColor { r: 181, g: 181, b: 181 };

static ACTIVE_THEME: OnceLock<Theme> = OnceLock::new();
static DEFAULT_THEME: LazyLock<Theme> = LazyLock::new(Theme::default);

// Messages printed before the configuration exists (failed initialization, unreadable language
// files) fall back to the defaults rather than being left unstyled
pub fn get_active() -> &'static Theme {
    ACTIVE_THEME.get().unwrap_or(&DEFAULT_THEME)
}

pub fn set_active(theme: Theme) {
    let _ = ACTIVE_THEME.set(theme);
}

// How a style gets its color: one color for everything it paints, or, where a run of cells is
// painted one at a time, a sweep across them. A gradient holds the channels themselves and never
// a named color, because interpolation needs them and 'cyan' is whatever the terminal's own
// scheme maps it to; two stops or more, spread evenly over the run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Fill {
    #[default]
    Terminal,
    Flat(Color),
    Gradient(Vec<(u8, u8, u8)>),
    Rainbow
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    pub fill: Fill,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
    pub reverse: bool,
}

impl Style {
    pub fn plain() -> Style {
        Style::default()
    }

    pub fn of(color: Color) -> Style {
        Style { fill: Fill::Flat(color), ..Style::default() }
    }

    pub fn bold(mut self) -> Style {
        self.bold = true;
        self
    }

    pub fn italic(mut self) -> Style {
        self.italic = true;
        self
    }

    pub fn underline(mut self) -> Style {
        self.underline = true;
        self
    }

    pub fn dim(mut self) -> Style {
        self.dim = true;
        self
    }

    pub fn paint(&self, text: &str) -> ColoredString {
        match self.get_color() {
            Some(color) => self.paint_with_color(text, color),
            None => self.apply_attributes(ColoredString::from(text))
        }
    }

    pub fn paint_with_color(&self, text: &str, color: Color) -> ColoredString {
        self.apply_attributes(ColoredString::from(text).color(color))
    }

    // The one color a style paints everything with. A sweep has none, since it answers per cell,
    // and its first stop is the closest thing to an answer for a caller painting text.
    pub fn get_color(&self) -> Option<Color> {
        match &self.fill {
            Fill::Flat(color) => Some(*color),
            Fill::Gradient(stops) => stops.first().map(|&(r, g, b)| Color::TrueColor { r, g, b }),
            Fill::Terminal | Fill::Rainbow => None
        }
    }

    // The color of one cell of a run of them. 'phase' turns once per rainbow cycle and is the
    // caller's clock: the cells of a still frame are one moment of it.
    pub fn get_color_of_cell(&self, at: usize, cells: usize, phase: f32) -> Option<Color> {
        let across = if cells > 1 {at as f32 / (cells - 1) as f32} else {0.0};
        match &self.fill {
            Fill::Gradient(stops) => Some(interpolate_stops(stops, across)),
            // The spectrum spans the run, and the phase carries it along: every cell has moved a
            // whole cycle by the time the animation repeats
            Fill::Rainbow => Some(color_of_hue((across + phase) * 360.0)),
            _ => self.get_color()
        }
    }

    fn apply_attributes(&self, painted: ColoredString) -> ColoredString {
        let mut painted = painted;
        if self.bold {
            painted = painted.bold();
        }
        if self.italic {
            painted = painted.italic();
        }
        if self.underline {
            painted = painted.underline();
        }
        if self.dim {
            painted = painted.dimmed();
        }
        if self.reverse {
            painted = painted.reversed();
        }

        painted
    }

    // Every attribute is additive and only one color is allowed, so the order of the tokens carries
    // no meaning and 'code-label = bold' is as valid as 'code-label = b5a98a italic'
    pub fn parse(value: &str) -> Option<Style> {
        let mut style = Style::plain();
        let mut color_was_given = false;
        let mut token_count = 0;

        for token in value.split_whitespace() {
            token_count += 1;
            match token.to_lowercase().as_str() {
                "bold" => style.bold = true,
                "italic" => style.italic = true,
                "underline" => style.underline = true,
                "dim" => style.dim = true,
                "reverse" => style.reverse = true,
                "default" => {
                    if color_was_given { return None; }
                    color_was_given = true;
                },
                _ => {
                    if color_was_given { return None; }
                    color_was_given = true;
                    style.fill = parse_fill(token)?;
                }
            }
        }

        if token_count == 0 { None } else { Some(style) }
    }

    pub fn to_config_string(&self) -> String {
        let mut parts = Vec::with_capacity(5);
        parts.push(match &self.fill {
            Fill::Terminal => "default".to_owned(),
            Fill::Flat(color) => color_to_config_string(color),
            Fill::Gradient(stops) => stops.iter().map(|(r, g, b)| format!("{r:02x}{g:02x}{b:02x}"))
                    .collect::<Vec<_>>().join(".."),
            Fill::Rainbow => "rainbow".to_owned()
        });
        if self.bold { parts.push("bold".to_owned()); }
        if self.italic { parts.push("italic".to_owned()); }
        if self.underline { parts.push("underline".to_owned()); }
        if self.dim { parts.push("dim".to_owned()); }
        if self.reverse { parts.push("reverse".to_owned()); }

        parts.join(" ")
    }

    // Used only by the tests of this file, which is what marks it
    #[cfg(test)]
    pub fn reverse(mut self) -> Style {
        self.reverse = true;
        self
    }
}

macro_rules! theme_tokens {
    ($($field:ident => $name:literal, $default:expr;)+) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct Theme {
            $(pub $field: Style,)+
        }

        impl Default for Theme {
            fn default() -> Self {
                Theme { $($field: $default,)+ }
            }
        }

        impl Theme {
            pub fn get_style_of_token_mut(&mut self, token: &str) -> Option<&mut Style> {
                match token.to_lowercase().replace('_', "-").as_str() {
                    $($name => Some(&mut self.$field),)+
                    _ => None
                }
            }

            #[cfg(test)]
            pub fn get_token_names() -> &'static [&'static str] {
                &[$($name,)+]
            }

            pub fn find_non_default_tokens(&self) -> Vec<(&'static str, String)> {
                let defaults = Theme::default();
                let mut entries = Vec::new();
                $(if self.$field != defaults.$field {
                    entries.push(($name, self.$field.to_config_string()));
                })+
                entries
            }
        }
    };
}

// Every counted quantity owns both of its tokens, the number and the word next to it, so that a
// theme can pick out one of them without touching the rest. The token is named after the word that
// appears on screen, so 'files' and 'comments' are plural and 'code' and 'extra' are not.
theme_tokens! {
    version           => "version",           Style::plain();
    heading           => "heading",           Style::plain().underline().bold();
    separator         => "separator",         Style::of(FAINT);
    arrow             => "arrow",             Style::of(FAINT);
    bar_frame         => "bar-frame",         Style::plain();
    percent           => "percent",           Style::of(FAINTER);

    files_number      => "files-number",      Style::plain();
    files_label       => "files-label",       Style::of(LABEL_GOLD).italic();
    lines_number      => "lines-number",      Style::of(Color::White).bold();
    lines_label       => "lines-label",       Style::of(LABEL_GOLD).italic();
    code_number       => "code-number",       Style::plain();
    code_label        => "code-label",        Style::of(LABEL_GOLD).italic();
    comments_number   => "comments-number",   Style::plain();
    comments_label    => "comments-label",    Style::of(LABEL_GOLD).italic();
    extra_number      => "extra-number",      Style::plain();
    extra_label       => "extra-label",       Style::of(LABEL_GOLD).italic();
    total_size_number => "total-size-number", Style::plain();
    total_size_label  => "total-size-label",  Style::of(LABEL_GOLD).italic();
    avg_size_number   => "avg-size-number",   Style::plain();
    avg_size_label    => "avg-size-label",    Style::of(LABEL_GOLD).italic();
    // One token for both the total and the average, since there is no reason to want KBs in one
    // color next to KBs in another. It is the one piece of the size that stays dim, because a unit
    // is the least informative part of a figure the reader is scanning past
    size_unit         => "size-unit",         Style::of(SIZE_GOLD);
    keyword_number    => "keyword-number",    Style::of(KEYWORD_GREY);
    keyword_label     => "keyword-label",     Style::plain().dim();

    // The word "Language" over the column and the name of a language in a row are different
    // things that happened to share a token, the same way the size header and the unit did
    details_language_header => "details-language-header", Style::of(LABEL_GOLD).italic();
    details_language_name   => "details-language-name",   Style::plain().bold();
    // The name of a module, wherever one is printed: the row that opens its section in the details,
    // its heading in the keywords, and its line in the history section
    details_module    => "details-module",    Style::of(LABEL_GOLD).bold();
    details_total     => "details-total",     Style::plain().bold();
    overview_label    => "overview-label",    Style::plain();
    overview_percent  => "overview-percent",  Style::plain();

    language_1        => "language-1",        Style::of(Color::Cyan);
    language_2        => "language-2",        Style::of(Color::BrightMagenta);
    language_3        => "language-3",        Style::of(Color::BrightYellow);
    language_4        => "language-4",        Style::of(Color::TrueColor { r: 106, g: 217, b: 189 });
    language_others   => "language-others",   Style::of(Color::TrueColor { r: 215, g: 201, b: 240 });

    // A figure that moved, wherever one is shown: the history section and a '--diff' comparison
    change_up       => "change-up",         Style::of(Color::TrueColor { r: 201, g: 255, b: 189 });
    change_down     => "change-down",       Style::of(Color::TrueColor { r: 219, g: 129, b: 129 });
    change_same     => "change-same",       Style::of(Color::TrueColor { r: 255, g: 255, b: 255 });
    history_entry    => "history-entry",     Style::plain().bold();
    // Two tokens and not one per setting: the word is the flag, the names are the detail
    history_modified => "history-modified",  Style::of(Color::Yellow);
    history_modified_field => "history-modified-field", Style::plain();

    // The live lines, which only a terminal ever sees. The track is a step above a dark
    // terminal's own background and no more: the bar has to be readable as a length, and the
    // part not reached yet is the quietest thing on the line. 'default' turns it off, leaving
    // those cells blank.
    progress_bar_fill    => "progress-bar-fill",    Style::plain();
    progress_bar_empty   => "progress-bar-empty",   Style::of(Color::TrueColor { r: 34, g: 34, b: 34 });
    progress_bar_figures => "progress-bar-figures", Style::plain().dim();

    summary           => "summary",           Style::plain();
    note              => "note",              Style::plain().dim().italic();
    success           => "success",           Style::of(Color::BrightGreen);
    warning           => "warning",           Style::of(Color::Yellow);
    error             => "error",             Style::of(Color::Red);
    footer            => "footer",            Style::plain().dim();
}

impl Theme {
    // The overview paints a language by the slot it occupies. The bar cells take the color alone,
    // since bold or underline on a block character is not something a terminal shows usefully.
    pub fn get_language_slots(&self) -> [&Style; 5] {
        [&self.language_1, &self.language_2, &self.language_3, &self.language_4, &self.language_others]
    }

    pub fn get_language_colors(&self) -> [Color; 5] {
        self.get_language_slots().map(|x| x.get_color().unwrap_or(Color::White))
    }

    pub fn set_token(&mut self, token: &str, value: &str) -> Result<(), ThemeParseError> {
        let style = Style::parse(value).ok_or_else(|| ThemeParseError::InvalidValue(token.to_owned(), value.trim().to_owned()))?;
        if matches!(style.fill, Fill::Gradient(_) | Fill::Rainbow)
                && !SWEEPABLE_TOKENS.contains(&token.to_lowercase().replace('_', "-").as_str()) {
            return Err(ThemeParseError::OneColorOnly(token.trim().to_owned()));
        }
        match self.get_style_of_token_mut(token) {
            Some(existing) => {
                *existing = style;
                Ok(())
            },
            None => Err(ThemeParseError::UnknownToken(token.trim().to_owned()))
        }
    }
}

// The same 'token = value' shape that '--style' and a config's style block carry. Tokens it does not
// mention are left to the next layer of the chain, which is why the overrides stay raw pairs rather
// than becoming a Theme.
pub type ThemeFile = (Vec<(String, String)>, Vec<ThemeParseError>);

#[derive(Debug, PartialEq, Eq)]
pub enum ThemeParseError {
    UnknownToken(String),
    InvalidValue(String, String),
    OneColorOnly(String),
    MalformedLine(String),
    EmptyTheme,
}

impl ThemeParseError {
    pub fn format(&self) -> String {
        match self {
            Self::UnknownToken(token) => format!("'{token}' is not a style token."),
            Self::InvalidValue(token, value) =>
                format!("'{value}' is not a valid style for '{token}'. Expected a color (hex or a terminal color name), or for the progress bar cells a gradient of two or more hex values separated by '..', or 'rainbow', and any of: bold, italic, underline, dim, reverse."),
            Self::OneColorOnly(token) =>
                format!("'{token}' takes one color. A gradient and 'rainbow' give a color to each cell of a run, which today only the cells of the live progress bar are."),
            Self::MalformedLine(line) => format!("'{line}' is not a 'token = value' line."),
            Self::EmptyTheme => "the theme declares no styles at all.".to_owned()
        }
    }
}

pub fn parse_theme_file(contents: &str) -> ThemeFile {
    let mut validation_theme = Theme::default();
    let (mut styles, mut errors) = (Vec::new(), Vec::new());

    for line in super::strip_byte_order_mark(contents).lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((token, value)) = line.split_once('=') else {
            errors.push(ThemeParseError::MalformedLine(line.to_owned()));
            continue;
        };
        let (token, value) = (token.trim().to_lowercase(), value.trim());

        match validation_theme.set_token(&token, value) {
            Ok(()) => styles.push((token, value.to_owned())),
            Err(x) => errors.push(x)
        }
    }

    // A theme that declares nothing at all is a broken file, not an empty preference. It is only
    // worth saying when nothing else was wrong, since otherwise the reason is already above.
    if styles.is_empty() && errors.is_empty() {
        errors.push(ThemeParseError::EmptyTheme);
    }

    (styles, errors)
}

pub fn create_theme_file_contents(styles: &[(String, String)]) -> String {
    styles.iter().map(|(token, value)| format!("{token} = {value}\n")).collect()
}

// The form used by '--style' and by the 'style' line of a config file: comma separated pairs, the
// same shape every other list-valued command in the program uses
pub fn parse_overrides(value: &str) -> Result<Vec<(String, String)>, ThemeParseError> {
    let mut validation_theme = Theme::default();
    let mut overrides = Vec::new();

    // Commas on the command line, one per line in a configuration file
    for entry in value.split([',', '\n']).map(str::trim).filter(|x| !x.is_empty()) {
        let Some((token, style)) = entry.split_once('=') else {
            return Err(ThemeParseError::MalformedLine(entry.to_owned()));
        };
        let (token, style) = (token.trim().to_lowercase(), style.trim().to_owned());
        validation_theme.set_token(&token, &style)?;
        overrides.push((token, style));
    }

    if overrides.is_empty() { Err(ThemeParseError::MalformedLine(value.trim().to_owned())) } else { Ok(overrides) }
}

// The same list as above, read the way a file is read: a mistake on one line is reported and the
// line is dropped, while every other line still applies. '--style' keeps stopping on the first one,
// because there the mistake was just typed and can be retyped.
pub fn parse_overrides_leniently(value: &str) -> (Vec<(String, String)>, Vec<ThemeParseError>) {
    let mut validation_theme = Theme::default();
    let (mut overrides, mut errors) = (Vec::new(), Vec::new());

    for entry in value.split([',', '\n']).map(str::trim).filter(|x| !x.is_empty()) {
        let Some((token, style)) = entry.split_once('=') else {
            errors.push(ThemeParseError::MalformedLine(entry.to_owned()));
            continue;
        };
        let (token, style) = (token.trim().to_lowercase(), style.trim().to_owned());
        match validation_theme.set_token(&token, &style) {
            Ok(()) => overrides.push((token, style)),
            Err(x) => errors.push(x)
        }
    }

    (overrides, errors)
}

// The precedence chain of the whole styling system, in one place, one ladder of increasing
// specificity: what the program hardcodes, then the named theme, then this project's config, then
// this run's '--style'. 'language-others' is the one inherited token: a theme that names the four
// language slots and not the fold almost always meant the fourth, and the two are never on screen
// together, since folding only happens past five languages and then only three are shown.
pub fn resolve(theme_styles: &[(String, String)], config_styles: &[(String, String)], cmd_styles: &[(String, String)]) -> Theme {
    let mut theme = Theme::default();
    let (mut declared_fourth, mut declared_others) = (false, false);
    for (token, value) in theme_styles.iter().chain(config_styles.iter()).chain(cmd_styles.iter()) {
        declared_fourth |= token == "language-4";
        declared_others |= token == "language-others";
        let _ = theme.set_token(token, value);
    }

    if declared_fourth && !declared_others {
        theme.language_others = theme.language_4.clone();
    }

    theme
}

// 'bright_black' and 'bright-black' are one name, and so are '#ff0080' and 'ff0080'.
fn parse_fill(token: &str) -> Option<Fill> {
    if token == "rainbow" {
        return Some(Fill::Rainbow);
    }
    if !token.contains("..") {
        return Some(Fill::Flat(parse_single_color(token)?));
    }
    // A stop that is a name, or an empty one from a doubled separator, takes the whole value down
    let stops = token.split("..").map(|stop| match parse_single_color(stop) {
        Some(Color::TrueColor { r, g, b }) => Some((r, g, b)),
        _ => None
    }).collect::<Option<Vec<_>>>()?;

    (stops.len() > 1).then_some(Fill::Gradient(stops))
}

// 'across' is where the cell sits in the run, from 0 at the first to 1 at the last. It lands
// inside one pair of stops, and the color is that pair mixed by how far into it the cell is.
fn interpolate_stops(stops: &[(u8, u8, u8)], across: f32) -> Color {
    let scaled = across * (stops.len() - 1) as f32;
    let pair = (scaled as usize).min(stops.len() - 2);
    let ratio = scaled - pair as f32;
    let (from, to) = (stops[pair], stops[pair + 1]);
    let channel = |from: u8, to: u8| (f32::from(from) + (f32::from(to) - f32::from(from)) * ratio) as u8;

    Color::TrueColor { r: channel(from.0, to.0), g: channel(from.1, to.1), b: channel(from.2, to.2) }
}

// Full saturation and brightness, so the sweep is the rainbow everybody pictures rather than a
// pastel one. Six sectors of the circle, each holding one channel still while another moves.
fn color_of_hue(hue: f32) -> Color {
    let sector = (hue / 60.0).rem_euclid(6.0);
    let rising = (sector.fract() * 255.0) as u8;
    let falling = 255 - rising;
    let (r, g, b) = match sector as u8 {
        0 => (255, rising, 0),
        1 => (falling, 255, 0),
        2 => (0, 255, rising),
        3 => (0, falling, 255),
        4 => (rising, 0, 255),
        _ => (255, 0, falling)
    };

    Color::TrueColor { r, g, b }
}

pub fn parse_single_color(token: &str) -> Option<Color> {
    match token.to_lowercase().replace('_', "-").as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "bright-black" => Some(Color::BrightBlack),
        "bright-red" => Some(Color::BrightRed),
        "bright-green" => Some(Color::BrightGreen),
        "bright-yellow" => Some(Color::BrightYellow),
        "bright-blue" => Some(Color::BrightBlue),
        "bright-magenta" => Some(Color::BrightMagenta),
        "bright-cyan" => Some(Color::BrightCyan),
        "bright-white" => Some(Color::BrightWhite),
        other => {
            let hex = other.strip_prefix('#').unwrap_or(other);
            if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            Some(Color::TrueColor {
                r: u8::from_str_radix(&hex[0..2], 16).ok()?,
                g: u8::from_str_radix(&hex[2..4], 16).ok()?,
                b: u8::from_str_radix(&hex[4..6], 16).ok()?
            })
        }
    }
}

// The form 'parse_single_color' reads back, so a theme saved and loaded is the theme it was
pub fn color_to_config_string(color: &Color) -> String {
    match color {
        Color::TrueColor {r, g, b} => format!("{r:02x}{g:02x}{b:02x}"),
        named => format!("{:?}", named).chars().enumerate().flat_map(|(i, c)| {
            if i > 0 && c.is_ascii_uppercase() { vec!['-', c.to_ascii_lowercase()] }
            else { vec![c.to_ascii_lowercase()] }
        }).collect()
    }
}

// How many columns a painted line takes on the screen. The escape sequences the styles above
// produce have their bytes in the string and nothing on the screen, so they are skipped.
pub fn calculate_visible_len(line: &str) -> usize {
    visible_chars(line).count()
}

// Terminal columns and not characters: CJK and emoji occupy two each, which a character count
// declares half as wide as they draw. The report keeps counting characters, since its layouts and
// goldens are built on that; the live lines measure with this, because a revision name is the one
// user-written text that reaches them.
pub fn measure_columns(line: &str) -> usize {
    visible_chars(line).map(|character| character.width().unwrap_or(0)).sum()
}

fn visible_chars(line: &str) -> impl Iterator<Item = char> + '_ {
    let mut chars = line.chars();
    std::iter::from_fn(move || {
        while let Some(character) = chars.next() {
            if character == '\x1b' {
                for terminator in chars.by_ref() {
                    if terminator == 'm' {break}
                }
            } else {
                return Some(character);
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_to_config_string() {
        assert_eq!("cyan", color_to_config_string(&Color::Cyan));
        assert_eq!("bright-magenta", color_to_config_string(&Color::BrightMagenta));
        assert_eq!("ff0080", color_to_config_string(&Color::TrueColor{r:255,g:0,b:128}));
    }

    #[test]
    fn parses_colors_and_attributes_in_any_order() {
        assert_eq!(Some(Style::of(Color::TrueColor{r:181,g:169,b:138}).italic()), Style::parse("b5a98a italic"));
        assert_eq!(Some(Style::of(Color::TrueColor{r:181,g:169,b:138}).italic()), Style::parse("italic b5a98a"));
        assert_eq!(Some(Style::of(Color::Cyan)), Style::parse("cyan"));
        assert_eq!(Some(Style::of(Color::BrightMagenta).bold()), Style::parse("BRIGHT_MAGENTA Bold"));
        assert_eq!(Some(Style::plain().bold().underline()), Style::parse("bold underline"));
        assert_eq!(Some(Style::plain()), Style::parse("default"));
        assert_eq!(Some(Style::plain().dim()), Style::parse("default dim"));
        assert_eq!(Some(Style::plain().reverse()), Style::parse("reverse"));
        assert_eq!(Some(Style::of(Color::Cyan).reverse().bold()), Style::parse("reverse cyan bold"));
    }

    // A gradient answers per cell: its ends land on the first and the last, and the middle is
    // between them. A rainbow answers per cell and per moment.
    #[test]
    fn a_sweep_gives_every_cell_of_a_run_its_own_color() {
        let gradient = Style::parse("ff0000..0000ff").unwrap();
        assert_eq!(Fill::Gradient(vec![(255, 0, 0), (0, 0, 255)]), gradient.fill);
        assert_eq!(Some(Color::TrueColor { r: 255, g: 0, b: 0 }), gradient.get_color_of_cell(0, 5, 0.0));
        assert_eq!(Some(Color::TrueColor { r: 0, g: 0, b: 255 }), gradient.get_color_of_cell(4, 5, 0.0));
        assert_eq!(Some(Color::TrueColor { r: 127, g: 0, b: 127 }), gradient.get_color_of_cell(2, 5, 0.0));
        // a run of one cell has nowhere to travel and takes the colour it starts from
        assert_eq!(Some(Color::TrueColor { r: 255, g: 0, b: 0 }), gradient.get_color_of_cell(0, 1, 0.0));

        // every stop of a longer gradient lands on its own cell, and the pairs mix between them
        let three = Style::parse("ff0000..00ff00..0000ff").unwrap();
        assert_eq!(Fill::Gradient(vec![(255, 0, 0), (0, 255, 0), (0, 0, 255)]), three.fill);
        assert_eq!(Some(Color::TrueColor { r: 255, g: 0, b: 0 }), three.get_color_of_cell(0, 5, 0.0));
        assert_eq!(Some(Color::TrueColor { r: 0, g: 255, b: 0 }), three.get_color_of_cell(2, 5, 0.0));
        assert_eq!(Some(Color::TrueColor { r: 0, g: 0, b: 255 }), three.get_color_of_cell(4, 5, 0.0));
        assert_eq!(Some(Color::TrueColor { r: 127, g: 127, b: 0 }), three.get_color_of_cell(1, 5, 0.0));
        assert_eq!(Some(Color::TrueColor { r: 0, g: 127, b: 127 }), three.get_color_of_cell(3, 5, 0.0));

        let rainbow = Style::parse("rainbow").unwrap();
        let still = rainbow.get_color_of_cell(3, 10, 0.0);
        assert_ne!(still, rainbow.get_color_of_cell(4, 10, 0.0), "two cells of one frame share a color");
        assert_ne!(still, rainbow.get_color_of_cell(3, 10, 0.25), "the same cell stands still over time");
        assert_eq!(still, rainbow.get_color_of_cell(3, 10, 1.0), "a whole cycle does not come back around");

        // and a flat style answers the same for every cell, which is what makes this one question
        let flat = Style::parse("cyan").unwrap();
        assert_eq!(flat.get_color_of_cell(0, 9, 0.0), flat.get_color_of_cell(8, 9, 0.7));
    }

    // Interpolation needs the channels of both ends, and a terminal color name has none we know
    #[test]
    fn a_sweep_is_refused_where_it_could_not_be_honoured() {
        assert_eq!(None, Style::parse("red..blue"));
        assert_eq!(None, Style::parse("ff0000..cyan"));
        assert_eq!(None, Style::parse("ff0000..zzzzzz"));
        assert_eq!(None, Style::parse("ff0000..00ff00..red"));
        // a doubled separator leaves an empty stop, which is no color at all
        assert_eq!(None, Style::parse("ff0000....0000ff"));
        assert_eq!(None, Style::parse("ff0000.."));

        let mut theme = Theme::default();
        assert!(theme.set_token("progress-bar-fill", "ff0000..0000ff").is_ok());
        assert!(theme.set_token("progress-bar-empty", "rainbow").is_ok());
        assert_eq!(Err(ThemeParseError::OneColorOnly("heading".to_owned())),
                theme.set_token("heading", "rainbow"));
        assert_eq!(Err(ThemeParseError::OneColorOnly("language-1".to_owned())),
                theme.set_token("language-1", "ff0000..0000ff"));
    }

    #[test]
    fn rejects_malformed_styles() {
        assert_eq!(None, Style::parse(""));
        assert_eq!(None, Style::parse("   "));
        assert_eq!(None, Style::parse("cyan magenta"));
        assert_eq!(None, Style::parse("default cyan"));
        assert_eq!(None, Style::parse("italic blinking"));
        assert_eq!(None, Style::parse("b5a98"));
    }

    // The mark lands in front of the first token name, so that line stops being a token anybody
    // recognises and is reported as a malformed line instead. A theme is lenient by design, which is
    // what makes this the mildest of the four and also the least likely to be noticed: the rest of
    // the file applies, and exactly one style silently does not.
    #[test]
    fn a_theme_file_saved_with_a_byte_order_mark_still_reads() {
        let good = "heading = cyan bold\nlanguage-1 = b5a98a\n";
        let (styles, errors) = parse_theme_file(good);
        let (styles_with_mark, errors_with_mark) = parse_theme_file(&("\u{feff}".to_owned() + good));

        assert_eq!(styles, styles_with_mark, "a style read differently depending on how the editor saved it");
        assert!(errors.is_empty() && errors_with_mark.is_empty(), "{errors:?} / {errors_with_mark:?}");
        assert_eq!(2, styles_with_mark.len());
    }

    #[test]
    fn round_trips_through_its_config_representation() {
        for value in ["cyan", "b5a98a italic", "bright-yellow bold underline dim", "default", "default bold",
                "reverse", "cyan reverse bold", "ff0000..0000ff", "rainbow", "ff0000..0000ff dim",
                "ff0000..00ff00..0000ff"] {
            let style = Style::parse(value).unwrap();
            assert_eq!(Some(style.clone()), Style::parse(&style.to_config_string()), "round trip failed for '{value}'");
        }
    }

    #[test]
    fn token_names_resolve_and_unknown_ones_are_rejected() {
        let mut theme = Theme::default();
        for name in Theme::get_token_names() {
            assert!(theme.get_style_of_token_mut(name).is_some(), "'{name}' is listed but does not resolve");
        }
        assert!(theme.get_style_of_token_mut("details_language_name").is_some(), "underscores are accepted as separators");
        assert!(theme.get_style_of_token_mut("DETAILS-LANGUAGE-NAME").is_some(), "token names are case insensitive");
        assert_eq!(Err(ThemeParseError::UnknownToken("headings".to_owned())), theme.set_token("headings", "cyan"));
        assert_eq!(Err(ThemeParseError::InvalidValue("heading".to_owned(), "nonsense".to_owned())), theme.set_token("heading", "nonsense"));
    }

    #[test]
    fn the_overview_language_token_keeps_the_per_language_color() {
        // Asserting on the fields rather than the rendered text, because a test binary is not a
        // terminal and colored emits no escape codes there, which would make the check vacuous
        let style = Style::parse("red italic bold").unwrap();
        let painted = style.paint_with_color("Rust", Color::Cyan);

        assert_eq!(Some(Color::Cyan), painted.fgcolor(), "the caller's color must win");
        assert!(painted.style().contains(colored::Styles::Italic));
        assert!(painted.style().contains(colored::Styles::Bold));

        // and the ordinary paint still honours the style's own color
        assert_eq!(Some(Color::Red), style.paint("Rust").fgcolor());
    }

    #[test]
    fn only_modified_tokens_are_reported_as_non_default() {
        let mut theme = Theme::default();
        assert!(theme.find_non_default_tokens().is_empty());

        theme.set_token("code-number", "bright-black").unwrap();
        theme.set_token("percent", "dim").unwrap();
        let non_defaults = theme.find_non_default_tokens();
        assert_eq!(2, non_defaults.len());
        assert!(non_defaults.contains(&("code-number", "bright-black".to_owned())));
        assert!(non_defaults.contains(&("percent", "default dim".to_owned())));
    }

    #[test]
    fn a_plain_style_adds_no_escape_codes() {
        assert_eq!("Total", Style::plain().paint("Total").to_string());
    }

    #[test]
    fn parses_a_theme_file() {
        let (styles, errors) = parse_theme_file("# a comment\nlanguage-1 = cyan\n\ncode-label = b5a98a italic\ncode-number = dim\n");
        assert!(errors.is_empty());
        assert_eq!(vec![("language-1".to_owned(), "cyan".to_owned()), ("code-label".to_owned(), "b5a98a italic".to_owned()),
                ("code-number".to_owned(), "dim".to_owned())], styles);
    }

    #[test]
    fn reports_every_malformed_line_of_a_theme_file() {
        assert_eq!(vec![ThemeParseError::MalformedLine("cyan magenta".to_owned())], parse_theme_file("cyan magenta").1);
        assert_eq!(vec![ThemeParseError::UnknownToken("labell".to_owned())], parse_theme_file("labell = cyan").1);
        assert_eq!(vec![ThemeParseError::InvalidValue("code-label".to_owned(), "nope".to_owned())], parse_theme_file("code-label = nope").1);
        assert_eq!(vec![ThemeParseError::InvalidValue("language-1".to_owned(), "nope".to_owned())], parse_theme_file("language-1 = nope").1);
    }

    // A mistyped token costs nothing that was measured, so it must not take the rest of the file
    // down with it the way it used to
    #[test]
    fn a_broken_line_does_not_discard_the_lines_around_it() {
        let (styles, errors) = parse_theme_file("language-1 = cyan\nlabell = cyan\ncode-label = nope\nheading = white bold\n");
        assert_eq!(vec![("language-1".to_owned(), "cyan".to_owned()), ("heading".to_owned(), "white bold".to_owned())], styles);
        assert_eq!(vec![ThemeParseError::UnknownToken("labell".to_owned()),
                ThemeParseError::InvalidValue("code-label".to_owned(), "nope".to_owned())], errors);
    }

    #[test]
    fn a_theme_that_declares_nothing_says_so() {
        assert_eq!(vec![ThemeParseError::EmptyTheme], parse_theme_file("# just a comment\n\n").1);
        assert_eq!(vec![ThemeParseError::EmptyTheme], parse_theme_file("").1);
    }

    #[test]
    fn a_theme_file_round_trips() {
        let original = parse_theme_file("language-1 = cyan\nheading = white bold\nkeyword-label = bright-yellow italic\n").0;
        assert_eq!(original, parse_theme_file(&create_theme_file_contents(&original)).0);
    }

    // The fold and the fourth language never share a screen, so a theme that names the four slots
    // and not the fold almost always meant the fourth, and falling back to the built-in lilac
    // would wreck it. With neither declared, the built-in stays.
    #[test]
    fn the_others_slot_follows_the_fourth_language_unless_it_is_declared() {
        let four = [("language-4".to_owned(), "cyan".to_owned())];
        assert_eq!(Style::of(Color::Cyan), resolve(&four, &[], &[]).language_others);

        let both = [("language-4".to_owned(), "cyan".to_owned()), ("language-others".to_owned(), "red".to_owned())];
        assert_eq!(Style::of(Color::Red), resolve(&both, &[], &[]).language_others);

        assert_eq!(Theme::default().language_others, resolve(&[], &[], &[]).language_others);
    }

    // One ladder of increasing specificity: each layer wins over the one before it, and a token
    // that a later layer does not name keeps whatever the earlier one gave it
    #[test]
    fn every_style_layer_wins_over_the_one_before_it() {
        let theme_file = [("code-label".to_owned(), "cyan".to_owned()), ("code-number".to_owned(), "dim".to_owned()),
                ("heading".to_owned(), "green".to_owned())];
        let config = [("code-label".to_owned(), "bright-red bold".to_owned()), ("heading".to_owned(), "blue".to_owned())];
        let cmd = [("heading".to_owned(), "magenta underline".to_owned())];

        let theme = resolve(&theme_file, &config, &cmd);
        assert_eq!(Style::of(Color::Magenta).underline(), theme.heading);
        assert_eq!(Style::of(Color::BrightRed).bold(), theme.code_label);
        assert_eq!(Style::plain().dim(), theme.code_number);
        assert_eq!(Theme::default().summary, theme.summary);
    }
}
