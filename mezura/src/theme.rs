// Every printed token and the style it carries. One list of tokens generates the struct, the
// defaults, the name lookup and the name listing, so a new one cannot be added to some of them and
// forgotten in the rest.
use std::sync::{LazyLock, OnceLock};

use colored::{Color, ColoredString, Colorize};
use unicode_width::UnicodeWidthChar;

// A sweep is a color per cell, so it says something only where a run of cells is painted one at a
// time, which today is the live progress bar alone.
const SWEEPABLE_TOKENS : [&str; 2] = ["progress-bar-fill", "progress-bar-empty"];
const LABEL_GOLD: Color = Color::TrueColor { r: 181, g: 169, b: 138 };
const SIZE_GOLD: Color = Color::TrueColor { r: 125, g: 119, b: 105 };
// A step below the terminal's foreground, not a step above black. 'bright-black' and the 'dim'
// attribute both land far darker than this on most schemes.
const FAINT: Color = Color::TrueColor { r: 170, g: 170, b: 170 };
const FAINTER: Color = Color::TrueColor { r: 150, g: 150, b: 150 };
// The rows hanging under a language, and the same again for a file: a teal band and a grey one, so
// that the two lists under one language are told apart by color as well as by shape
const SUB_ROW_TEAL: Color = Color::TrueColor { r: 93, g: 135, b: 134 };
const SUB_ROW_TEAL_BRIGHT: Color = Color::TrueColor { r: 112, g: 153, b: 152 };
const SUB_ROW_TEAL_FAINT: Color = Color::TrueColor { r: 85, g: 102, b: 102 };
const FILE_ROW_GREY: Color = Color::TrueColor { r: 122, g: 122, b: 122 };

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
// painted one at a time, a sweep across them. A gradient holds the channels themselves and never a
// named color, because interpolation needs them and 'cyan' is whatever the terminal maps it to.
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
    // What the cell is painted on, where the fill is what the glyphs are painted in. Never a
    // gradient: a sweep answers per cell of a run, and a background belongs to one span of text.
    pub background: Option<Color>,
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
            Fill::Rainbow => Some(color_of_hue((across + phase) * 360.0)),
            _ => self.get_color()
        }
    }

    fn apply_attributes(&self, painted: ColoredString) -> ColoredString {
        let mut painted = painted;
        if let Some(background) = self.background {
            painted = painted.on_color(background);
        }
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

    // Every attribute is additive and the order of the attributes carries no meaning, so
    // 'code-label = bold' is as valid as 'code-label = b5a98a italic'. The colors are the one thing
    // that counts: the first is what the glyphs are painted in and the second what they sit on, so
    // 'total = white 223344' is white on a dark blue band. A gradient is one word with '..' inside
    // it, which is why two colors can never be read as one sweep. 'default' holds a place without
    // filling it, so 'default 223344' leaves the text the color the terminal gives it.
    pub fn parse(value: &str) -> Option<Style> {
        let mut style = Style::plain();
        let mut colors_given = 0;
        let mut token_count = 0;

        for token in value.split_whitespace() {
            token_count += 1;
            match token.to_lowercase().as_str() {
                "bold" => style.bold = true,
                "italic" => style.italic = true,
                "underline" => style.underline = true,
                "dim" => style.dim = true,
                "reverse" => style.reverse = true,
                "default" => colors_given += 1,
                _ => {
                    match colors_given {
                        0 => style.fill = parse_fill(token)?,
                        // A sweep answers per cell of a run and a background is one span, so the
                        // second color is a flat one or the value does not read at all
                        1 => style.background = Some(parse_single_color(token)?),
                        _ => return None
                    }
                    colors_given += 1;
                }
            }
        }

        if token_count == 0 || colors_given > 2 { None } else { Some(style) }
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
        // Straight after the fill, since the two are told apart by which comes first
        if let Some(background) = &self.background {
            parts.push(color_to_config_string(background));
        }
        if self.bold { parts.push("bold".to_owned()); }
        if self.italic { parts.push("italic".to_owned()); }
        if self.underline { parts.push("underline".to_owned()); }
        if self.dim { parts.push("dim".to_owned()); }
        if self.reverse { parts.push("reverse".to_owned()); }

        parts.join(" ")
    }

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

            pub fn get_token_names() -> &'static [&'static str] {
                &[$($name,)+]
            }

            #[cfg(test)]
            pub fn find_all_tokens(&self) -> Vec<(&'static str, String)> {
                vec![$(($name, self.$field.to_config_string())),+]
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
    separator_total   => "separator-total",   Style::of(FAINT);
    separator_header  => "separator-header",  Style::of(FAINT);
    // The color of the headers it sits in, painted on its own so that their italics do not slant a
    // glyph that is not a word
    sort_marker       => "sort-marker",       Style::of(LABEL_GOLD);
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
    size_unit         => "size-unit",         Style::of(SIZE_GOLD);
    keyword_number    => "keyword-number",    Style::of(FAINTER);
    keyword_label     => "keyword-label",     Style::plain().dim();

    details_language_header => "details-language-header", Style::of(LABEL_GOLD).italic();
    details_language_name   => "details-language-name",   Style::of(Color::Cyan);
    // The name of a module, wherever one is printed: the row that opens its section in the details,
    // its heading in the keywords, and its line in the history section
    details_module    => "details-module",    Style::of(LABEL_GOLD).bold();
    details_total     => "details-total",     Style::of(Color::BrightMagenta);

    // The rows hanging under a language, one token per column so that a theme can treat them as
    // their own band of the table. They are set apart by color and not by dimming: a column of dim
    // numbers is unreadable down its length, which is how they are meant to be read.
    nested_name       => "nested-name",       Style::of(SUB_ROW_TEAL);
    nested_branch     => "nested-branch",     Style::of(FAINT);
    nested_percent    => "nested-percent",    Style::of(SUB_ROW_TEAL_FAINT);
    nested_files      => "nested-files",      Style::of(SUB_ROW_TEAL);
    nested_lines      => "nested-lines",      Style::of(SUB_ROW_TEAL_BRIGHT);
    nested_code       => "nested-code",       Style::of(SUB_ROW_TEAL);
    nested_comments   => "nested-comments",   Style::of(SUB_ROW_TEAL);
    nested_extra      => "nested-extra",      Style::of(SUB_ROW_TEAL);
    nested_size       => "nested-size",       Style::of(SUB_ROW_TEAL);
    nested_size_unit  => "nested-size-unit",  Style::of(SIZE_GOLD);

    // The same set again for the files of a '--by-file' run. They hang under a language beside the
    // sections and are a different question asked of it, so a theme can tell the two apart.
    file_name         => "file-name",         Style::of(FILE_ROW_GREY);
    file_branch       => "file-branch",       Style::of(FAINT);
    file_percent      => "file-percent",      Style::of(FAINTER);
    file_files        => "file-files",        Style::plain();
    file_lines        => "file-lines",        Style::of(Color::White).bold();
    file_code         => "file-code",         Style::plain();
    file_comments     => "file-comments",     Style::plain();
    file_extra        => "file-extra",        Style::plain();
    file_size         => "file-size",         Style::plain();
    file_size_unit    => "file-size-unit",    Style::of(SIZE_GOLD);

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
    history_age      => "history-age",       Style::plain().dim().italic();
    history_label    => "history-label",     Style::of(FAINT);
    history_modified => "history-modified",  Style::of(Color::Yellow);
    history_modified_field => "history-modified-field", Style::plain();

    // The live lines, which only a terminal ever sees. The track is a step above a dark terminal's
    // own background and no more, and 'default' turns it off, leaving those cells blank.
    progress_bar_fill    => "progress-bar-fill",    Style::plain();
    progress_bar_empty   => "progress-bar-empty",   Style::of(Color::TrueColor { r: 34, g: 34, b: 34 });
    progress_bar_figures => "progress-bar-figures", Style::plain().dim();

    summary           => "summary",           Style::plain();
    // '--explain': the heading is the file line at the top, the two span styles paint stretches
    // inside a source line, the three bucket tokens paint the verdict words, and 'explain-detail'
    // is the secondary text of a verdict row.
    explain_heading   => "explain-heading",   Style::plain().bold();
    explain_string    => "explain-string",    Style::of(Color::Green);
    explain_comment   => "explain-comment",   Style::of(Color::BrightBlack).italic();
    explain_code      => "explain-code",      Style::of(Color::Cyan);
    explain_comments  => "explain-comments",  Style::of(Color::BrightBlack).italic();
    explain_extra     => "explain-extra",     Style::of(LABEL_GOLD).italic();
    explain_detail    => "explain-detail",    Style::of(Color::BrightBlack);
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

// The same 'token = value' shape that '--style' and a config's style block carry. The pairs stay
// raw rather than becoming a Theme, so that a token the file does not mention is left to the next
// layer of the chain.
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

    for line in crate::config_files::strip_byte_order_mark(contents).lines() {
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
    let (overrides, errors) = parse_overrides_leniently(value);
    match errors.into_iter().next() {
        Some(error) => Err(error),
        None if overrides.is_empty() => Err(ThemeParseError::MalformedLine(value.trim().to_owned())),
        None => Ok(overrides)
    }
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

// The precedence chain of the whole styling system, one ladder of increasing specificity: what the
// program hardcodes, then the named theme, then this project's config, then this run's '--style'.
// 'language-others' is the one inherited token: a theme that names the four language slots and not
// the fold almost always meant the fourth, and the two are never on screen together.
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
        named => {
            let spelled = format!("{named:?}");
            let mut written = String::with_capacity(spelled.len() + 2);
            for (i, character) in spelled.chars().enumerate() {
                if i > 0 && character.is_ascii_uppercase() {
                    written.push('-');
                }
                written.push(character.to_ascii_lowercase());
            }
            written
        }
    }
}

// How many columns a painted line takes on the screen: the escape sequences the styles above
// produce have bytes in the string and nothing on the screen, and CJK and emoji draw two columns
// each where a character count would say one. Every alignment in the report goes through this.
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

    // 'colored' 2.2.0 hid this behind COLORTERM, which almost no Windows terminal sets, and every
    // hex colour came out as the nearest of the sixteen. This fails if the pin against it is lifted.
    #[test]
    fn a_hex_colour_leaves_as_twenty_four_bit() {
        assert_eq!(Color::TrueColor{r:203,g:166,b:247}.to_fg_str(), "38;2;203;166;247");
        assert_eq!(Color::TrueColor{r:0,g:0,b:0}.to_bg_str(), "48;2;0;0;0");
    }

    #[test]
    fn a_sweep_gives_every_cell_of_a_run_its_own_color() {
        let gradient = Style::parse("ff0000..0000ff").unwrap();
        assert_eq!(Fill::Gradient(vec![(255, 0, 0), (0, 0, 255)]), gradient.fill);
        assert_eq!(Some(Color::TrueColor { r: 255, g: 0, b: 0 }), gradient.get_color_of_cell(0, 5, 0.0));
        assert_eq!(Some(Color::TrueColor { r: 0, g: 0, b: 255 }), gradient.get_color_of_cell(4, 5, 0.0));
        assert_eq!(Some(Color::TrueColor { r: 127, g: 0, b: 127 }), gradient.get_color_of_cell(2, 5, 0.0));
        // a run of one cell has nowhere to travel and takes the color it starts from
        assert_eq!(Some(Color::TrueColor { r: 255, g: 0, b: 0 }), gradient.get_color_of_cell(0, 1, 0.0));

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
        assert_eq!(None, Style::parse("italic blinking"));
        assert_eq!(None, Style::parse("b5a98"));
        // A third color has nowhere to go
        assert_eq!(None, Style::parse("cyan magenta yellow"));
        assert_eq!(None, Style::parse("cyan magenta default"));
    }

    #[test]
    fn the_second_color_of_a_style_is_what_the_text_sits_on() {
        let on_magenta = Style::parse("cyan magenta").expect("two colors did not read");
        assert_eq!(Fill::Flat(Color::Cyan), on_magenta.fill);
        assert_eq!(Some(Color::Magenta), on_magenta.background);

        // The place is held whichever way round it is left empty, so a background can be given
        // alone and a foreground can be given without one
        assert_eq!(Style { fill: Fill::Terminal, background: Some(Color::Cyan), ..Style::plain() },
                Style::parse("default cyan").unwrap());
        assert_eq!(Style { fill: Fill::Flat(Color::Cyan), background: None, ..Style::plain() },
                Style::parse("cyan default").unwrap());
        assert_eq!(None, Style::parse("cyan").unwrap().background);

        // The attributes are still in any order and still say nothing about which color is which
        let mixed = Style::parse("bold 001122 italic ffeedd").unwrap();
        assert_eq!((Fill::Flat(Color::TrueColor {r: 0, g: 0x11, b: 0x22}),
                Some(Color::TrueColor {r: 0xff, g: 0xee, b: 0xdd}), true, true),
                (mixed.fill, mixed.background, mixed.bold, mixed.italic));

        // A sweep is one word holding '..', which is what keeps it apart from two colors, and it
        // has no meaning behind a span of text
        assert_eq!(Fill::Gradient(vec![(0, 0, 0), (255, 255, 255)]),
                Style::parse("000000..ffffff").unwrap().fill);
        assert_eq!(None, Style::parse("cyan 000000..ffffff"));
        assert_eq!(None, Style::parse("cyan rainbow"));

        // And it survives being written out and read back, which is what '--save-theme' does
        let written = on_magenta.to_config_string();
        assert_eq!("cyan magenta", written);
        assert_eq!(on_magenta, Style::parse(&written).unwrap());
        let alone = Style::parse("default 223344").unwrap();
        assert_eq!(alone, Style::parse(&alone.to_config_string()).unwrap());
    }

    // The mark lands in front of the first token name, and a theme keeps reading past a line it
    // cannot parse, so without stripping it exactly one style silently does not apply.
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
        for value in ["cyan", "bright-magenta", "ff0080", "b5a98a italic", "bright-yellow bold underline dim",
                "default", "default bold", "reverse", "cyan reverse bold", "ff0000..0000ff", "rainbow",
                "ff0000..0000ff dim", "ff0000..00ff00..0000ff"] {
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
    fn parses_a_theme_file() {
        let (styles, errors) = parse_theme_file("# a comment\nlanguage-1 = cyan\n\ncode-label = b5a98a italic\ncode-number = dim\n");
        assert!(errors.is_empty());
        assert_eq!(vec![("language-1".to_owned(), "cyan".to_owned()), ("code-label".to_owned(), "b5a98a italic".to_owned()),
                ("code-number".to_owned(), "dim".to_owned())], styles);
    }

    #[test]
    fn a_broken_line_does_not_discard_the_lines_around_it() {
        let (styles, errors) = parse_theme_file(
                "language-1 = cyan\nlabell = cyan\ncyan magenta\ncode-label = nope\nheading = white bold\n");
        assert_eq!(vec![("language-1".to_owned(), "cyan".to_owned()), ("heading".to_owned(), "white bold".to_owned())], styles);
        assert_eq!(vec![ThemeParseError::UnknownToken("labell".to_owned()),
                ThemeParseError::MalformedLine("cyan magenta".to_owned()),
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

    #[test]
    fn the_others_slot_follows_the_fourth_language_unless_it_is_declared() {
        let four = [("language-4".to_owned(), "cyan".to_owned())];
        assert_eq!(Style::of(Color::Cyan), resolve(&four, &[], &[]).language_others);

        let both = [("language-4".to_owned(), "cyan".to_owned()), ("language-others".to_owned(), "red".to_owned())];
        assert_eq!(Style::of(Color::Red), resolve(&both, &[], &[]).language_others);

        assert_eq!(Theme::default().language_others, resolve(&[], &[], &[]).language_others);
    }

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
