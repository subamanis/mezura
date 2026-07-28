use std::cmp::max;

use crate::*;

type ColorFunc = Box<dyn Fn(&str) -> String>;

//the total number of vertical lines ( | ) that appear in the [-|||...|-] in the overview section
const NUM_OF_VERTICALS : usize = 50;

//the number of languages the overview shows before folding the rest into "others"
const OVERVIEW_LANGUAGES : usize = 3;

const DEFAULT_LANGUAGE_COLORS : [Color; 4] = [Color::Cyan, Color::BrightMagenta, Color::BrightYellow,
        Color::TrueColor{r:106,g:217,b:189}];
const DEFAULT_OTHERS_COLOR : Color = Color::TrueColor{r:215,g:201,b:240};

//a language that is present but whose share rounds away to zero: shown as "<0.01", given no cell
const PRESENT_BUT_TINY : f64 = 0.001;

const KEYWORD_LINE_OFFSET : usize = 19;
const STANDARD_LINE_STATS_LEN : usize = 33;

//log file keys
const FILES         : &str  = "Files:";
const LINES         : &str  = "Lines:";
const CODE          : &str  = "Code:";
const EXTRA         : &str  = "Extra:";
const TOTAL_SIZE    : &str  = "Total Size:";
const AVERAGE_SIZE  : &str  = "Average Size:";

pub fn format_and_print_results(content_info_map: &mut HashMap<String, LanguageContentInfo>, languages_metadata_map: &mut HashMap<String, LanguageMetadata>,
        final_stats: &FinalStats, existing_log_content: &Option<String>, datetime_now: &DateTime<Local>, config: &Configuration) 
{
    let mut sorted_language_names = get_sorted_language_names(content_info_map, languages_metadata_map, config.sort_by);

    // The list is cut, but the total below it still counts everything, so the reader is told what
    // is missing rather than left to wonder why the rows do not add up
    let hidden_languages = config.top_n.map_or(0, |top| sorted_language_names.len().saturating_sub(top));
    let shown_language_names = &sorted_language_names[..sorted_language_names.len() - hidden_languages];

    let biggest_prefix_standard_spaces = get_biggest_prefix_standard_spaces(shown_language_names, languages_metadata_map);
    let should_print_keywords = !config.hidden.keywords;

    if !config.hidden.details {
        print_individually(shown_language_names, content_info_map, languages_metadata_map,
                biggest_prefix_standard_spaces, should_print_keywords);
        if hidden_languages > 0 {
            let plural = if hidden_languages == 1 {"language"} else {"languages"};
            println!("\n{}", theme::active().summary.paint(&format!("(+{hidden_languages} more {plural} hidden by --top {})", config.top_n.unwrap())));
        }
    }

    if languages_metadata_map.len() > 1 {
        if !config.hidden.details {
            print_sum(content_info_map, final_stats, biggest_prefix_standard_spaces, should_print_keywords);
        }
        if !config.hidden.overview {
            print_visual_overview(&mut sorted_language_names, content_info_map, languages_metadata_map, final_stats, config);
        }
    }

    if !config.hidden.progress && let Some(content) = existing_log_content && config.compare_level != 0 {
        print_comparison_to_previous_runs(final_stats, content,  config.compare_level, datetime_now);
    }
}


fn print_individually(sorted_languages: &[String], content_info_map: &HashMap<String,LanguageContentInfo>,
     languages_metadata_map: &HashMap<String, LanguageMetadata>, biggest_prefix_standard_spaces: usize, should_print_keywords: bool)
{
    fn get_size_text(metadata: &LanguageMetadata) -> String {
        let (size, size_desc) = get_size_and_formatted_size_text(metadata.bytes, "total");
        let (average_size, average_size_desc) = get_size_and_formatted_size_text(
                metadata.bytes / metadata.files, "average");

        format!("{size:.1} {size_desc} - {average_size:.1} {average_size_desc}")
    }

    fn reconstruct_line(i: usize, max_line_stats_len: usize, titles_vec: &[String], lines_stats_vec: &[String],
         lines_stats_len_vec: &[usize], size_stats_vec: &[String], keywords_stats_vec: &[String]) -> String
    {
        let spaces = max_line_stats_len+1 - lines_stats_len_vec[i];
        let mut line = titles_vec[i].clone() + &lines_stats_vec[i] + &" ".repeat(spaces) + " |  " + &size_stats_vec[i];
        //if run with --hide keywords
        if !keywords_stats_vec.is_empty(){
            line = line + "\n" + &keywords_stats_vec[i];
        } 
        line
    }

    println!("{}.\n", theme::active().heading.paint("Details"));

    let mut max_line_stats_len = STANDARD_LINE_STATS_LEN;
    let (mut titles_vec, mut lines_stats_vec, mut lines_stats_len_vec, mut size_stats_vec,
            mut keywords_stats_vec) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());

    for lang_name in sorted_languages {
        let content_info = content_info_map.get(lang_name).unwrap();
        let metadata = languages_metadata_map.get(lang_name).unwrap();

        let files_str = with_seperators(metadata.files);
        let prefix_standard_spaces = lang_name.chars().count() + metadata.files.to_string().chars().count() +
                 utils::num_of_seperators(metadata.files); 
        let title = format!("{}   {}{} {}  -> ",theme::active().details_language.paint(lang_name),
                 " ".repeat(biggest_prefix_standard_spaces - prefix_standard_spaces), number(&files_str), colored_word("files"));
        titles_vec.push(title);

        let code_lines_percentage = if content_info.lines > 0 {content_info.code_lines as f64 / content_info.lines as f64 * 100f64} else {0f64};
        let lines_str = with_seperators(content_info.lines);
        let code_lines_str = with_seperators(content_info.code_lines);
        let extra_lines_str = with_seperators(content_info.lines - content_info.code_lines);
        let curr_line_stats_len = STANDARD_LINE_STATS_LEN + lines_str.len() + code_lines_str.len() + extra_lines_str.len();
        lines_stats_len_vec.push(curr_line_stats_len); 
        if max_line_stats_len < curr_line_stats_len {
            max_line_stats_len = curr_line_stats_len;
        }
        
        lines_stats_vec.push(format!("{} {} {{{} code ({}%) + {} extra}}", colored_word("lines"), number(&lines_str), number(&code_lines_str),
                 percent(code_lines_percentage), number(&extra_lines_str)));
        size_stats_vec.push(get_size_text(metadata));
        
        if should_print_keywords {
            keywords_stats_vec.push(get_keywords_as_str(&content_info.keyword_occurences, biggest_prefix_standard_spaces));
        }
    }

    for i in 0..lines_stats_vec.len() {
        let line = reconstruct_line(i, max_line_stats_len, &titles_vec, &lines_stats_vec,
                &lines_stats_len_vec, &size_stats_vec, &keywords_stats_vec);

        if i == lines_stats_len_vec.len() - 1 {
            println!("{line}");
        } else {
            println!("{line}\n");
        }
    }
}


// The text is already coloured by the time it gets measured, so the escape sequences have to be
// skipped: their bytes are in the string but not on the screen.
fn widest_visible_line(text: &str) -> usize {
    fn visible_len(line: &str) -> usize {
        let mut len = 0;
        let mut chars = line.chars();
        while let Some(character) = chars.next() {
            if character == '\x1b' {
                for terminator in chars.by_ref() {
                    if terminator == 'm' {break}
                }
            } else {
                len += 1;
            }
        }
        len
    }

    text.lines().map(visible_len).max().unwrap_or(0)
}


fn print_sum(content_info_map: &HashMap<String,LanguageContentInfo>, final_stats: &FinalStats, biggest_prefix_standard_spaces: usize,
        should_print_keywords: bool)
{
    let (total_files_str, total_lines_str, total_code_lines_str, total_extra_lines_str) =
            (with_seperators(final_stats.files),with_seperators(final_stats.lines),with_seperators(final_stats.code_lines), with_seperators(final_stats.extra_lines));

    let keywords_sum_map = create_keyword_sum_map(content_info_map);
    let keywords_line = get_keywords_as_str(&keywords_sum_map, biggest_prefix_standard_spaces);

    let spaces = biggest_prefix_standard_spaces - (5 + total_files_str.len());
    let title = format!("{}   {}{} {}  -> ",theme::active().details_total.paint("Total")," ".repeat(spaces),number(&total_files_str),colored_word("files"));
    let code_lines_percentage = if final_stats.lines > 0 {final_stats.code_lines as f64 / final_stats.lines as f64 * 100f64} else {0f64};
    let size_text = format!("{} {} - {} {}",number(&final_stats.size.to_string()), colored_word(&format!("{} total", final_stats.size_measurement)),
            number(&final_stats.average_size.to_string()),colored_word(&format!("{} average", final_stats.average_size_measurement)));

    let info = format!("{} {} {{{} code ({}%) + {} extra}}  |  {}\n",colored_word("lines"), number(&total_lines_str),number(&total_code_lines_str),
            percent(code_lines_percentage), number(&total_extra_lines_str), size_text);

    // The separator follows the total line, measured from the text that is actually printed. It
    // used to be a formula over some of the numbers plus two magic constants, which left out the
    // total line count, the width of the language name column and the size units, so it always
    // fell short. The keywords line is deliberately not measured, since it can be much wider than
    // the row it annotates.
    let line_len = widest_visible_line(&format!("{title}{info}"));
    println!("{} ",theme::active().separator.paint(&"-".repeat(line_len)));

    if should_print_keywords {
        println!("{title}{info}{keywords_line}\n");
    } else {
        println!("{title}{info}");
    }
}

//                                    OVERVIEW
//
// Files:    47% java - 32% cs - 21% py        [-||||||||||||||||||||||||||||||||||||||||||||||||||] 
//
// Lines: ...
//
// Size : ...
fn print_visual_overview(sorted_language_vec: &mut Vec<String>, content_info_map: &mut HashMap<String, LanguageContentInfo>,
        languages_metadata_map: &mut HashMap<String, LanguageMetadata>, final_stats: &FinalStats, config: &Configuration) 
{
    // The function itself decides whether there is anything to fold
    retain_most_relevant_and_add_others_field_for_rest(sorted_language_vec, content_info_map, languages_metadata_map, final_stats, config.top_n);

    println!("{}.\n", theme::active().heading.paint("Overview"));

    // 'others' takes its color by identity and not by position, because --top moves it: with
    // --top 2 it sits third and used to steal the color meant for the third language.
    // It claims the last color the palette actually declares, so it never shares one with a
    // language that is on screen next to it.
    let others_color = config.colors.get(4).or(config.colors.get(3)).copied().unwrap_or(DEFAULT_OTHERS_COLOR);
    let color_func_vec : Vec<ColorFunc> = sorted_language_vec.iter().enumerate().map(|(i, name)| {
            let color = if name == "others" {
                others_color
            } else {
                config.colors.get(i).copied().unwrap_or(DEFAULT_LANGUAGE_COLORS[i.min(DEFAULT_LANGUAGE_COLORS.len()-1)])
            };
            // The attributes come from the theme while the color stays per language, which is why
            // a color declared on 'overview-language' has nothing to apply to and is ignored
            let style = theme::active().overview_language.clone();
            Box::new(move |s: &str| style.paint_with_color(s, color).to_string()) as ColorFunc
        }).collect();

    let files_percentages = get_files_percentages(languages_metadata_map, sorted_language_vec);
    let lines_percentages = get_lines_percentages(content_info_map, sorted_language_vec);
    let sizes_percentages = get_sizes_percentages(languages_metadata_map, sorted_language_vec);

    let files_verticals = if config.hidden.bar {vec![]} else{get_num_of_verticals(&files_percentages, NUM_OF_VERTICALS)};
    let lines_verticals = if config.hidden.bar {vec![]} else{get_num_of_verticals(&lines_percentages, NUM_OF_VERTICALS)};
    let size_verticals = if config.hidden.bar {vec![]} else{get_num_of_verticals(&sizes_percentages, NUM_OF_VERTICALS)};

    let files_line = create_overview_line("Files:", &files_percentages, &files_verticals,
            sorted_language_vec, &color_func_vec, config);
    let lines_line = create_overview_line("Lines:", &lines_percentages, &lines_verticals,
            sorted_language_vec, &color_func_vec, config);
    let size_line = create_overview_line("Size :", &sizes_percentages, &size_verticals,
            sorted_language_vec, &color_func_vec, config);

    println!("{files_line}\n\n{lines_line}\n\n{size_line}\n");
}

fn print_comparison_to_previous_runs(final_stats: &FinalStats, log_content: &str, num_of_entries: usize, datetime_now: &DateTime<Local>) {
    println!("\n{}.\n", theme::active().heading.paint("Progress"));

    let log_entries = parse_N_previous_entries(log_content, num_of_entries);

    let mut comparison_str = String::with_capacity(200);
    for entry in log_entries.iter() {
        let duration = datetime_now.signed_duration_since(entry.datetime);
        let (days, hours, minutes) = split_minutes_to_D_H_M(duration.num_minutes());
        let arrow = theme::active().progress_entry.paint("->");
        if let Some(name) = &entry.name {
            comparison_str.push_str(&format!("{} \"{}\" ({} days, {} hours and {} minutes ago)\n",arrow, name, days, hours, minutes));
        } else {
            let then_str = entry.datetime.naive_local().to_string();
            comparison_str.push_str(&format!("{} {} ({} days, {} hours and {} minutes ago)\n",arrow, then_str, days, hours, minutes));
        }
        comparison_str.push_str(&format!("     Files: {}({}%) Lines: {}({}%) {{Code: {}({}%), Extra: {}({}%)}}\n\n",
                number(&with_seperators(entry.stats.files)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.files, final_stats.files)),
                number(&with_seperators(entry.stats.lines)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.lines, final_stats.lines)),
                number(&with_seperators(entry.stats.code_lines)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.code_lines, final_stats.code_lines)),
                number(&with_seperators(entry.stats.extra_lines)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.extra_lines, final_stats.extra_lines)),
        ));
    }
    print!("{comparison_str}");

    fn color_percentage(percentage: &str) -> ColoredString {
        let theme = theme::active();
        if percentage.starts_with('+') {
            theme.progress_up.paint(percentage)
        } else if percentage.starts_with('-') {
            theme.progress_down.paint(percentage)
        } else {
            theme.progress_same.paint(percentage)
        }
    }
}


fn split_minutes_to_D_H_M(mut minutes: i64) -> (i64, i64, i64) {
    let minutes_in_day = 60 * 24;
    let minutes_in_hour = 60;
    let days = minutes / minutes_in_day;
    minutes -= days * minutes_in_day;
    let hours = minutes / minutes_in_hour;
    minutes -= hours * minutes_in_hour;

    (days, hours, minutes)
}

fn difference_as_signed_percentage_str_of_usize(older: usize, newer: usize) -> String {
    let (difference, sign) = if newer > older {(newer-older, "+".to_owned())} else if older > newer {(older-newer, "-".to_owned())} else {(0,String::new())};
    let mut percentage = (difference as f64 / older as f64) * 100.0;
    let mut prefix_symbol = "";
    if percentage > 0.0 && percentage < 0.01 {
        prefix_symbol = " <";
        percentage = 0.01;
    }
    
    sign + prefix_symbol + &round_2(percentage).to_string()
}


#[cfg(test)]
fn difference_as_signed_percentage_str_of_f64(older: f64, newer: f64) -> String {
    let (difference, sign) = if newer > older {(newer-older, "+".to_owned())} else if older > newer {(older-newer, "-".to_owned())} else {(0.0,String::new())};
    let mut percentage = (difference / older) * 100.0;
    if percentage > 0.0 && percentage < 0.01 {
        percentage = 0.01;
    }

    sign + &round_2(percentage).to_string()
}

#[derive(Debug)]
struct LogEntry {
    name: Option<String>,
    stats: FinalStats,
    datetime: DateTime<Local>,
}

fn parse_N_previous_entries(log_content: &str, n: usize) -> Vec<LogEntry> {
    let mut log_entries = Vec::with_capacity(15);
    let (mut files, mut lines, mut code_lines, mut extra_lines, mut bytes_size) = (0, 0, 0, 0, 0);
    let mut counter = 0;
    let mut is_expecting_date = false;
    let mut entry_name = None;
    let mut datetime = chrono::Local::now();

    for line in log_content.lines() {
        let line = line.trim_start();
        if is_expecting_date {
            let fixed_datetime = chrono::DateTime::parse_from_str(line, "%Y-%m-%d %H:%M:%S %z").unwrap();
            datetime = fixed_datetime.with_timezone(&Local);
            is_expecting_date = false;
        }

        if let Some(entry) = line.strip_prefix("===>") {
            is_expecting_date = true;
            let _entry = entry.trim();
            if !_entry.is_empty() {
                entry_name = Some(_entry.to_owned());
            } else {
                entry_name = None;
            }
        } else if let Some(value) = line.strip_prefix(FILES) {
            files = value.trim().parse::<usize>().unwrap();
        } else if let Some(value) = line.strip_prefix(LINES) {
            lines = value.trim().parse::<usize>().unwrap();
        } else if let Some(value) = line.strip_prefix(CODE) {
            code_lines = value.trim().parse::<usize>().unwrap();
        } else if let Some(value) = line.strip_prefix(EXTRA) {
            extra_lines = value.trim().parse::<usize>().unwrap();
        } else if let Some(value) = line.strip_prefix(TOTAL_SIZE) {
            bytes_size = value.trim().parse::<usize>().unwrap();
        } else if let Some(value) = line.strip_prefix(AVERAGE_SIZE) {
            let bytes_average_size = value.trim().parse::<usize>().unwrap();
            let stats = FinalStats::new_extended(files, lines, code_lines, extra_lines, bytes_size, bytes_average_size);
            log_entries.push(LogEntry{name: entry_name.clone(), stats, datetime});

            counter += 1;
            if counter == n {return log_entries}
        }
    }

    log_entries
} 

fn get_keywords_as_str(keyword_occurencies: &HashMap<String,usize>, max_files_num_size: usize) -> String {
    let mut keyword_info = String::new();
    if !keyword_occurencies.is_empty() {
        let mut sorted_keywords = keyword_occurencies.iter().collect::<Vec<_>>();
        sorted_keywords.sort_unstable_by_key(|(name,_)| name.as_str());
        let mut keyword_iter = sorted_keywords.into_iter();
        let first_keyword = keyword_iter.next().unwrap();
        let theme = theme::active();
        keyword_info.push_str(&format!("{}{}: {}"," ".repeat(KEYWORD_LINE_OFFSET + max_files_num_size),
                theme.keyword.paint(first_keyword.0),theme.number.paint(&with_seperators(*first_keyword.1))));
        for (keyword_name,occurancies) in keyword_iter {
            keyword_info.push_str(&format!(" , {}: {}",theme.keyword.paint(keyword_name),theme.number.paint(&with_seperators(*occurancies))));
        }
    }
    keyword_info
}

fn create_keyword_sum_map(content_info_map: &HashMap<String,LanguageContentInfo>) -> HashMap<String,usize> {
    let mut collective_keywords_map : HashMap<String,usize> = HashMap::new();
    for content_info in content_info_map.values() {
        for keyword in &content_info.keyword_occurences {
            if *keyword.1 == 0 {continue;}
            if let Some(x) = collective_keywords_map.get_mut(keyword.0) {
                *x += *keyword.1;
            } else {
                collective_keywords_map.insert(keyword.0.to_owned(), *keyword.1);
            }
        }
    }

    collective_keywords_map
}

fn get_size_and_formatted_size_text(value: usize, suffix: &str) -> (f64,ColoredString) {
    if value > 1000000
        {(value as f64 / 1000000f64, colored_word(&("MBs ".to_owned() + suffix)))}
    else if value > 1000
        {(value as f64 / 1000f64, colored_word(&("KBs ".to_owned() + suffix)))}
    else
        {(value as f64, colored_word(&("Bytes ".to_owned() + suffix)))}
}

fn colored_word(word: &str) -> ColoredString {
    theme::active().label.paint(word)
}

fn number(value: &str) -> ColoredString {
    theme::active().number.paint(value)
}

// A language that is present but rounds to 0.00 would read as absent, while the bar still shows a
// cell for it because of the minimum-one rule. '<0.01' is the same convention the progress section
// already uses for tiny differences. Comparing the formatted text rather than the number keeps this
// independent of how the formatter rounds a halfway value.
fn percent_text(value: f64) -> String {
    let text = format!("{value:.2}");
    if value > 0.0 && text == "0.00" { "<0.01".to_owned() } else { text }
}

fn percent(value: f64) -> ColoredString {
    theme::active().percent.paint(&percent_text(value))
}


// Ties are broken by name rather than left to the iteration order of the maps, which would make
// the printed order differ between runs on the very projects where languages are evenly matched
fn get_sorted_language_names(content_info_map: &HashMap<String, LanguageContentInfo>,
        languages_metadata_map: &HashMap<String, LanguageMetadata>, criterion: SortCriterion) -> Vec<String>
{
    let value_of = |name: &String| match criterion {
        SortCriterion::Files => languages_metadata_map.get(name).map_or(0, |x| x.files),
        SortCriterion::Size => languages_metadata_map.get(name).map_or(0, |x| x.bytes),
        SortCriterion::Lines => content_info_map.get(name).map_or(0, |x| x.lines),
        SortCriterion::Code => content_info_map.get(name).map_or(0, |x| x.code_lines),
        SortCriterion::Name => 0
    };

    let mut names = languages_metadata_map.keys().cloned().collect::<Vec<_>>();
    if criterion == SortCriterion::Name {
        names.sort_by_key(|x| x.to_lowercase());
    } else {
        names.sort_by(|a, b| value_of(b).cmp(&value_of(a)).then_with(|| a.to_lowercase().cmp(&b.to_lowercase())));
    }

    names
}

// Largest remainder apportionment. Every language takes the whole part of its exact share, a
// language with any presence at all keeps at least one cell so that it cannot vanish from the bar,
// and the remaining cells go one at a time to whichever language sits furthest from its exact
// share. Exact by construction, in both directions: the minimum-one rule can push the total over
// the target (97/1/1/1 wants 51 cells), and that is corrected without ever emptying a language.
fn get_num_of_verticals(percentages: &[f64], width: usize) -> Vec<usize> {
    let exact = percentages.iter().map(|x| x * width as f64 / 100.0).collect::<Vec<_>>();
    let mut verticals = percentages.iter().zip(exact.iter())
            .map(|(percentage, exact)| if *percentage < 0.01 {0} else {(*exact as usize).max(1)})
            .collect::<Vec<_>>();

    let mut sum = verticals.iter().sum::<usize>();

    while sum < width {
        let distance_below = |i: &usize| exact[*i] - verticals[*i] as f64;
        let furthest_below = (0..verticals.len()).filter(|i| percentages[*i] >= 0.01)
                .max_by(|a, b| distance_below(a).total_cmp(&distance_below(b)));
        match furthest_below {
            Some(i) => verticals[i] += 1,
            None => break
        }
        sum += 1;
    }

    // The cell comes off whoever holds the most, not off whoever is closest to its exact share.
    // What matters in a bar is relative fidelity: one cell missing from a language holding 96 is
    // invisible, while the same cell taken from a language holding 3 understates it by a third.
    while sum > width {
        let largest = (0..verticals.len()).filter(|i| verticals[*i] > 1)
                .max_by(|a, b| verticals[*a].cmp(&verticals[*b]).then(exact[*a].total_cmp(&exact[*b])));
        match largest {
            Some(i) => verticals[i] -= 1,
            None => break
        }
        sum -= 1;
    }

    verticals
}

fn create_overview_line(prefix: &str, percentages: &[f64], verticals: &[usize], languages_name: &[String],
        color_func_vec: &[ColorFunc], config: &Configuration) -> String
{
    let mut line = String::with_capacity(150);
    line.push_str(&format!("{}    ", theme::active().overview_label.paint(prefix)));
    for (i,percentage) in percentages.iter().enumerate() {
        // The padding is computed from the same text that gets printed, and saturates, since a
        // single language at 100.00 is six characters wide and used to underflow this subtraction
        let str_perc = percent_text(*percentage);
        line.push_str(&format!("{}{}% ", " ".repeat(5usize.saturating_sub(str_perc.len())), percent(*percentage)));
        line.push_str(&color_func_vec[i](&languages_name[i]));
        if i < percentages.len() - 1{
            line.push_str(" - ")
        }
    }

    if !config.hidden.bar {
        add_verticals_str(&mut line, verticals, color_func_vec, config.bar_thickness.character());
    }

    line
}

fn add_verticals_str(line: &mut String, files_verticals: &[usize], color_func_vec: &[ColorFunc], character: &str) {
    let theme = theme::active();
    line.push_str("    ");
    line.push_str(&theme.bar_frame.paint("[-").to_string());
    for (i,verticals) in files_verticals.iter().enumerate() {
        line.push_str(&color_func_vec[i](character).repeat(*verticals));
    }
    line.push_str(&theme.bar_frame.paint("-]").to_string());
}

fn retain_most_relevant_and_add_others_field_for_rest(sorted_language_names: &mut Vec<String>,
        content_info_map: &mut HashMap<String, LanguageContentInfo>,
        languages_metadata_map: &mut HashMap<String, LanguageMetadata>,
        final_stats: &FinalStats, top_n: Option<usize>)
{
    fn get_files_lines_size(content_info_map: &HashMap<String, LanguageContentInfo>,
        languages_metadata_map: &HashMap<String, LanguageMetadata>) -> (usize,usize,usize) 
   {
       let (mut files, mut lines, mut size) = (0,0,0);
       content_info_map.iter().for_each(|x| lines += x.1.lines);
       languages_metadata_map.iter().for_each(|x| {files += x.1.files; size += x.1.bytes});
       (files, lines, size) 
   }

    // --top never widens the overview past its own cap, it only narrows it, so that asking for the
    // top 2 does not leave three languages sitting in the bar
    let to_keep = OVERVIEW_LANGUAGES.min(top_n.unwrap_or(OVERVIEW_LANGUAGES));
    if sorted_language_names.len() <= to_keep + 1 {
        return;
    }

    sorted_language_names.truncate(to_keep);
    sorted_language_names.push("others".to_owned());
    content_info_map.retain(|x,_| sorted_language_names.contains(x));
    languages_metadata_map.retain(|x,_| sorted_language_names.contains(x));

    let (relevant_files, relevant_lines, relevant_size) = get_files_lines_size(content_info_map, languages_metadata_map);
    let (other_files, other_lines, other_size) =
        (final_stats.files - relevant_files, final_stats.lines - relevant_lines,
         final_stats.bytes_size - relevant_size);

    //We only care about the total lines of code for the "others" field, this is the only field involved with calculations
    content_info_map.insert("others".to_string(), LanguageContentInfo::dummy(other_lines));
    languages_metadata_map.insert("others".to_string(), LanguageMetadata::new(other_files, other_size));
}


fn get_files_percentages(languages_metadata_map: &HashMap<String,LanguageMetadata>, sorted_language_names: &[String]) -> Vec<f64> {
    let mut language_files = [0].repeat(languages_metadata_map.len());
    languages_metadata_map.iter().for_each(|e| {
        let pos = sorted_language_names.iter().position(|name| name == e.0).unwrap();
        language_files[pos] = e.1.files;
    });
    
    get_percentages(&language_files)
}

fn get_lines_percentages(content_info_map: &HashMap<String,LanguageContentInfo>, languages_name: &[String]) -> Vec<f64> {
    let mut language_lines = [0].repeat(content_info_map.len());
    content_info_map.iter().for_each(|e| {
        let pos = languages_name.iter().position(|name| name == e.0).unwrap();
        language_lines[pos] = e.1.lines;
    });

    get_percentages(&language_lines)
}

fn get_sizes_percentages(languages_metadata_map: &HashMap<String,LanguageMetadata>, languages_name: &[String]) -> Vec<f64> {
    let mut language_size = [0].repeat(languages_metadata_map.len());
    languages_metadata_map.iter().for_each(|e| {
        let pos = languages_name.iter().position(|name| name == e.0).unwrap();
        language_size[pos] = e.1.bytes;
    });
    
    get_percentages(&language_size)
}

fn get_percentages(numbers: &[usize]) -> Vec<f64> {
    let total_files :usize = numbers.iter().sum();
    let mut language_percentages = Vec::with_capacity(4);
    let mut sum = 0.0;
    for (counter,files) in numbers.iter().enumerate() {
        if counter == numbers.len() - 1 {
            let remainder = if sum > 99.99 {0.0} else {((100f64 - sum) * 100f64).round() / 100f64};
            // The last entry is the one that absorbs the rounding, and it is usually 'others', so
            // it needs the same marker as the rest when it is present but too small to print
            language_percentages.push(if remainder == 0.0 && *files > 0 {PRESENT_BUT_TINY} else {remainder});
        } else {
            let percentage = *files as f64/total_files as f64;
            let canonicalized = (percentage * 10000f64).round() / 100f64;
            // A language that exists but rounds away to zero keeps a value just above zero, so
            // that the printed text can say "<0.01" instead of claiming it is absent. The running
            // sum still takes the rounded value, so the arithmetic of the last entry is untouched,
            // and the marker stays below the threshold that earns a cell in the bar.
            let canonicalized = if canonicalized == 0.0 && *files > 0 {PRESENT_BUT_TINY} else {canonicalized};
            sum += (canonicalized * 100f64).round() / 100f64;
            language_percentages.push(canonicalized);
        }
    }
    language_percentages
}

fn get_biggest_prefix_standard_spaces(sorted_language_names: &[String], languages_metadata_map: &HashMap<String, LanguageMetadata>) -> usize {
    let longest_lang_name = sorted_language_names.iter().map(|x| x.chars().count()).max().unwrap();
    let longest_lang_name = max(longest_lang_name,5);
    let total_files: usize = languages_metadata_map.iter().map(|meta| meta.1.files).sum();
    let total_files_digits = total_files.to_string().chars().count();

    longest_lang_name + total_files_digits + utils::num_of_seperators(total_files)
}


#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{config_manager::LogOption, io_handler::log_stats};

    use super::*;
    #[test]
    fn test_get_lines_percentages() {
        let ext_names = ["py".to_string(),"java".to_string(),"cs".to_string()];

        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(100),
            "java".to_string() => LanguageContentInfo::dummy(100), "py".to_string() => LanguageContentInfo::dummy(0));
        assert_eq!(vec![0f64,50f64,50f64], get_lines_percentages(&content_info_map, &ext_names));
        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(0),
        "java".to_string() => LanguageContentInfo::dummy(0), "py".to_string() => LanguageContentInfo::dummy(1));
        assert_eq!(vec![100f64,0f64,0f64], get_lines_percentages(&content_info_map, &ext_names));
        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(20),
        "java".to_string() => LanguageContentInfo::dummy(20), "py".to_string() => LanguageContentInfo::dummy(20));
        assert_eq!(vec![33.33f64,33.33f64,33.34f64], get_lines_percentages(&content_info_map, &ext_names));
        
        let ext_names = ["py".to_string(),"java".to_string(),"cs".to_string(),"rs".to_string()];

        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(100),
            "java".to_string() => LanguageContentInfo::dummy(100), "py".to_string() => LanguageContentInfo::dummy(0),
            "rs".to_string() => LanguageContentInfo::dummy(0));
        assert_eq!(vec![0f64,50f64,50f64,0f64], get_lines_percentages(&content_info_map, &ext_names));
        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(100),
            "java".to_string() => LanguageContentInfo::dummy(100), "py".to_string() => LanguageContentInfo::dummy(100),
            "rs".to_string() => LanguageContentInfo::dummy(0));
        assert_eq!(vec![33.33,33.33,33.33,0.01], get_lines_percentages(&content_info_map, &ext_names));
        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(201),
            "java".to_string() => LanguageContentInfo::dummy(200), "py".to_string() => LanguageContentInfo::dummy(200),
            "rs".to_string() => LanguageContentInfo::dummy(0));
        assert_eq!(vec![33.28,33.28,33.44,0.0], get_lines_percentages(&content_info_map, &ext_names));

        let ext_names = ["py".to_string(),"java".to_string(),"cs".to_string(),"rs".to_string(),"cpp".to_string()];

        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(100),
            "java".to_string() => LanguageContentInfo::dummy(100), "py".to_string() => LanguageContentInfo::dummy(0),
            "rs".to_string() => LanguageContentInfo::dummy(0), "cpp".to_string() => LanguageContentInfo::dummy(0));
        assert_eq!(vec![0.0,50f64,50f64,0f64,0f64], get_lines_percentages(&content_info_map, &ext_names));
    }

    #[test]
    fn test_get_num_of_verticals() {
        let percentages = vec![49.6,50.4];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![25,25], verticals);

        let percentages = vec![0.0,100.0];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![0,50], verticals);


        let percentages = vec![33.33,33.33,33.34];
        assert_eq!(vec![16,17,17], get_num_of_verticals(&percentages, NUM_OF_VERTICALS));

        let percentages = vec![0.3,65.67,34.3];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![1,32,17], verticals);
        
        let percentages = vec![0.0,0.0,100.0];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![0,0,50], verticals);

        let percentages = vec![0.2,49.9,49.9];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![1,24,25], verticals);


        let percentages = vec![12.5,50.0,25.0,12.5];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![6,25,13,6], verticals);

        let percentages = vec![0.1,0.1,49.9,49.9];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![1,1,24,24], verticals);

        // The minimum-one rule wants 48+1+1+1 here, which is one cell over the target. The excess
        // has to come off the largest share rather than emptying one of the small ones.
        let verticals = get_num_of_verticals(&[97.0,1.0,1.0,1.0], NUM_OF_VERTICALS);
        assert_eq!(vec![47,1,1,1], verticals);

        // Every protected minimum is paid for by the only entry that has cells to spare
        assert_eq!(vec![47,1,1,1], get_num_of_verticals(&[99.4,0.2,0.2,0.2], NUM_OF_VERTICALS));

        let verticals = get_num_of_verticals(&[99.7,0.1,0.1,0.1], NUM_OF_VERTICALS);
        assert_eq!(50, verticals.iter().sum::<usize>());
        assert!(verticals.iter().all(|x| *x >= 1), "a language that is present must never lose its last cell");
    }

    #[test]
    fn a_language_that_rounds_away_is_shown_as_less_than_a_hundredth_and_gets_no_cell() {
        // 3 files out of 800000 is 0.000375%, which used to print as a flat 0.00. Checked in the
        // middle and in the last position, which are computed by different branches
        for numbers in [vec![500_000, 3, 299_997], vec![500_000, 299_997, 3]] {
            let percentages = get_percentages(&numbers);
            let tiny = numbers.iter().position(|x| *x == 3).unwrap();
            assert_eq!("<0.01", percent_text(percentages[tiny]), "for {numbers:?}");
            let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
            assert_eq!(0, verticals[tiny], "a share too small to be printed must not claim a cell either");
            assert_eq!(NUM_OF_VERTICALS, verticals.iter().sum::<usize>());
        }

        // A language that really is absent stays at a flat zero and keeps no cell
        let percentages = get_percentages(&[500_000, 299_997, 0]);
        assert_eq!("0.00", percent_text(percentages[2]));
        assert_eq!(0, get_num_of_verticals(&percentages, NUM_OF_VERTICALS)[2]);

        // A genuine zero stays a zero, and anything printable is left alone
        assert_eq!("0.00", percent_text(0.0));
        assert_eq!("0.01", percent_text(0.01));
        assert_eq!("12.35", percent_text(12.345));
        assert_eq!("100.00", percent_text(100.0));

        // The overview pads every percentage into a 5 column field, so the marker has to fit in it.
        // 100.00 is the one value that does not, which is why that padding saturates.
        for value in [0.0, PRESENT_BUT_TINY, 0.01, 9.9, 99.99] {
            assert!(percent_text(value).len() <= 5, "'{}' does not fit the column", percent_text(value));
        }
        assert_eq!(6, percent_text(100.0).len());
    }

    #[test]
    fn sorting_uses_the_chosen_criterion_and_breaks_ties_by_name() {
        let content = hashmap![
            "Zig".to_owned() => LanguageContentInfo::new(100, 50, HashMap::new()),
            "Ada".to_owned() => LanguageContentInfo::new(100, 90, HashMap::new()),
            "Rust".to_owned() => LanguageContentInfo::new(300, 10, HashMap::new())];
        let meta = hashmap![
            "Zig".to_owned() => LanguageMetadata::new(9, 10),
            "Ada".to_owned() => LanguageMetadata::new(1, 900),
            "Rust".to_owned() => LanguageMetadata::new(5, 50)];

        assert_eq!(vec!["Rust","Ada","Zig"], get_sorted_language_names(&content, &meta, SortCriterion::Lines));
        assert_eq!(vec!["Zig","Rust","Ada"], get_sorted_language_names(&content, &meta, SortCriterion::Files));
        assert_eq!(vec!["Ada","Rust","Zig"], get_sorted_language_names(&content, &meta, SortCriterion::Size));
        assert_eq!(vec!["Ada","Zig","Rust"], get_sorted_language_names(&content, &meta, SortCriterion::Code));
        assert_eq!(vec!["Ada","Rust","Zig"], get_sorted_language_names(&content, &meta, SortCriterion::Name));

        // Ada and Zig both have 100 lines, so the name decides, not the iteration order of the map
        assert_eq!(vec!["Rust","Ada","Zig"], get_sorted_language_names(&content, &meta, SortCriterion::Lines));
    }

    #[test]
    fn the_cell_that_a_protected_minimum_costs_comes_off_the_largest_share() {
        // Six entries at 100 cells: the second language deserves 3 and must keep them, because
        // losing one understates it by a third while the first barely notices
        let percentages = vec![96.96, 3.0, 0.01, 0.01, 0.01, 0.01];
        assert_eq!(vec![93,3,1,1,1,1], get_num_of_verticals(&percentages, 100));
        assert_eq!(vec![45,1,1,1,1,1], get_num_of_verticals(&percentages, NUM_OF_VERTICALS));

        // The width is a parameter, so the same shares scale to any bar
        assert_eq!(25, get_num_of_verticals(&[50.0,50.0], 50)[0]);
        assert_eq!(50, get_num_of_verticals(&[50.0,50.0], 100)[0]);
        assert_eq!(10, get_num_of_verticals(&[50.0,50.0], 20)[0]);
    }

    #[test]
    fn verticals_always_sum_to_the_bar_width_and_keep_present_languages_visible() {
        let cases: Vec<Vec<f64>> = vec![
            vec![100.0], vec![50.0,50.0], vec![0.01,99.99], vec![0.0,0.0,0.0,100.0],
            vec![25.0,25.0,25.0,25.0], vec![70.0,10.0,10.0,10.0], vec![1.0,1.0,1.0,97.0],
            vec![0.04,0.04,0.04,99.88], vec![33.34,33.33,33.33], vec![60.5,39.5],
            vec![98.0,2.0], vec![2.0,98.0], vec![0.0,100.0,0.0]
        ];

        for percentages in cases {
            let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
            assert_eq!(NUM_OF_VERTICALS, verticals.iter().sum::<usize>(), "wrong total for {percentages:?}");
            for (i, percentage) in percentages.iter().enumerate() {
                if *percentage > 0.0 {
                    assert!(verticals[i] >= 1, "{percentages:?} made a present language disappear");
                } else {
                    assert_eq!(0, verticals[i], "{percentages:?} gave cells to an absent language");
                }
            }
        }
    }

    #[test]
    fn test_retain_most_relevant_and_add_others_field_for_rest() {
        let mut sorted_language_names = vec!["a".to_owned(), "b".to_owned(), "c".to_owned(), "d".to_owned(), "e".to_owned()];
        let mut content_info_map = hashmap![
            "a".to_owned() => LanguageContentInfo::new(1000, 800, hashmap![]),
            "b".to_owned() => LanguageContentInfo::new(900, 700, hashmap![]),
            "c".to_owned() => LanguageContentInfo::new(800, 600, hashmap![]),
            "d".to_owned() => LanguageContentInfo::new(700, 500, hashmap![]),
            "e".to_owned() => LanguageContentInfo::new(600, 400, hashmap![])
        ];
        let mut languages_metadata_map = hashmap![
            "a".to_owned() => LanguageMetadata::new(10, 60000),
            "b".to_owned() => LanguageMetadata::new(9, 50000),
            "c".to_owned() => LanguageMetadata::new(8, 40000),
            "d".to_owned() => LanguageMetadata::new(7, 30000),
            "e".to_owned() => LanguageMetadata::new(6, 20000)
        ];
        let final_stats = FinalStats::new(40, 4000, 3000, 200000);

        retain_most_relevant_and_add_others_field_for_rest(&mut sorted_language_names, &mut content_info_map, &mut languages_metadata_map, &final_stats, None);

        assert_eq!(hashmap![
            "a".to_owned() => LanguageContentInfo::new(1000, 800, hashmap![]),
            "b".to_owned() => LanguageContentInfo::new(900, 700, hashmap![]),
            "c".to_owned() => LanguageContentInfo::new(800, 600, hashmap![]),
            "others".to_owned() => LanguageContentInfo::new(1300, 0, hashmap![])
            ], content_info_map);
        
        assert_eq!(hashmap![
            "a".to_owned() => LanguageMetadata::new(10, 60000),
            "b".to_owned() => LanguageMetadata::new(9, 50000),
            "c".to_owned() => LanguageMetadata::new(8, 40000),
            "others".to_owned() => LanguageMetadata::new(13, 50000)
            ], languages_metadata_map);
    }

    #[test]
    fn test_time_split_from_minutes() {
        assert_eq!((0,0,0),split_minutes_to_D_H_M(0));
        assert_eq!((0,0,59),split_minutes_to_D_H_M(59));
        assert_eq!((0,1,0),split_minutes_to_D_H_M(60));
        assert_eq!((0,1,1),split_minutes_to_D_H_M(61));
        assert_eq!((1,0,0),split_minutes_to_D_H_M(1440));
        assert_eq!((1,0,1),split_minutes_to_D_H_M(1441));
        assert_eq!((1,1,1),split_minutes_to_D_H_M(1501));
    }

    #[test]
    fn test_difference_as_percentages() {
        assert_eq!("0",difference_as_signed_percentage_str_of_usize(100, 100));
        assert_eq!("-10",difference_as_signed_percentage_str_of_usize(100, 90));
        assert_eq!("+100",difference_as_signed_percentage_str_of_usize(100, 200));
        assert_eq!("+ <0.01",difference_as_signed_percentage_str_of_usize(22819, 22820));
        
        assert_eq!("0",difference_as_signed_percentage_str_of_f64(100.0, 100.0));
        assert_eq!("-10",difference_as_signed_percentage_str_of_f64(100.0, 90.0));
        assert_eq!("+100",difference_as_signed_percentage_str_of_f64(100.0, 200.0));
        assert_eq!("+0.01",difference_as_signed_percentage_str_of_f64(22819.0, 22820.0));
    }

    #[test]
    fn test_parse_N_previous_entries() {
        let contents = utils::extract_file_contents(&(LOCAL_APP_PATHS.test_dir.clone()+"logs/test1")).unwrap();
        let log_entries = parse_N_previous_entries(&contents, 3);

        assert_eq!(10, log_entries[0].stats.files);
        assert_eq!(1000, log_entries[0].stats.lines);
        assert_eq!(100, log_entries[0].stats.code_lines);
        assert_eq!(100, log_entries[0].stats.extra_lines);
        assert_eq!(100000, log_entries[0].stats.bytes_size);
        assert_eq!(100.0, log_entries[0].stats.size);
        assert_eq!("KBs".to_owned(), log_entries[0].stats.size_measurement);
        assert_eq!(10000, log_entries[0].stats.bytes_average_size);
        assert_eq!(10.0, log_entries[0].stats.average_size);
        assert_eq!("KBs".to_owned(), log_entries[0].stats.average_size_measurement);
        let datetime: DateTime<Local> = chrono::DateTime::from_str("2021-09-12 16:42:00 +0300").unwrap();
        assert_eq!(datetime, log_entries[0].datetime);
        assert_eq!(Some("entry one".to_owned()),log_entries[0].name);

        assert_eq!(11, log_entries[1].stats.files);
        assert_eq!(1111, log_entries[1].stats.lines);
        assert_eq!(111, log_entries[1].stats.code_lines);
        assert_eq!(111, log_entries[1].stats.extra_lines);
        assert_eq!(111100, log_entries[1].stats.bytes_size);
        assert_eq!(111.1, log_entries[1].stats.size);
        assert_eq!("KBs".to_owned(), log_entries[1].stats.size_measurement);
        assert_eq!(11100, log_entries[1].stats.bytes_average_size);
        assert_eq!(11.1, log_entries[1].stats.average_size);
        assert_eq!("KBs".to_owned(), log_entries[1].stats.average_size_measurement);
        let datetime: DateTime<Local> = chrono::DateTime::from_str("2021-09-12 16:23:50 +03:00").unwrap();
        assert_eq!(datetime, log_entries[1].datetime);
        assert_eq!(None,log_entries[1].name);

        assert_eq!(12, log_entries[2].stats.files);
        assert_eq!(1222, log_entries[2].stats.lines);
        assert_eq!(122, log_entries[2].stats.code_lines);
        assert_eq!(122, log_entries[2].stats.extra_lines);
        assert_eq!(122200, log_entries[2].stats.bytes_size);
        assert_eq!(122.2, log_entries[2].stats.size);
        assert_eq!("KBs".to_owned(), log_entries[2].stats.size_measurement);
        assert_eq!(12200, log_entries[2].stats.bytes_average_size);
        assert_eq!(12.2, log_entries[2].stats.average_size);
        assert_eq!("KBs".to_owned(), log_entries[2].stats.average_size_measurement);
        let datetime: DateTime<Local> = chrono::DateTime::from_str("2021-09-12 04:01:56 +03:00").unwrap();
        assert_eq!(datetime, log_entries[2].datetime);
        assert_eq!(Some("entry three".to_owned()),log_entries[2].name);
    }

    #[test]
    fn test_log_creation_and_reading() -> std::io::Result<()> {
        let test_log_dir = LOCAL_APP_PATHS.test_log_dir.clone() + "test2";
        if Path::new(&test_log_dir).exists() {
            std::fs::remove_file(&test_log_dir).unwrap();
        }

        let mut config = Configuration::new(vec!["./".to_owned()]);
        config.set_log_option(LogOption::new(Some("test name".to_owned())));
        let final_stats = FinalStats::new(10, 1000, 100, 100);

        log_stats(&test_log_dir, &None, &final_stats, &chrono::DateTime::from_str("2021-09-12 04:00:00 +03:00").unwrap(), &config).unwrap();

        let contents = utils::extract_file_contents(&test_log_dir).unwrap();
        let log_entries = parse_N_previous_entries(&contents, 1);

        assert_eq!(10, log_entries[0].stats.files);
        assert_eq!(1000, log_entries[0].stats.lines);
        assert_eq!(100, log_entries[0].stats.code_lines);
        assert_eq!(900, log_entries[0].stats.extra_lines);
        assert_eq!(100, log_entries[0].stats.bytes_size);
        assert_eq!(100.0, log_entries[0].stats.size);
        assert_eq!("Bytes".to_owned(), log_entries[0].stats.size_measurement);
        assert_eq!(10, log_entries[0].stats.bytes_average_size);
        assert_eq!(10.0, log_entries[0].stats.average_size);
        assert_eq!("Bytes".to_owned(), log_entries[0].stats.average_size_measurement);
        assert_eq!(Some("test name".to_owned()),log_entries[0].name);

        Ok(())
    }
}