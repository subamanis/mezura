use std::sync::{LazyLock, OnceLock};

use crate::{Color, ColoredString, Colorize, utils};

const LABEL_GOLD: Color = Color::TrueColor { r: 181, g: 169, b: 138 };

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
    // no meaning and 'label = bold' is as valid as 'label = b5a98a italic'
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

// The defaults reproduce the hardcoded appearance of v2.0.1 exactly, so a palette that declares
// none of these renders byte for byte as before
theme_tokens! {
    heading         => "heading",         Style::plain().underline().bold();
    separator       => "separator",       Style::plain();
    bar_frame       => "bar-frame",       Style::plain();
    number          => "number",          Style::plain();
    percent         => "percent",         Style::plain();
    label           => "label",           Style::of(LABEL_GOLD).italic();
    overview_label  => "overview-label",  Style::plain();
    details_language => "details-language", Style::plain().bold();
    overview_language => "overview-language", Style::plain();
    details_total     => "details-total", Style::plain().bold();
    keyword         => "keyword",         Style::of(LABEL_GOLD).italic();
    progress_up     => "progress-up",     Style::of(Color::TrueColor { r: 201, g: 255, b: 189 });
    progress_down   => "progress-down",   Style::of(Color::TrueColor { r: 219, g: 129, b: 129 });
    progress_same   => "progress-same",   Style::of(Color::TrueColor { r: 255, g: 255, b: 255 });
    progress_entry  => "progress-entry",  Style::plain().bold();
    summary         => "summary",         Style::plain();
    success         => "success",         Style::of(Color::BrightGreen);
    warning         => "warning",         Style::of(Color::Yellow);
    error           => "error",           Style::of(Color::Red);
    footer          => "footer",          Style::plain();
}

impl Theme {
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

pub const FORMAT_MARKER: &str = "# mezura-format: 2";
pub const LANGUAGES_KEY: &str = "languages";

// A palette carries the language slot colors and any style tokens it chooses to override. Tokens it
// does not mention are left to whatever the next layer of the precedence chain provides, which is
// why the overrides are kept as raw pairs instead of being baked into a Theme here.
#[derive(Debug, PartialEq, Eq, Default)]
pub struct Palette {
    pub languages: Option<Vec<Color>>,
    pub styles: Vec<(String, String)>,
}

impl Palette {
    // The '#' of a comment and the '#' of a hex color are the same character, so a legacy file
    // whose first color is written as '#e80000' must not be mistaken for a comment. The old format
    // had no comments at all, so the first non-empty line is always its content.
    pub fn is_in_legacy_format(contents: &str) -> bool {
        match contents.lines().map(str::trim).find(|line| !line.is_empty()) {
            Some(line) => !line.starts_with("# mezura-format") && !line.contains('='),
            None => false
        }
    }

    // The pre-v3 format was a single positional line of colors. The conversion is mechanical and
    // lossless, so installed palettes are migrated in place instead of being rejected.
    pub fn from_legacy_format(contents: &str) -> Option<Palette> {
        let colors_line = contents.lines().find(|line| !line.trim().is_empty())?;
        Some(Palette { languages: Some(utils::parse_colors_to_vec(colors_line)?), styles: Vec::new() })
    }

    pub fn parse(contents: &str) -> Result<Palette, ThemeParseError> {
        let mut palette = Palette::default();
        let mut validation_theme = Theme::default();

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                return Err(ThemeParseError::MalformedLine(line.to_owned()));
            };
            let (key, value) = (key.trim().to_lowercase(), value.trim());

            if key == LANGUAGES_KEY {
                palette.languages = Some(utils::parse_colors_to_vec(value).ok_or_else(||
                        ThemeParseError::InvalidValue(LANGUAGES_KEY.to_owned(), value.to_owned()))?);
            } else {
                validation_theme.set_token(&key, value)?;
                palette.styles.push((key, value.to_owned()));
            }
        }

        // A palette that declares nothing at all is a broken file, not an empty preference. Without
        // this it would load successfully and silently leave everything at the defaults.
        if palette.languages.is_none() && palette.styles.is_empty() {
            return Err(ThemeParseError::EmptyPalette);
        }

        Ok(palette)
    }

    pub fn to_file_contents(&self) -> String {
        let mut contents = String::from(FORMAT_MARKER);
        contents.push('\n');
        if let Some(languages) = &self.languages {
            let rendered = languages.iter().map(utils::color_to_config_string).collect::<Vec<_>>();
            contents.push_str(&format!("{LANGUAGES_KEY} = {}\n", rendered.join(" ")));
        }
        for (token, value) in &self.styles {
            contents.push_str(&format!("{token} = {value}\n"));
        }

        contents
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ThemeParseError {
    UnknownToken(String),
    InvalidValue(String, String),
    MalformedLine(String),
    EmptyPalette,
}

impl ThemeParseError {
    pub fn formatted(&self) -> String {
        match self {
            Self::UnknownToken(token) => format!("'{token}' is not a style token."),
            Self::InvalidValue(token, value) =>
                format!("'{value}' is not a valid style for '{token}'. Expected a color (hex or a terminal color name) and any of: bold, italic, underline, dim, reverse."),
            Self::MalformedLine(line) => format!("'{line}' is not a 'token = value' line."),
            Self::EmptyPalette => "the palette declares no colors and no styles.".to_owned()
        }
    }
}

// The form used by '--style' and by the 'style' line of a config file: comma separated pairs, the
// same shape every other list-valued command in the program uses
pub fn parse_overrides(value: &str) -> Result<Vec<(String, String)>, ThemeParseError> {
    let mut validation_theme = Theme::default();
    let mut overrides = Vec::new();

    for entry in value.split(',').map(str::trim).filter(|x| !x.is_empty()) {
        let Some((token, style)) = entry.split_once('=') else {
            return Err(ThemeParseError::MalformedLine(entry.to_owned()));
        };
        let (token, style) = (token.trim().to_lowercase(), style.trim().to_owned());
        validation_theme.set_token(&token, &style)?;
        overrides.push((token, style));
    }

    if overrides.is_empty() { Err(ThemeParseError::MalformedLine(value.trim().to_owned())) } else { Ok(overrides) }
}

// The precedence chain of the whole styling system, in one place: what the program hardcodes, then
// what the palette declares, then what the user's own configuration says
pub fn resolve(palette_styles: &[(String, String)], config_styles: &[(String, String)]) -> Theme {
    let mut theme = Theme::default();
    for (token, value) in palette_styles.iter().chain(config_styles.iter()) {
        let _ = theme.set_token(token, value);
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

        theme.set_token("number", "bright-black").unwrap();
        theme.set_token("percent", "dim").unwrap();
        let non_defaults = theme.non_default_tokens();
        assert_eq!(2, non_defaults.len());
        assert!(non_defaults.contains(&("number", "bright-black".to_owned())));
        assert!(non_defaults.contains(&("percent", "default dim".to_owned())));
    }

    #[test]
    fn a_plain_style_adds_no_escape_codes() {
        assert_eq!("Total", Style::plain().paint("Total").to_string());
    }

    #[test]
    fn parses_a_palette_file() {
        let palette = Palette::parse("# mezura-format: 2\nlanguages = cyan bright-magenta 6ad9bd\n\nlabel = b5a98a italic\nnumber = dim\n").unwrap();
        assert_eq!(Some(vec![Color::Cyan, Color::BrightMagenta, Color::TrueColor{r:106,g:217,b:189}]), palette.languages);
        assert_eq!(vec![("label".to_owned(), "b5a98a italic".to_owned()), ("number".to_owned(), "dim".to_owned())], palette.styles);
    }

    #[test]
    fn rejects_a_malformed_palette_file() {
        assert_eq!(Err(ThemeParseError::MalformedLine("cyan magenta".to_owned())), Palette::parse("cyan magenta"));
        assert_eq!(Err(ThemeParseError::UnknownToken("labell".to_owned())), Palette::parse("labell = cyan"));
        assert_eq!(Err(ThemeParseError::InvalidValue("label".to_owned(), "nope".to_owned())), Palette::parse("label = nope"));
        assert_eq!(Err(ThemeParseError::InvalidValue("languages".to_owned(), "nope".to_owned())), Palette::parse("languages = nope"));
    }

    #[test]
    fn detects_and_converts_the_legacy_format() {
        let legacy = "cyan bright-magenta bright-yellow 6ad9bd d7c9f0";
        assert!(Palette::is_in_legacy_format(legacy));
        assert!(!Palette::is_in_legacy_format("# mezura-format: 2\nlanguages = cyan"));
        assert!(!Palette::is_in_legacy_format("languages = cyan"));
        assert!(!Palette::is_in_legacy_format(""));

        // A legacy palette whose first color carries the optional '#' must not be mistaken for a
        // comment, otherwise it silently loads as an empty palette instead of being converted
        let hex_prefixed = "#e80000 #00f2b6 #99ff00 #ffe5a3";
        assert!(Palette::is_in_legacy_format(hex_prefixed));
        assert_eq!(4, Palette::from_legacy_format(hex_prefixed).unwrap().languages.unwrap().len());
        assert_eq!(Err(ThemeParseError::EmptyPalette), Palette::parse(hex_prefixed));
        assert_eq!(Err(ThemeParseError::EmptyPalette), Palette::parse("# just a comment\n\n"));

        let converted = Palette::from_legacy_format(legacy).unwrap();
        assert_eq!(5, converted.languages.as_ref().unwrap().len());
        assert!(converted.styles.is_empty());

        let rewritten = converted.to_file_contents();
        assert!(rewritten.starts_with(FORMAT_MARKER));
        assert_eq!(converted, Palette::parse(&rewritten).unwrap());
    }

    #[test]
    fn a_palette_file_round_trips() {
        let original = Palette::parse("languages = cyan d7c9f0\nheading = white bold\nkeyword = bright-yellow italic\n").unwrap();
        assert_eq!(original, Palette::parse(&original.to_file_contents()).unwrap());
    }

    #[test]
    fn the_config_layer_wins_over_the_palette_layer() {
        let palette = [("label".to_owned(), "cyan".to_owned()), ("number".to_owned(), "dim".to_owned())];
        let config = [("label".to_owned(), "bright-red bold".to_owned())];

        let theme = resolve(&palette, &config);
        assert_eq!(Style::of(Color::BrightRed).bold(), theme.label);
        assert_eq!(Style::plain().dim(), theme.number);
        assert_eq!(Theme::default().heading, theme.heading);
    }
}
