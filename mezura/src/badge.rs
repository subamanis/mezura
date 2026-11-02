use mezura_core::{RunResult, Stats, TestCode};
use mezura_core::render::{calculate_percentage, calculate_percentages_of_their_own_sum};

use super::config_manager::Configuration;
use super::number_formatter::{format_with_decimal_separator, format_with_separators, get_active};
use super::result_printer::get_sorted_language_names;

const BADGE_LABEL : &str = "mezura";
const BADGE_FIGURES : [BadgeFigure; 6] = [
    BadgeFigure::Lines, BadgeFigure::Code, BadgeFigure::Comments, BadgeFigure::Tests,
    BadgeFigure::Files, BadgeFigure::Languages
];
const PILL_HEIGHT : usize = 20;
const PILL_PADDING : f64 = 5.0;
const PILL_FONT_SIZE : f64 = 11.0;
const PILL_LABEL_FILL : &str = "#555555";
const PILL_LABEL_TEXT : &str = "#ffffff";
const PILL_VALUE_FILL : &str = "#6ad9bd";
const PILL_VALUE_TEXT : &str = "#0b3b31";
const PILL_DIVIDER : &str = "#3fb79a";
const CARD_WIDTH : usize = 300;
const CARD_INSET : usize = 16;
const CARD_FONT_SIZE : f64 = 12.0;
const CARD_FOOTER_FONT_SIZE : f64 = 11.0;
const CARD_TITLE_BASELINE : usize = 26;
const CARD_BAR_TOP : usize = 36;
const CARD_BAR_HEIGHT : usize = 8;
const CARD_LIST_TOP : usize = 64;
const CARD_ROW_HEIGHT : usize = 18;
const CARD_LANGUAGES : usize = 6;
const CARD_NAME_LIMIT : usize = 14;
const CARD_STYLE : &str = ".bg{fill:#ffffff}.bd{stroke:#d0d7de}.fg{fill:#1f2328}.mu{fill:#656d76}.ot{fill:#8b949e}\
        .tr{fill:#eaeef2}@media(prefers-color-scheme:dark){.bg{fill:#0d1117}.bd{stroke:#30363d}.fg{fill:#e6edf3}\
        .mu{fill:#8b949e}.ot{fill:#484f58}.tr{fill:#21262d}}";
const OTHER_NAME : &str = "Other";
const OTHER_FILL : &str = "url(#o)";
const UNKNOWN_LANGUAGE_COLOR : &str = "#9e9e9e";
// Linguist's colors
const LANGUAGE_COLORS : [(&str, &str); 85] = [
    ("Ada", "#02f88c"), ("Assembly", "#6E4C13"), ("Astro", "#ff5a03"), ("Batch", "#C1F12E"),
    ("C Header", "#555555"), ("C#", "#178600"), ("C++ Header", "#f34b7d"), ("C++", "#f34b7d"), ("C", "#555555"),
    ("C3", "#aa23ef"), ("CMake", "#DA3434"), ("COBOL", "#005ca5"), ("CSS", "#663399"), ("Carbon", "#d55e22"),
    ("Clojure", "#db5855"), ("CoffeeScript", "#244776"), ("D", "#ba595e"), ("Dart", "#00B4AB"),
    ("Delphi", "#E3F171"), ("Dockerfile", "#384d54"), ("Elixir", "#6e4a7e"), ("Elm", "#60B5CC"),
    ("Emacs Lisp", "#c065db"), ("Erlang", "#B83998"), ("F#", "#b845fc"), ("Fortran", "#4d41b1"),
    ("GDScript", "#355570"), ("GLSL", "#5686a5"), ("Go", "#00ADD8"), ("GraphQL", "#e10098"),
    ("Groovy", "#4298b8"), ("HCL", "#844FBA"), ("HLSL", "#aace60"), ("HTML", "#e34c26"), ("Haskell", "#5e5086"),
    ("JAI", "#ab8b4b"), ("JSX", "#f1e05a"), ("Java", "#b07219"), ("JavaScript", "#f1e05a"), ("Julia", "#a270ba"),
    ("Kotlin", "#A97BFF"), ("Less", "#1d365d"), ("Lisp", "#3fb68b"), ("Lua", "#000080"), ("MATLAB", "#e16737"),
    ("Make", "#427819"), ("Mercury", "#ff2b2b"), ("Metal", "#8f14e9"), ("Nix", "#7e7eff"), ("OCaml", "#ef7a08"),
    ("Objective-C", "#438eff"), ("Odin", "#60AFFE"), ("PHP", "#4F5D95"), ("Pascal", "#E3F171"), ("Perl", "#0298c3"),
    ("PowerShell", "#012456"), ("Processing", "#0096D8"), ("Prolog", "#74283c"), ("Protocol Buffers", "#4a6ea9"),
    ("Python", "#3572A5"), ("R", "#198CE7"), ("Ruby", "#701516"), ("Rust", "#dea584"), ("SCSS", "#c6538c"),
    ("SQL", "#e38c00"), ("Scala", "#c22d40"), ("Scheme", "#1e4aec"), ("ShaderLab", "#222c37"), ("Shell", "#89e051"),
    ("Slang", "#1fbec9"), ("Smalltalk", "#596706"), ("Solidity", "#AA6746"), ("Svelte", "#ff3e00"),
    ("Swift", "#F05138"), ("SystemVerilog", "#DAE1C2"), ("TSX", "#3178c6"), ("TypeScript", "#3178c6"),
    ("VHDL", "#adb2cb"), ("Verilog", "#b2b7f8"), ("Vim Script", "#199f4b"), ("Visual Basic", "#945db7"),
    ("Vue", "#41b883"), ("WAT", "#04133b"), ("WGSL", "#1a5e9a"), ("Zig", "#ec915c")
];
// Verdana's advance widths in em. 'textLength' on every text of the pill fits the glyphs to the
// figure, so a width here that is a little off stretches the letters a little and never the layout.
const VERDANA_WIDTHS : [(char, f64); 66] = [
    (' ', 0.352), ('.', 0.364), (',', 0.364), ('%', 1.076), ('0', 0.636), ('1', 0.636), ('2', 0.636),
    ('3', 0.636), ('4', 0.636), ('5', 0.636), ('6', 0.636), ('7', 0.636), ('8', 0.636), ('9', 0.636),
    ('a', 0.601), ('b', 0.623), ('c', 0.521), ('d', 0.623), ('e', 0.596), ('f', 0.352), ('g', 0.623),
    ('h', 0.633), ('i', 0.274), ('j', 0.344), ('k', 0.592), ('l', 0.274), ('m', 0.973), ('n', 0.633),
    ('o', 0.607), ('p', 0.623), ('q', 0.623), ('r', 0.427), ('s', 0.521), ('t', 0.394), ('u', 0.633),
    ('v', 0.592), ('w', 0.818), ('x', 0.592), ('y', 0.592), ('z', 0.525), ('A', 0.684), ('B', 0.686),
    ('C', 0.698), ('D', 0.771), ('E', 0.632), ('F', 0.575), ('G', 0.775), ('H', 0.751), ('I', 0.421),
    ('J', 0.455), ('K', 0.693), ('L', 0.557), ('M', 0.843), ('N', 0.748), ('O', 0.787), ('P', 0.603),
    ('Q', 0.787), ('R', 0.695), ('S', 0.684), ('T', 0.616), ('U', 0.732), ('V', 0.684), ('W', 0.989),
    ('X', 0.685), ('Y', 0.615), ('Z', 0.685)
];
const UNLISTED_GLYPH_WIDTH : f64 = 0.65;

// What '--badge' asked for: the shape, and the figures in the order they were written, none when
// none was
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadgeSpec {
    shape: BadgeShape,
    figures: Vec<BadgeFigure>
}

impl BadgeSpec {
    pub fn of(shape: BadgeShape) -> BadgeSpec {
        BadgeSpec { shape, figures: Vec::new() }
    }

    // The words in any order, separated by spaces or commas. The shape is a word like the others.
    pub fn parse(words: &str) -> Result<BadgeSpec, String> {
        let mut shape = BadgeShape::Pill;
        let mut figures = Vec::new();
        for word in words.split([',', ' ', '\t']).map(str::trim).filter(|x| !x.is_empty()) {
            match (BadgeShape::parse(word), BadgeFigure::parse(word)) {
                (Some(found), _) => shape = found,
                (_, Some(figure)) => {
                    if !figures.contains(&figure) {
                        figures.push(figure);
                    }
                },
                _ => return Err(word.to_owned())
            }
        }

        Ok(BadgeSpec { shape, figures })
    }

    pub fn names_tests(&self) -> bool {
        self.figures.contains(&BadgeFigure::Tests)
    }

    fn get_figures_to_draw(&self) -> &[BadgeFigure] {
        if self.figures.is_empty() {self.shape.get_default_figures()} else {&self.figures}
    }
}

pub fn print_badge(result: &RunResult, config: &Configuration) {
    if let Some(spec) = &config.view.badge {
        outln!("{}", render_badge(result, config, spec));
    }
}

pub fn render_badge(result: &RunResult, config: &Configuration, spec: &BadgeSpec) -> String {
    let figures = collect_figures(result, config, spec);
    match spec.shape {
        BadgeShape::Pill => render_pill(&figures),
        BadgeShape::Card => render_card(result, config, &figures)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeShape {
    Pill,
    Card
}

impl BadgeShape {
    fn parse(word: &str) -> Option<BadgeShape> {
        [Self::Pill, Self::Card].into_iter().find(|shape| shape.as_str().eq_ignore_ascii_case(word))
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Pill => "pill",
            Self::Card => "card"
        }
    }

    fn get_default_figures(self) -> &'static [BadgeFigure] {
        match self {
            Self::Pill => &[BadgeFigure::Code],
            Self::Card => &[BadgeFigure::Code, BadgeFigure::Comments, BadgeFigure::Tests, BadgeFigure::Languages]
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BadgeFigure {
    Lines,
    Code,
    Comments,
    Tests,
    Files,
    Languages
}

impl BadgeFigure {
    fn parse(word: &str) -> Option<BadgeFigure> {
        BADGE_FIGURES.into_iter().find(|figure| figure.as_str().eq_ignore_ascii_case(word))
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Lines => "lines",
            Self::Code => "code",
            Self::Comments => "comments",
            Self::Tests => "tests",
            Self::Files => "files",
            Self::Languages => "languages"
        }
    }
}

struct DrawnFigure {
    figure: BadgeFigure,
    amount: Amount
}

impl DrawnFigure {
    fn format(&self, format_count: fn(usize) -> String) -> String {
        let amount = match self.amount {
            Amount::Count(count) => format_count(count),
            Amount::Share(percent) => format!("{}%", get_active().format_percent(percent))
        };

        format!("{amount} {}", self.figure.as_str())
    }

    fn format_short(&self) -> String {
        self.format(format_short_number)
    }

    fn format_whole(&self) -> String {
        self.format(format_with_separators)
    }
}

enum Amount {
    Count(usize),
    Share(f64)
}

// A tests figure that was not asked for is left out at zero
fn collect_figures(result: &RunResult, config: &Configuration, spec: &BadgeSpec) -> Vec<DrawnFigure> {
    let model = config.view.counting;
    let tests_lines = TestCode::calculate_total_of(&result.tests).stats.lines;
    spec.get_figures_to_draw().iter().filter_map(|figure| {
        let amount = match figure {
            BadgeFigure::Lines => Amount::Count(result.total.lines),
            BadgeFigure::Code => Amount::Count(result.total.calculate_code_lines(model)),
            BadgeFigure::Comments => Amount::Count(result.total.calculate_comment_lines(model)),
            BadgeFigure::Tests => Amount::Share(calculate_percentage(tests_lines, result.total.lines)),
            BadgeFigure::Files => Amount::Count(result.total.files),
            BadgeFigure::Languages => Amount::Count(result.per_language.len())
        };
        if *figure == BadgeFigure::Tests && tests_lines == 0 && !spec.names_tests() {
            return None;
        }
        Some(DrawnFigure { figure: *figure, amount })
    }).collect()
}

fn render_pill(figures: &[DrawnFigure]) -> String {
    let label_text_width = measure_pill_text(BADGE_LABEL);
    let texts = figures.iter().map(DrawnFigure::format_short).collect::<Vec<_>>();
    let text_widths = texts.iter().map(|text| measure_pill_text(text)).collect::<Vec<_>>();
    let label_cell_width = calculate_pill_cell_width(label_text_width);
    let cell_widths = text_widths.iter().map(|width| calculate_pill_cell_width(*width)).collect::<Vec<_>>();
    let total_width = label_cell_width + cell_widths.iter().sum::<usize>();
    let title = escape_xml(&format!("{BADGE_LABEL}: {}", texts.join(", ")));

    let mut svg = String::new();
    svg.push_str(&format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{total_width}\" height=\"{PILL_HEIGHT}\" \
            role=\"img\" aria-label=\"{title}\"><title>{title}</title>"));
    svg.push_str(&format!("<clipPath id=\"r\"><rect width=\"{total_width}\" height=\"{PILL_HEIGHT}\" rx=\"3\"/></clipPath>"));
    svg.push_str("<g clip-path=\"url(#r)\">");
    svg.push_str(&format!("<rect width=\"{label_cell_width}\" height=\"{PILL_HEIGHT}\" fill=\"{PILL_LABEL_FILL}\"/>"));
    svg.push_str(&format!("<rect x=\"{label_cell_width}\" width=\"{}\" height=\"{PILL_HEIGHT}\" fill=\"{PILL_VALUE_FILL}\"/>",
            total_width - label_cell_width));
    let mut x = label_cell_width;
    for width in &cell_widths[..cell_widths.len().saturating_sub(1)] {
        x += width;
        svg.push_str(&format!("<line x1=\"{x}\" y1=\"0\" x2=\"{x}\" y2=\"{PILL_HEIGHT}\" stroke=\"{PILL_DIVIDER}\"/>"));
    }
    svg.push_str("</g>");
    svg.push_str(&format!("<g font-family=\"Verdana,Geneva,DejaVu Sans,sans-serif\" font-size=\"{PILL_FONT_SIZE}\" \
            text-anchor=\"middle\">"));
    write_pill_text(&mut svg, BADGE_LABEL, label_cell_width as f64 / 2.0, label_text_width, PILL_LABEL_TEXT);
    let mut x = label_cell_width;
    for ((text, text_width), cell_width) in texts.iter().zip(&text_widths).zip(&cell_widths) {
        write_pill_text(&mut svg, text, x as f64 + *cell_width as f64 / 2.0, *text_width, PILL_VALUE_TEXT);
        x += cell_width;
    }
    svg.push_str("</g></svg>");

    svg
}

fn write_pill_text(svg: &mut String, text: &str, center: f64, width: f64, fill: &str) {
    svg.push_str(&format!("<text x=\"{}\" y=\"14\" fill=\"{fill}\" textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\">{}</text>",
            format_with_one_decimal(center), format_with_one_decimal(width), escape_xml(text)));
}

fn calculate_pill_cell_width(text_width: f64) -> usize {
    (text_width + 2.0 * PILL_PADDING).ceil() as usize
}

// The first figure is the headline beside the title, the rest make the footer
fn render_card(result: &RunResult, config: &Configuration, figures: &[DrawnFigure]) -> String {
    let headline = figures.first();
    let footer = figures.get(1..).unwrap_or_default();
    let languages = collect_card_languages(result, config);
    let inner_width = CARD_WIDTH - 2 * CARD_INSET;
    let footer_lines = wrap_footer(&footer.iter().map(DrawnFigure::format_whole).collect::<Vec<_>>(), inner_width as f64);
    let rows = languages.len().div_ceil(2);
    // Baselines. A row's descenders reach 4 under it, and the footer sits a row under whatever
    // came last.
    let after_list = if rows == 0 {CARD_BAR_TOP + CARD_BAR_HEIGHT} else {CARD_LIST_TOP + CARD_ROW_HEIGHT * (rows - 1) + 4};
    let first_footer = after_list + CARD_ROW_HEIGHT;
    let height = match footer_lines.len() {
        0 => after_list + 12,
        count => first_footer + CARD_ROW_HEIGHT * (count - 1) + 14
    };
    let title = escape_xml(&format!("{BADGE_LABEL}: {}", figures.iter().map(DrawnFigure::format_whole).collect::<Vec<_>>().join(", ")));

    let mut svg = String::new();
    svg.push_str(&format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{CARD_WIDTH}\" height=\"{height}\" \
            role=\"img\" aria-label=\"{title}\"><title>{title}</title><style>{CARD_STYLE}</style>"));
    svg.push_str("<pattern id=\"o\" width=\"4\" height=\"4\" patternUnits=\"userSpaceOnUse\" patternTransform=\"rotate(45)\">\
            <rect width=\"4\" height=\"4\" class=\"tr\"/><rect width=\"2\" height=\"4\" class=\"ot\"/></pattern>");
    svg.push_str(&format!("<rect x=\"0.5\" y=\"0.5\" width=\"{}\" height=\"{}\" rx=\"6\" class=\"bg bd\"/>",
            CARD_WIDTH - 1, height - 1));
    svg.push_str(&format!("<g font-family=\"-apple-system,BlinkMacSystemFont,Segoe UI,Helvetica,Arial,sans-serif\" \
            font-size=\"{CARD_FONT_SIZE}\">"));
    svg.push_str(&format!("<text x=\"{CARD_INSET}\" y=\"{CARD_TITLE_BASELINE}\" class=\"mu\">{BADGE_LABEL}</text>"));
    if let Some(figure) = headline {
        svg.push_str(&format!("<text x=\"{}\" y=\"{CARD_TITLE_BASELINE}\" text-anchor=\"end\" font-weight=\"500\" class=\"fg\">{}</text>",
                CARD_WIDTH - CARD_INSET, escape_xml(&figure.format_whole())));
    }
    write_card_bar(&mut svg, &languages, inner_width);
    let column_width = inner_width / 2;
    for (at, language) in languages.iter().enumerate() {
        let column_start = CARD_INSET + (at % 2) * (column_width + 4);
        let baseline = CARD_LIST_TOP + CARD_ROW_HEIGHT * (at / 2);
        svg.push_str(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"4\" fill=\"{}\"/>", column_start + 5, baseline - 4, language.fill));
        svg.push_str(&format!("<text x=\"{}\" y=\"{baseline}\" class=\"fg\">{}</text>", column_start + 14,
                escape_xml(&shorten_name(&language.name))));
        svg.push_str(&format!("<text x=\"{}\" y=\"{baseline}\" text-anchor=\"end\" class=\"mu\">{}%</text>",
                column_start + column_width - 4, get_active().format_percent(language.share)));
    }
    for (at, line) in footer_lines.iter().enumerate() {
        svg.push_str(&format!("<text x=\"{CARD_INSET}\" y=\"{}\" font-size=\"{CARD_FOOTER_FONT_SIZE}\" class=\"mu\">{}</text>",
                first_footer + CARD_ROW_HEIGHT * at, escape_xml(line)));
    }
    svg.push_str("</g></svg>");

    svg
}

struct CardLanguage {
    name: String,
    share: f64,
    fill: &'static str
}

// Sorted as the report is, cut to '--top' or the card's own cap, and whatever is past the cut folded
// into one row, unless that row would hold a single language. The shares are the overview's, so
// the last row takes the rounding and the list adds up to 100.
fn collect_card_languages(result: &RunResult, config: &Configuration) -> Vec<CardLanguage> {
    let names = get_sorted_language_names(&result.per_language, config.view.sort_by, config.view.counting);
    let to_keep = config.view.top_n.unwrap_or(CARD_LANGUAGES);
    let (kept, folded) = if names.len() <= to_keep + 1 {(&names[..], &[][..])} else {names.split_at(to_keep)};
    let count_lines_of = |name: &String| result.per_language.get(name).map_or(0, |stats: &Stats| stats.lines);
    let mut rows = kept.iter().map(|name| (name.as_str(), find_language_color(name), count_lines_of(name)))
            .collect::<Vec<_>>();
    if !folded.is_empty() {
        rows.push((OTHER_NAME, OTHER_FILL, folded.iter().map(count_lines_of).sum()));
    }
    let shares = calculate_percentages_of_their_own_sum(&rows.iter().map(|(_, _, lines)| *lines).collect::<Vec<_>>());

    rows.into_iter().zip(shares).map(|((name, fill, _), share)| CardLanguage { name: name.to_owned(), share, fill })
            .collect()
}

fn write_card_bar(svg: &mut String, languages: &[CardLanguage], inner_width: usize) {
    svg.push_str(&format!("<clipPath id=\"b\"><rect x=\"{CARD_INSET}\" y=\"{CARD_BAR_TOP}\" width=\"{inner_width}\" \
            height=\"{CARD_BAR_HEIGHT}\" rx=\"4\"/></clipPath>"));
    svg.push_str("<g clip-path=\"url(#b)\">");
    svg.push_str(&format!("<rect x=\"{CARD_INSET}\" y=\"{CARD_BAR_TOP}\" width=\"{inner_width}\" height=\"{CARD_BAR_HEIGHT}\" \
            class=\"tr\"/>"));
    let mut x = CARD_INSET as f64;
    for language in languages {
        let width = inner_width as f64 * language.share / 100.0;
        svg.push_str(&format!("<rect x=\"{}\" y=\"{CARD_BAR_TOP}\" width=\"{}\" height=\"{CARD_BAR_HEIGHT}\" fill=\"{}\"/>",
                format_with_one_decimal(x), format_with_one_decimal(width), language.fill));
        x += width;
    }
    svg.push_str("</g>");
}

fn find_language_color(name: &str) -> &'static str {
    LANGUAGE_COLORS.iter().find(|(known, _)| *known == name).map_or(UNKNOWN_LANGUAGE_COLOR, |(_, color)| color)
}

// Measured in Verdana, the widest of the fonts the card names, so the footer wraps early and never
// overflows
fn wrap_footer(texts: &[String], width: f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for text in texts {
        match lines.last_mut() {
            Some(line) if measure_text(&format!("{line} · {text}"), CARD_FOOTER_FONT_SIZE) <= width => {
                line.push_str(" · ");
                line.push_str(text);
            },
            _ => lines.push(text.clone())
        }
    }

    lines
}

fn shorten_name(name: &str) -> String {
    match name.chars().count() > CARD_NAME_LIMIT {
        true => name.chars().take(CARD_NAME_LIMIT - 1).collect::<String>() + "…",
        false => name.to_owned()
    }
}

fn format_short_number(value: usize) -> String {
    match value {
        0..=999 => value.to_string(),
        1_000..=999_949 => format_with_decimal_separator(format_with_one_decimal(value as f64 / 1_000.0)) + "k",
        _ => format_with_decimal_separator(format_with_one_decimal(value as f64 / 1_000_000.0)) + "M"
    }
}

fn measure_pill_text(text: &str) -> f64 {
    measure_text(text, PILL_FONT_SIZE)
}

fn measure_text(text: &str, font_size: f64) -> f64 {
    text.chars().map(|glyph| VERDANA_WIDTHS.iter().find(|(known, _)| *known == glyph)
            .map_or(UNLISTED_GLYPH_WIDTH, |(_, width)| *width)).sum::<f64>() * font_size
}

// '12.3' and '12', never '12.0'
fn format_with_one_decimal(value: f64) -> String {
    let text = format!("{value:.1}");
    match text.strip_suffix(".0") {
        Some(whole) => whole.to_owned(),
        None => text
    }
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mezura_core::{SortCriterion, Target};

    use super::*;
    use crate::test_support::{plain_result_of, plain_stats_of};

    #[test]
    fn the_words_name_a_shape_and_the_figures_in_their_order_and_nothing_else() {
        let typed = BadgeSpec::parse("card, tests code").unwrap();
        assert_eq!(BadgeShape::Card, typed.shape);
        assert_eq!(vec![BadgeFigure::Tests, BadgeFigure::Code], typed.figures);
        assert!(typed.names_tests());

        let bare = BadgeSpec::parse("").unwrap();
        assert!(bare.shape == BadgeShape::Pill && bare.figures.is_empty() && !bare.names_tests());
        assert_eq!([BadgeFigure::Code], bare.get_figures_to_draw());
        assert_eq!([BadgeFigure::Code, BadgeFigure::Comments, BadgeFigure::Tests, BadgeFigure::Languages],
                BadgeSpec::parse("CARD").unwrap().get_figures_to_draw());
        assert_eq!(vec![BadgeFigure::Lines], BadgeSpec::parse("lines lines").unwrap().figures, "a repeated word drew twice");
        assert_eq!(Err("keywords".to_owned()), BadgeSpec::parse("code keywords"));
    }

    #[test]
    fn a_short_number_keeps_one_decimal_and_never_a_trailing_zero() {
        assert_eq!(["0", "999", "1k", "1.5k", "9.9k", "10k", "123.5k", "999.5k", "999.9k", "1M", "1M", "1.2M", "12M"],
                [0, 999, 1000, 1549, 9949, 10_000, 123_456, 999_499, 999_949, 999_950, 1_000_000, 1_250_000, 12_000_000]
                        .map(format_short_number));
    }

    // What the golden does not hold: a cut typed with '--top', and a language the table has no
    // color for, which is painted plain grey and never with the hatch of the folded row
    #[test]
    fn the_card_cuts_to_the_top_asked_for_and_paints_an_unknown_language_grey() {
        let eight = [("Rust", 800), ("Go", 700), ("Python", 600), ("Java", 500), ("Shell", 400), ("C", 300),
                ("Zig", 200), ("Lua", 100)];
        let mut config = Configuration::new(vec!["./".to_owned()]);
        config.view.sort_by = SortCriterion::Lines;
        config.view.top_n = Some(2);
        let card = BadgeSpec::parse("card").unwrap();

        let drawn = render_badge(&result_of(&eight), &config, &card);
        assert!(drawn.contains(">Go<") && !drawn.contains(">Python<") && drawn.contains(">Other<"), "{drawn}");

        let unknown = render_badge(&result_of(&[("Brainfuck", 10), ("Rust", 5)]), &config, &card);
        assert!(unknown.contains(UNKNOWN_LANGUAGE_COLOR) && !unknown.contains(OTHER_FILL), "{unknown}");
    }

    // Adding a language costs one file and no test code, and without this the new one would draw
    // grey on every card with nothing failing
    #[test]
    fn every_shipped_language_has_a_color() {
        let (languages, _) = mezura_core::language_file::parse_languages_in_dir(crate::paths::test_paths::CORE_LANGUAGES_DIR).unwrap();
        let uncolored = languages.iter().map(|language| language.name.as_str())
                .filter(|name| find_language_color(name) == UNKNOWN_LANGUAGE_COLOR).collect::<Vec<_>>();
        assert!(uncolored.is_empty(), "languages the card has no color for: {uncolored:?}");
    }

    #[test]
    fn every_badge_matches_the_golden_file() {
        crate::test_support::assert_matches_golden("badges.golden", &render_every_badge(), "every_badge");
    }

    fn render_every_badge() -> String {
        let mut result = result_of(&[("Rust", 7700), ("TypeScript", 2700), ("Python", 1100), ("Shell", 500),
                ("Protocol Buffers", 200), ("Lua", 100), ("Zig", 45)]);
        result.tests.insert("Rust".to_owned(), TestCode { stats: plain_stats_of(12, 40_000, 2800, 2500, 100, HashMap::new()),
                whole_files: 12 });
        let mut config = Configuration::new(vec!["./".to_owned()]);
        config.view.sort_by = SortCriterion::Lines;
        let specs = ["", "lines code comments tests files languages", "card", "card code tests", "card lines",
                "card lines code comments tests files languages"];
        let mut rendered = String::new();
        for spec in specs {
            rendered.push_str(&format!("{}\n{}\n\n", format!("--badge {spec}").trim_end(),
                    render_badge(&result, &config, &BadgeSpec::parse(spec).unwrap())));
        }
        let card = BadgeSpec::parse("card").unwrap();
        let mut nine = result.per_language.clone();
        nine.insert("Brainfuck".to_owned(), plain_stats_of(1, 300, 30, 25, 2, HashMap::new()));
        nine.insert("Go".to_owned(), plain_stats_of(2, 900, 90, 70, 5, HashMap::new()));
        rendered.push_str(&format!("--badge card over nine languages\n{}\n\n", render_badge(&plain_result_of(nine, Vec::new(),
                vec![Target::of("./")]), &config, &card)));
        let nothing = plain_result_of(HashMap::new(), Vec::new(), vec![Target::of("./")]);
        rendered.push_str(&format!("--badge card over nothing\n{}\n\n", render_badge(&nothing, &config, &card)));
        rendered.push_str(&format!("--badge tests over nothing\n{}\n", render_badge(&nothing, &config,
                &BadgeSpec::parse("tests").unwrap())));

        rendered
    }

    fn result_of(languages: &[(&str, usize)]) -> RunResult {
        let per_language = languages.iter().map(|(name, lines)| (name.to_string(),
                plain_stats_of(lines / 100 + 1, lines * 30, *lines, lines * 8 / 10, lines / 10, HashMap::new())))
                .collect::<HashMap<_, _>>();

        plain_result_of(per_language, Vec::new(), vec![Target::of("./")])
    }
}
