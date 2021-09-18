use std::{cmp::max, io::Write};

use chrono::{FixedOffset, NaiveDateTime, TimeZone};

use crate::*;

//the total number of vertical lines ( | ) that appear in the [-|||...|-] in the overview section
const NUM_OF_VERTICALS : usize = 50;

const KEYWORD_LINE_OFFSET : usize = 19;
const STANDARD_LINE_STATS_LEN : usize = 33;
const DASH_LINE_OFFSET : usize = 47;

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
    remove_languages_with_0_files(content_info_map, languages_metadata_map);

    let mut sorted_language_names = get_language_names_as_sorted_vec_according_to_how_much_they_appeared(languages_metadata_map);
    let biggest_prefix_standard_spaces = get_biggest_prefix_standard_spaces(&sorted_language_names, &languages_metadata_map);

    print_individually(&sorted_language_names, &content_info_map, languages_metadata_map, biggest_prefix_standard_spaces);

    if languages_metadata_map.len() > 1 {
        print_sum(&content_info_map, final_stats, biggest_prefix_standard_spaces);
        print_visual_overview(&mut sorted_language_names, content_info_map, languages_metadata_map, final_stats, config);
    }

    if let Some(content) = existing_log_content {
        if config.compare_level != 0 {
            print_comparison_to_previous_runs(final_stats, content,  config.compare_level, datetime_now);
        }    
    }
}


fn print_individually(sorted_languages: &[String], content_info_map: &HashMap<String,LanguageContentInfo>,
     languages_metadata_map: &HashMap<String, LanguageMetadata>, biggest_prefix_standard_spaces: usize)
{
    fn get_size_text(metadata: &LanguageMetadata) -> String {
        let (size, size_desc) = get_size_and_formatted_size_text(metadata.bytes, "total");
        let (average_size, average_size_desc) = get_size_and_formatted_size_text(
                metadata.bytes / metadata.files, "average");

        format!("{:.1} {} - {:.1} {}",size, size_desc, average_size, average_size_desc)
    }

    fn reconstruct_line(i: usize, max_line_stats_len: usize, titles_vec: &[String], lines_stats_vec: &[String],
         lines_stats_len_vec: &[usize], size_stats_vec: &[String], keywords_stats_vec: &[String]) -> String
    {
        let spaces = max_line_stats_len+1 - lines_stats_len_vec[i];
        titles_vec[i].clone() + &lines_stats_vec[i] + &" ".repeat(spaces) + " |  " + &size_stats_vec[i] +
                "\n" + &keywords_stats_vec[i]
    }

    println!("{}.\n", "Details".underline().bold());
    
    let mut max_line_stats_len = STANDARD_LINE_STATS_LEN;
    let (mut titles_vec, mut lines_stats_vec, mut lines_stats_len_vec, mut size_stats_vec,
            mut keywords_stats_vec) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());

    for lang_name in sorted_languages {
        let content_info = content_info_map.get(lang_name).unwrap();
        let metadata = languages_metadata_map.get(lang_name).unwrap();

        let files_str = with_seperators(metadata.files);
        let prefix_standard_spaces = lang_name.chars().count() + metadata.files.to_string().chars().count() +
                 utils::num_of_seperators(metadata.files); 
        let title = format!("{}   {}{} {}  -> ",lang_name.bold(), " ".repeat(biggest_prefix_standard_spaces - prefix_standard_spaces),
                 files_str, colored_word("files"));
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
        
        lines_stats_vec.push(format!("{} {} {{{} code ({:.2}%) + {} extra}}", colored_word("lines"), lines_str, code_lines_str,
                 code_lines_percentage, extra_lines_str));
        size_stats_vec.push(get_size_text(metadata));
        
        keywords_stats_vec.push(get_keywords_as_str(&content_info.keyword_occurences, biggest_prefix_standard_spaces));
    }

    for i in 0..lines_stats_vec.len() {
        let line = reconstruct_line(i, max_line_stats_len, &titles_vec, &lines_stats_vec,
                &lines_stats_len_vec, &size_stats_vec, &keywords_stats_vec);
                
        if i == lines_stats_len_vec.len() - 1 {
            println!("{}",line);
        } else {
            println!("{}\n",line);
        }
    }
}


fn print_sum(content_info_map: &HashMap<String,LanguageContentInfo>, final_stats: &FinalStats, biggest_prefix_standard_spaces: usize) 
{
    let (total_files_str, total_lines_str, total_code_lines_str, total_extra_lines_str) = 
            (with_seperators(final_stats.files),with_seperators(final_stats.lines),with_seperators(final_stats.code_lines), with_seperators(final_stats.extra_lines)); 

    let keywords_sum_map = create_keyword_sum_map(content_info_map);
    let keywords_line = get_keywords_as_str(&keywords_sum_map, biggest_prefix_standard_spaces);

    let spaces = biggest_prefix_standard_spaces - (5 + total_files_str.len());
    let title = format!("{}   {}{} {}  -> ","total".bold()," ".repeat(spaces),total_files_str,colored_word("files"));
    let code_lines_percentage = if final_stats.lines > 0 {final_stats.code_lines as f64 / final_stats.lines as f64 * 100f64} else {0f64};
    let size_text = format!("{} {} - {} {}",final_stats.size, colored_word(&format!("{} total", final_stats.size_measurement)),
            final_stats.average_size,colored_word(&format!("{} average", final_stats.average_size_measurement)));

    let line_len = STANDARD_LINE_STATS_LEN + total_files_str.len() + total_code_lines_str.len() + total_extra_lines_str.len() +
            final_stats.size.to_string().len() + final_stats.average_size.to_string().len() + DASH_LINE_OFFSET;
    println!("{} ","-".repeat(line_len));

    let info = format!("{} {} {{{} code ({:.2}%) + {} extra}}  |  {}\n",colored_word("lines"), total_lines_str,total_code_lines_str,
            code_lines_percentage, total_extra_lines_str, size_text);

    println!("{}", format!("{}{}{}\n",title,info,keywords_line));
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
    fn make_cyan(str: &str) -> String {
        str.cyan().to_string()
    }
    fn make_magenta(str: &str) -> String {
        str.bright_magenta().to_string()
    }
    fn make_yellow(str: &str) -> String {
        str.bright_yellow().to_string()
    }
    fn no_transformation(str: &str) -> String {
        str.to_owned()
    }
    fn make_fourth_color(str: &str) -> String {
        str.truecolor(106, 217, 189).to_string()
    }
    fn make_color_for_others(str: &str) -> String {
        str.truecolor(215, 201, 240).to_string()
    }

    if content_info_map.len() > 4 {
        retain_most_relevant_and_add_others_field_for_rest(sorted_language_vec, content_info_map, languages_metadata_map, final_stats);
    }

    println!("{}.\n", "Overview".underline().bold());

    let color_func_vec : Vec<fn(&str) -> String> = {
        if sorted_language_vec[sorted_language_vec.len()-1] == "others" {
            vec![make_cyan, make_magenta, make_yellow, make_color_for_others]
        } else {
            vec![make_cyan, make_magenta, make_yellow, make_fourth_color]
        }
    };

    let files_percentages = get_files_percentages(languages_metadata_map, sorted_language_vec);
    let lines_percentages = get_lines_percentages(content_info_map, sorted_language_vec);
    let sizes_percentages = get_sizes_percentages(languages_metadata_map, sorted_language_vec);

    let files_verticals = if config.no_visual {vec![]} else{get_num_of_verticals(&files_percentages)};
    let lines_verticals = if config.no_visual {vec![]} else{get_num_of_verticals(&lines_percentages)};
    let size_verticals = if config.no_visual {vec![]} else{get_num_of_verticals(&sizes_percentages)};

    let files_line = create_overview_line("Files:", &files_percentages, &files_verticals,
            sorted_language_vec, &color_func_vec, config);
    let lines_line = create_overview_line("Lines:", &lines_percentages, &lines_verticals,
            sorted_language_vec, &color_func_vec, config);
    let size_line = create_overview_line("Size :", &sizes_percentages, &size_verticals,
            sorted_language_vec, &color_func_vec, config);

    println!("{}\n\n{}\n\n{}\n",files_line, lines_line, size_line);
}

fn print_comparison_to_previous_runs(final_stats: &FinalStats, log_content: &str, num_of_entries: usize, datetime_now: &DateTime<Local>) {
    println!("\n{}.\n", "Progress".underline().bold());

    let (stat_entries, dates) = parse_N_previous_entries(log_content, num_of_entries);

    let mut comparison_str = String::with_capacity(200);
    for (i,entry) in stat_entries.iter().enumerate() {
        let then = (&dates[i]).to_owned();
        let then_str = then.naive_local().to_string();
        let duration = datetime_now.signed_duration_since(then);
        let (days, hours, minutes) = split_minutes_to_D_H_M(duration.num_minutes());
        comparison_str.push_str(&format!("{} {} ({} days, {} hours and {} minutes ago)\n","->".bold(), then_str, days, hours, minutes));
        comparison_str.push_str(&format!("     Files: {}({}%) Lines: {}({}%) {{Code: {}({}%), Extra: {}({}%)}}\n\n",
                with_seperators(entry.files), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.files, final_stats.files)),
                with_seperators(entry.lines), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.lines, final_stats.lines)),
                with_seperators(entry.code_lines), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.code_lines, final_stats.code_lines)),
                with_seperators(entry.extra_lines), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.extra_lines, final_stats.extra_lines)),
        ));
    }
    print!("{}", comparison_str);

    fn color_percentage(percentage: &str) -> ColoredString {
        if percentage.starts_with('+') {
            percentage.truecolor(201, 255, 189)
        } else if percentage.starts_with('-') {
            percentage.truecolor(219, 129, 129)
        } else {
            percentage.truecolor(255, 255, 255)
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
    if percentage > 0.0 && percentage < 0.01 {
        percentage = 0.01;
    }

    sign + &round_2(percentage).to_string()
}

fn difference_as_signed_percentage_str_of_f64(older: f64, newer: f64) -> String {
    let (difference, sign) = if newer > older {(newer-older, "+".to_owned())} else if older > newer {(older-newer, "-".to_owned())} else {(0.0,String::new())};
    let mut percentage = (difference as f64 / older as f64) * 100.0;
    if percentage > 0.0 && percentage < 0.01 {
        percentage = 0.01;
    }

    sign + &round_2(percentage).to_string()
}

fn parse_N_previous_entries(log_content: &str, n: usize) -> (Vec<FinalStats>, Vec<DateTime<Local>>) {
    let mut entries = Vec::new();
    let mut dates = Vec::new();
    let (mut files, mut lines, mut code_lines, mut extra_lines, mut bytes_size) = (0, 0, 0, 0, 0);
    let mut counter = 0;
    let mut is_expecting_date = false;

    for line in log_content.lines() {
        let line = line.trim_start();
        if is_expecting_date {
            let datetime = chrono::DateTime::parse_from_str(line, "%Y-%m-%d %H:%M:%S %z").unwrap();
            dates.push(datetime.with_timezone(&Local));
            is_expecting_date = false;
        }

        if line.starts_with("===>") {
            is_expecting_date = true;
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
            entries.push(FinalStats::new_extended(files, lines, code_lines, extra_lines, bytes_size, bytes_average_size));

            counter += 1;
            if counter == n {return (entries, dates)}
        }
    }

    (entries, dates)
} 

fn get_keywords_as_str(keyword_occurencies: &HashMap<String,usize>, max_files_num_size: usize) -> String {
    let mut keyword_info = String::new();
    if !keyword_occurencies.is_empty() {
        let mut keyword_iter = keyword_occurencies.iter();
        let first_keyword = keyword_iter.next().unwrap();
        keyword_info.push_str(&format!("{}{}: {}"," ".repeat(KEYWORD_LINE_OFFSET + max_files_num_size),
                colored_word(first_keyword.0),with_seperators(*first_keyword.1)));
        for (keyword_name,occurancies) in keyword_iter {
            keyword_info.push_str(&format!(" , {}: {}",colored_word(keyword_name),with_seperators(*occurancies)));
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
    word.italic().truecolor(181, 169, 138)
}

fn remove_languages_with_0_files(content_info_map: &mut HashMap<String,LanguageContentInfo>,
    languages_metadata_map: &mut HashMap<String, LanguageMetadata>) 
{
   let mut empty_languages = Vec::new();
   for element in languages_metadata_map.iter() {
       if element.1.files == 0 {
           empty_languages.push(element.0.to_owned());
       }
   }

   for ext in empty_languages {
       languages_metadata_map.remove(&ext);
       content_info_map.remove(&ext);
   }
}

fn get_language_names_as_sorted_vec_according_to_how_much_they_appeared(
   languages_metadata_map: &HashMap<String, LanguageMetadata>) -> Vec<String> 
{
    let mut value_map = HashMap::<String,usize>::new();
    let mut sorted_languages_vec = Vec::new();
    for (ext_name,metadata) in languages_metadata_map.iter() {
        value_map.insert(ext_name.to_owned(), metadata.files * 10 + metadata.bytes as usize);
        sorted_languages_vec.push(ext_name.to_owned());
    }

    sorted_languages_vec.sort_by(|a,b| {
        value_map.get(b).unwrap().cmp(value_map.get(a).unwrap())
    });

    sorted_languages_vec
}

fn get_num_of_verticals(percentages: &[f64]) -> Vec<usize> {
    let mut verticals = Vec::<usize>::with_capacity(4);
    let mut sum = 0;
    for files_percent in percentages.iter(){
        let num_of_verticals = 
        if *files_percent == 0f64 {
            0
        } else {
            let mut num_of_verticals = (files_percent/2.0).round() as usize;
            if num_of_verticals == 0 {
                num_of_verticals = 1;
            }
            num_of_verticals
        };
        verticals.push(num_of_verticals);
        sum += num_of_verticals;
    }

    if sum != NUM_OF_VERTICALS {
        normalize_to_NUM_OF_VERTICALS(&mut verticals, sum);
    }

    verticals
}

// A not very precise attempt to normalize the sum of verticals to the proper number that should appear 
// in the [-|||...|-] block, but is it good enough.
fn normalize_to_NUM_OF_VERTICALS(verticals: &mut Vec<usize>, sum: usize) {
    let mut sorted_verticals = Vec::new();
    for i in verticals.iter_mut() {
        sorted_verticals.push(i);
    }

    let comparator = |a: &&mut usize,b: &&mut usize| b.cmp(a);
    sorted_verticals.sort_by(comparator);

    let is_over = sum > NUM_OF_VERTICALS;
    let mut difference = if is_over {sum - NUM_OF_VERTICALS} else {NUM_OF_VERTICALS - sum}; 

    let same_num_of_verticals_indices = {
        let mut temp = Vec::new();
        let max_value = *sorted_verticals[0];
        let mut counter = 0;
        while counter < sorted_verticals.len() && *sorted_verticals[counter] == max_value {
            temp.push(counter);
            counter += 1;
        }
        temp
    };

    //ensures that if there are very close percentages, they wont have more than one vertical difference
    if same_num_of_verticals_indices.len() > 1 {
        for i in same_num_of_verticals_indices.iter() {
            if difference > 0 {
                if is_over {
                    *sorted_verticals[*i] -= 1
                } else {
                    *sorted_verticals[*i] += 1;
                }
                difference -= 1;
            } else {
                break;
            }
        }
    }

    if difference == 0 {return;}

    if is_over {
        *sorted_verticals[0] -= 1; 
        sorted_verticals.sort_by(comparator);
    } else {
        *sorted_verticals[0] += 1;
    }
    
    for _ in 0..difference-1 {
        if is_over {
            if *sorted_verticals[0] > *sorted_verticals[1] + 3 {
                *sorted_verticals[0] -= 1;
            } else {
                *sorted_verticals[1] -= 1;
                if sorted_verticals.len() > 2 {
                    sorted_verticals.sort_by(comparator);

                }
            }
        } else {
            if *sorted_verticals[0] > *sorted_verticals[1] + 5 {
                *sorted_verticals[1] += 1;
                if sorted_verticals.len() > 2 {
                    sorted_verticals.sort_by(comparator);
                }
            } else {
                *sorted_verticals[0] += 1;
            }
        }
    }
}

fn create_overview_line(prefix: &str, percentages: &[f64], verticals: &[usize], languages_name: &[String],
        color_func_vec: &[fn(&str) -> String], config: &Configuration) -> String 
{
    let mut line = String::with_capacity(150);
    line.push_str(&format!("{}    ",prefix));
    for (i,percent) in percentages.iter().enumerate() {
        let str_perc = format!("{:.2}",percent);
        line.push_str(&format!("{}{}% ", " ".repeat(5-str_perc.len()), str_perc));
        if config.no_visual {
            line.push_str(&languages_name[i]);
        } else {
            line.push_str(&color_func_vec[i](&languages_name[i]));
        }
        if i < percentages.len() - 1{
            line.push_str(" - ")
        }
    }
    
    if !config.no_visual {
        add_verticals_str(&mut line, verticals, color_func_vec);
    }

    line
}

fn add_verticals_str(line: &mut String, files_verticals: &[usize], color_func_vec: &[fn(&str) -> String]) {
    line.push_str("    [-");
    for (i,verticals) in files_verticals.iter().enumerate() {
        line.push_str(&color_func_vec[i]("|").repeat(*verticals));
    }
    line.push_str("-]");
}

fn retain_most_relevant_and_add_others_field_for_rest(sorted_language_names: &mut Vec<String>,
        content_info_map: &mut HashMap<String, LanguageContentInfo>,
        languages_metadata_map: &mut HashMap<String, LanguageMetadata>,
        final_stats: &FinalStats) 
{
    fn get_files_lines_size(content_info_map: &HashMap<String, LanguageContentInfo>,
        languages_metadata_map: &HashMap<String, LanguageMetadata>) -> (usize,usize,usize) 
   {
       let (mut files, mut lines, mut size) = (0,0,0);
       content_info_map.iter().for_each(|x| lines += x.1.lines);
       languages_metadata_map.iter().for_each(|x| {files += x.1.files; size += x.1.bytes});
       (files, lines, size as usize) 
   }

    if sorted_language_names.len() > 4 {
        for _ in 3..sorted_language_names.len() {
             sorted_language_names.remove(sorted_language_names.len()-1);
        }
        sorted_language_names.push("others".to_owned());

        content_info_map.retain(|x,_| sorted_language_names.contains(x));
        languages_metadata_map.retain(|x,_| sorted_language_names.contains(x));
    }
    
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
            if sum > 99.99 {
                language_percentages.push(0.0);
            } else {
                let rounded = ((100f64 - sum) * 100f64).round() / 100f64; 
                language_percentages.push(rounded);
            }
        } else {
            let percentage = *files as f64/total_files as f64;
            let canonicalized = (percentage * 10000f64).round() / 100f64;
            sum += canonicalized;
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

    use super::*;

    #[test]
    fn test_normalize() {
        let mut verticals = vec![18,15,19,1];
        normalize_to_NUM_OF_VERTICALS(&mut verticals, 53);
        assert_eq!(vec![16,15,18,1], verticals);
        assert!(verticals.iter().sum::<usize>() == 50);
        
        let mut verticals = vec![17,17,18,1];
        normalize_to_NUM_OF_VERTICALS(&mut verticals, 53);
        assert_eq!(vec![16,16,17,1], verticals);
        assert!(verticals.iter().sum::<usize>() == 50);
    
        let mut verticals = vec![16,15,16,1];
        normalize_to_NUM_OF_VERTICALS(&mut verticals, 48);
        assert_eq!(vec![17,15,17,1], verticals);
        assert!(verticals.iter().sum::<usize>() == 50);
    
        let mut verticals = vec![18,16,17];
        normalize_to_NUM_OF_VERTICALS(&mut verticals, 51);
        assert_eq!(vec![17,16,17], verticals);
        assert!(verticals.iter().sum::<usize>() == 50);
    
        let mut verticals = vec![25,26];
        normalize_to_NUM_OF_VERTICALS(&mut verticals, 51);
        assert_eq!(vec![25,25], verticals);
        assert!(verticals.iter().sum::<usize>() == 50);
    }

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
        let verticals = get_num_of_verticals(&percentages);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![25,25], verticals);

        let percentages = vec![0.0,100.0];
        let verticals = get_num_of_verticals(&percentages);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![0,50], verticals);


        let percentages = vec![33.33,33.33,33.34];
        assert_eq!(vec![16,17,17], get_num_of_verticals(&percentages));

        let percentages = vec![0.3,65.67,34.3];
        let verticals = get_num_of_verticals(&percentages);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![1,32,17], verticals);
        
        let percentages = vec![0.0,0.0,100.0];
        let verticals = get_num_of_verticals(&percentages);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![0,0,50], verticals);

        let percentages = vec![0.2,49.9,49.9];
        let verticals = get_num_of_verticals(&percentages);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![1,24,25], verticals);


        let percentages = vec![12.5,50.0,25.0,12.5];
        let verticals = get_num_of_verticals(&percentages);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![6,25,13,6], verticals);

        let percentages = vec![0.1,0.1,49.9,49.9];
        let verticals = get_num_of_verticals(&percentages);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![1,1,24,24], verticals);
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

        retain_most_relevant_and_add_others_field_for_rest(&mut sorted_language_names, &mut content_info_map, &mut languages_metadata_map, &final_stats);

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
        assert_eq!("+0.01",difference_as_signed_percentage_str_of_usize(22819, 22820));
        
        assert_eq!("0",difference_as_signed_percentage_str_of_f64(100.0, 100.0));
        assert_eq!("-10",difference_as_signed_percentage_str_of_f64(100.0, 90.0));
        assert_eq!("+100",difference_as_signed_percentage_str_of_f64(100.0, 200.0));
        assert_eq!("+0.01",difference_as_signed_percentage_str_of_f64(22819.0, 22820.0));
    }

    #[test]
    fn test_parse_N_previous_entries() {
        let contents = utils::extract_file_contents(&(io_handler::DATA_DIR.to_owned()+"/../test_dir/logs/test")).unwrap();
        let (entries, dates) = parse_N_previous_entries(&contents, 3);

        assert_eq!(10, entries[0].files);
        assert_eq!(1000, entries[0].lines);
        assert_eq!(100, entries[0].code_lines);
        assert_eq!(100, entries[0].extra_lines);
        assert_eq!(100000, entries[0].bytes_size);
        assert_eq!(100.0, entries[0].size);
        assert_eq!("KBs".to_owned(), entries[0].size_measurement);
        assert_eq!(10000, entries[0].bytes_average_size);
        assert_eq!(10.0, entries[0].average_size);
        assert_eq!("KBs".to_owned(), entries[0].average_size_measurement);
        let datetime: DateTime<Local> = chrono::DateTime::from_str("2021-09-12 16:42:00 +0300").unwrap();
        assert_eq!(datetime, dates[0]);

        assert_eq!(11, entries[1].files);
        assert_eq!(1111, entries[1].lines);
        assert_eq!(111, entries[1].code_lines);
        assert_eq!(111, entries[1].extra_lines);
        assert_eq!(111100, entries[1].bytes_size);
        assert_eq!(111.1, entries[1].size);
        assert_eq!("KBs".to_owned(), entries[1].size_measurement);
        assert_eq!(11100, entries[1].bytes_average_size);
        assert_eq!(11.1, entries[1].average_size);
        assert_eq!("KBs".to_owned(), entries[1].average_size_measurement);
        let datetime: DateTime<Local> = chrono::DateTime::from_str("2021-09-12 16:23:50 +03:00").unwrap();
        assert_eq!(datetime, dates[1]);

        assert_eq!(12, entries[2].files);
        assert_eq!(1222, entries[2].lines);
        assert_eq!(122, entries[2].code_lines);
        assert_eq!(122, entries[2].extra_lines);
        assert_eq!(122200, entries[2].bytes_size);
        assert_eq!(122.2, entries[2].size);
        assert_eq!("KBs".to_owned(), entries[2].size_measurement);
        assert_eq!(12200, entries[2].bytes_average_size);
        assert_eq!(12.2, entries[2].average_size);
        assert_eq!("KBs".to_owned(), entries[2].average_size_measurement);
        let datetime: DateTime<Local> = chrono::DateTime::from_str("2021-09-12 04:01:56 +03:00").unwrap();
        assert_eq!(datetime, dates[2]);
    }
}