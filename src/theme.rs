use std::sync::{LazyLock, OnceLock};

use crate::{Color, ColoredString, Colorize, utils};

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
pub fn active() -> &'static Theme {
    ACTIVE_THEME.get().unwrap_or(&DEFAULT_THEME)
}

pub fn set_active(theme: Theme) {
    let _ = ACTIVE_THEME.set(theme);
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    pub color: Option<Color>,
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
        Style { color: Some(color), ..Style::default() }
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

    pub fn reverse(mut self) -> Style {
        self.reverse = true;
        self
    }

    pub fn paint(&self, text: &str) -> ColoredString {
        self.apply_attributes(match self.color {
            Some(color) => ColoredString::from(text).color(color),
            None => ColoredString::from(text)
        })
    }

    // The overview colors every language individually, so the color comes from the caller there and
    // whatever the style itself declares is ignored
    pub fn paint_with_color(&self, text: &str, color: Color) -> ColoredString {
        self.apply_attributes(ColoredString::from(text).color(color))
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
                    style.color = Some(utils::parse_single_color(token)?);
                }
            }
        }

        if token_count == 0 { None } else { Some(style) }
    }

    pub fn to_config_string(&self) -> String {
        let mut parts = Vec::with_capacity(5);
        parts.push(self.color.map_or("default".to_owned(), |x| utils::color_to_config_string(&x)));
        if self.bold { parts.push("bold".to_owned()); }
        if self.italic { parts.push("italic".to_owned()); }
        if self.underline { parts.push("underline".to_owned()); }
        if self.dim { parts.push("dim".to_owned()); }
        if self.reverse { parts.push("reverse".to_owned()); }

        parts.join(" ")
    }
}

// The token list exists once, and the struct, the defaults, the name lookup and the name listing
// are all generated from it, so a new token cannot be added to one of them and forgotten in another
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
            pub fn style_of_token_mut(&mut self, token: &str) -> Option<&mut Style> {
                match token.to_lowercase().replace('_', "-").as_str() {
                    $($name => Some(&mut self.$field),)+
                    _ => None
                }
            }

            pub fn token_names() -> &'static [&'static str] {
                &[$($name,)+]
            }

            pub fn style_of_token(&self, token: &str) -> Option<&Style> {
                match token.to_lowercase().replace('_', "-").as_str() {
                    $($name => Some(&self.$field),)+
                    _ => None
                }
            }

            pub fn non_default_tokens(&self) -> Vec<(&'static str, String)> {
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
    // colour next to KBs in another. It is the one piece of the size that stays dim, because a unit
    // is the least informative part of a figure the reader is scanning past
    size_unit         => "size-unit",         Style::of(SIZE_GOLD);
    keyword_number    => "keyword-number",    Style::of(KEYWORD_GREY);
    keyword_label     => "keyword-label",     Style::plain().dim();

    details_language  => "details-language",  Style::plain().bold();
    details_total     => "details-total",     Style::plain().bold();
    overview_label    => "overview-label",    Style::plain();
    overview_percent  => "overview-percent",  Style::plain();

    language_1        => "language-1",        Style::of(Color::Cyan);
    language_2        => "language-2",        Style::of(Color::BrightMagenta);
    language_3        => "language-3",        Style::of(Color::BrightYellow);
    language_4        => "language-4",        Style::of(Color::TrueColor { r: 106, g: 217, b: 189 });
    language_others   => "language-others",   Style::of(Color::TrueColor { r: 215, g: 201, b: 240 });

    progress_up       => "progress-up",       Style::of(Color::TrueColor { r: 201, g: 255, b: 189 });
    progress_down     => "progress-down",     Style::of(Color::TrueColor { r: 219, g: 129, b: 129 });
    progress_same     => "progress-same",     Style::of(Color::TrueColor { r: 255, g: 255, b: 255 });
    progress_entry    => "progress-entry",    Style::plain().bold();

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
    pub fn language_slots(&self) -> [&Style; 5] {
        [&self.language_1, &self.language_2, &self.language_3, &self.language_4, &self.language_others]
    }

    pub fn language_colors(&self) -> [Color; 5] {
        self.language_slots().map(|x| x.color.unwrap_or(Color::White))
    }

    pub fn set_token(&mut self, token: &str, value: &str) -> Result<(), ThemeParseError> {
        let style = Style::parse(value).ok_or_else(|| ThemeParseError::InvalidValue(token.to_owned(), value.trim().to_owned()))?;
        match self.style_of_token_mut(token) {
            Some(existing) => {
                *existing = style;
                Ok(())
            },
            None => Err(ThemeParseError::UnknownToken(token.trim().to_owned()))
        }
    }
}

// A theme file is a list of 'token = value' lines and nothing else, which is the same shape that
// '--style' and a config's style block carry. Tokens it does not mention are left to whatever the
// next layer of the precedence chain provides, so the overrides stay raw pairs instead of a Theme.
pub type ThemeFile = (Vec<(String, String)>, Vec<ThemeParseError>);

pub fn parse_theme_file(contents: &str) -> ThemeFile {
    let mut validation_theme = Theme::default();
    let (mut styles, mut errors) = (Vec::new(), Vec::new());

    for line in contents.lines() {
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

pub fn theme_file_contents(styles: &[(String, String)]) -> String {
    styles.iter().map(|(token, value)| format!("{token} = {value}\n")).collect()
}

#[derive(Debug, PartialEq, Eq)]
pub enum ThemeParseError {
    UnknownToken(String),
    InvalidValue(String, String),
    MalformedLine(String),
    EmptyTheme,
}

impl ThemeParseError {
    pub fn formatted(&self) -> String {
        match self {
            Self::UnknownToken(token) => format!("'{token}' is not a style token."),
            Self::InvalidValue(token, value) =>
                format!("'{value}' is not a valid style for '{token}'. Expected a color (hex or a terminal color name) and any of: bold, italic, underline, dim, reverse."),
            Self::MalformedLine(line) => format!("'{line}' is not a 'token = value' line."),
            Self::EmptyTheme => "the theme declares no styles at all.".to_owned()
        }
    }
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

    #[test]
    fn rejects_malformed_styles() {
        assert_eq!(None, Style::parse(""));
        assert_eq!(None, Style::parse("   "));
        assert_eq!(None, Style::parse("cyan magenta"));
        assert_eq!(None, Style::parse("default cyan"));
        assert_eq!(None, Style::parse("italic blinking"));
        assert_eq!(None, Style::parse("b5a98"));
    }

    #[test]
    fn round_trips_through_its_config_representation() {
        for value in ["cyan", "b5a98a italic", "bright-yellow bold underline dim", "default", "default bold", "reverse", "cyan reverse bold"] {
            let style = Style::parse(value).unwrap();
            assert_eq!(Some(style.clone()), Style::parse(&style.to_config_string()), "round trip failed for '{value}'");
        }
    }

    #[test]
    fn token_names_resolve_and_unknown_ones_are_rejected() {
        let mut theme = Theme::default();
        for name in Theme::token_names() {
            assert!(theme.style_of_token_mut(name).is_some(), "'{name}' is listed but does not resolve");
        }
        assert!(theme.style_of_token_mut("details_language").is_some(), "underscores are accepted as separators");
        assert!(theme.style_of_token_mut("DETAILS-LANGUAGE").is_some(), "token names are case insensitive");
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
        assert!(theme.non_default_tokens().is_empty());

        theme.set_token("code-number", "bright-black").unwrap();
        theme.set_token("percent", "dim").unwrap();
        let non_defaults = theme.non_default_tokens();
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
        assert_eq!(original, parse_theme_file(&theme_file_contents(&original)).0);
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
