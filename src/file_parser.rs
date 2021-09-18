use std::{io::{BufRead, BufReader}, str::{self, MatchIndices}};

use crate::*;


pub fn parse_file(file_name: &str, lang_name: &str, buf: &mut String, language_map: LanguageMapRef, config: &Configuration)
-> Result<FileStats,String> 
{
    let reader = BufReader::new(match File::open(file_name){
        Ok(f) => f,
        Err(x) => return Err(x.to_string())
    });

    parse_lines(reader, buf, &language_map.get(lang_name).unwrap(), config)
}

fn parse_lines(mut reader: BufReader<File>, buf: &mut String, language: &Language, config: &Configuration)
-> Result<FileStats,String>
{
    let mut file_stats = FileStats::default(&language.keywords);
    let mut is_comment_closed = true;
    let mut open_str_symbol = None::<String>;
    loop {
        buf.clear();
        match reader.read_line(buf) {
            Ok(u) => if u == 0 {return Ok(file_stats)},
            Err(x) => return Err(x.to_string())
        }
        file_stats.incr_lines();

        let line = buf.trim();
        if line.is_empty() { continue; }

        // Two different parsing functions to skip the unnecessary checks for langs that don't support multiline comments
        // for performance reasons
        let line_info = 
        if language.supports_multiline_comments() { 
            get_bounds_w_multiline_comments(line, language, is_comment_closed, &open_str_symbol)
        } else {
            get_bounds_only_single_line_comments(line, language, &open_str_symbol)
        };

        is_comment_closed = !line_info.is_comment_open_after;
        open_str_symbol = line_info.open_str_sybol_after;

        if let Some(x) = line_info.cleansed_string {
            let cleansed = x.trim();
            if config.braces_as_code || cleansed.len() > 2 || (cleansed != "{" && cleansed != "}" && cleansed != "};") {
                file_stats.incr_code_lines();
                add_keywords_if_any(cleansed, &language, &mut file_stats);
            }
        } else {
            if line_info.has_string_literal {file_stats.incr_code_lines();}
        }
    }
}


#[derive(Debug, PartialEq)]
struct LineInfo {
    cleansed_string: Option<String>,
    has_string_literal: bool,
    is_comment_open_after: bool,
    open_str_sybol_after: Option<String>
}


fn get_bounds_only_single_line_comments(line: &str, language: &Language, open_str_symbol: &Option<String>) -> LineInfo {
    let (str_indices, str_symbols) = get_str_indices_and_symbols(line, language, open_str_symbol);
    if open_str_symbol.is_some() && str_indices.is_empty() {
        return LineInfo::none_str(false, true, open_str_symbol.to_owned());
    }

    let comment_indices = line.match_indices(&language.comment_symbol).map(|x| x.0).collect::<Vec<usize>>();
    if str_indices.is_empty() && comment_indices.is_empty() {
        return LineInfo::with_str(line.to_owned(), false);
    }
    
    let mut relevant = String::with_capacity(line.len());
    let has_more_strs = |counter| counter < str_indices.len();
    let has_more_comments = |counter| counter < comment_indices.len(); 
    let next_symbol_is_comment = |comment_counter: usize, str_counter: usize| {
        if !has_more_comments(comment_counter) {return false;}
        if has_more_strs(str_counter) && comment_indices[comment_counter] > str_indices[str_counter] {
            return false;
        }
        true
    };
    let next_symbol_is_string = |comment_counter: usize, str_counter: usize| {
        if !has_more_strs(str_counter) {return false;}
        if has_more_comments(comment_counter)  && str_indices[str_counter] > comment_indices[comment_counter] {
            return false;
        }
        true
    };
    let advance_comment_counter_until = |index, comment_counter: &mut usize| {
        while *comment_counter < comment_indices.len() && comment_indices[*comment_counter] < index {
            *comment_counter += 1;
        }
    };

    let mut has_string_literal = false;
    let mut slice_start_index = 0;
    let mut is_str_open_m = open_str_symbol.is_some();
    let (mut str_counter, mut comment_counter) = (0,0);
    loop {
        if is_str_open_m {
            let index_after = str_indices[str_counter] + 1;
            
            if index_after >= line.len() {
                if relevant.is_empty() {return LineInfo::none_all(true);}
                else {return LineInfo::with_str(relevant,true);}
            } 
            
            is_str_open_m = false;
            str_counter += 1;
            if !has_more_strs(str_counter) && is_str_open_m {
                return get_LineInfo_with_str_symbol(relevant, &str_symbols[str_counter-1]);
            }
            
            advance_comment_counter_until(index_after, &mut comment_counter);
            slice_start_index = index_after;
            has_string_literal = true;
        } else {
            if next_symbol_is_string(comment_counter, str_counter) {
                let this_index = str_indices[str_counter];
                relevant.push_str(&line[slice_start_index..this_index]);
                str_counter += 1;
                if !has_more_strs(str_counter) {
                    return get_LineInfo_with_str_symbol(relevant, &str_symbols[str_counter-1]);
                }
                
                is_str_open_m = true;
                has_string_literal = true;
            } else if next_symbol_is_comment(comment_counter, str_counter) {
                relevant.push_str(&line[slice_start_index..comment_indices[comment_counter]]);
                
                if relevant.is_empty() {return LineInfo::none_str(false, has_string_literal, None);}
                else {return LineInfo::new(Some(relevant), has_string_literal, false, None);}
            } else {
                relevant.push_str(&line[slice_start_index..line.len()]);
                return LineInfo::with_str(relevant, has_string_literal);
            }
        }
    }
}

fn get_bounds_w_multiline_comments(line: &str, language: &Language, is_comment_closed: bool,
    open_str_symbol: &Option<String>) -> LineInfo
{
   let mut com_end_indices = get_com_end_indices(line, language);
   let (str_indices, str_symbols) = get_str_indices_and_symbols(line, language, open_str_symbol);
   
   if is_comment_closed {
       if open_str_symbol.is_some() && str_indices.is_empty() {
           return LineInfo::none_str(false, true, open_str_symbol.to_owned());
       } 
   } else {
       if com_end_indices.is_empty() {
           return LineInfo::with_open_comment();
       }
   }
   
   let comment_indices = line.match_indices(&language.comment_symbol)
       .filter_map(|x| {
           if !is_intersecting_with_multi_line_end_symbol(x.0, language.multiline_len(), &com_end_indices) {
               Some(x.0)
           } else {
               None
           }
       })
       .collect::<Vec<usize>>();
   let mut com_start_indices = get_com_start_indices(line, language, &comment_indices);
   if !com_end_indices.is_empty() && !com_start_indices.is_empty() {
       resolve_double_counting_of_adjacent_start_and_end_symbols(&mut com_start_indices, &mut com_end_indices,
           !is_comment_closed, language.multiline_len());
   }
   
   if str_indices.is_empty() && comment_indices.is_empty() && com_start_indices.is_empty() && com_end_indices.is_empty() {
       return LineInfo::with_str(line.to_owned(), false);
   }
   
   let mut relevant = String::with_capacity(line.len());
   let (mut start_com_counter, mut end_com_counter, mut str_counter, mut comment_counter) = (0,0,0,0); 
   let (mut is_com_open_m, mut is_str_open_m) = (!is_comment_closed, open_str_symbol.is_some());

   let has_more_comments = |counter| counter < comment_indices.len(); 
   let has_more_strs = |counter| counter < str_indices.len();
   let has_more_ends = |counter| counter < com_end_indices.len();
   let has_more_starts = |counter| counter < com_start_indices.len();
   let next_symbol_is_comment = |comment_counter: usize, str_counter: usize,
        start_counter: usize| {
       if !has_more_comments(comment_counter) {return false; }
       if has_more_strs(str_counter) && comment_indices[comment_counter] > str_indices[str_counter] {
           return false;
       }
       if has_more_starts(start_counter) && comment_indices[comment_counter] > com_start_indices[start_counter] {
           return false;
       }
       true
   };
   let next_symbol_is_string = |comment_counter: usize, str_counter: usize,
        start_counter: usize| {
       if !has_more_strs(str_counter) {return false;}
       if has_more_comments(comment_counter)  && str_indices[str_counter] > comment_indices[comment_counter] {
           return false;
       }
       if has_more_starts(start_counter) && str_indices[str_counter] > com_start_indices[start_counter] {
           return false;
       }
       true
   };
   let next_symbol_is_com_start = |comment_counter: usize, str_counter: usize,
        start_counter: usize| {
       if !has_more_starts(start_counter) {return false;}
       if has_more_comments(comment_counter) && com_start_indices[start_counter] > comment_indices[comment_counter] {
           return false;
       }
       if has_more_strs(str_counter) && com_start_indices[start_counter] > str_indices[str_counter] {
           return false;
       }
       true
   };
   let progress_counters_after = |index, comment_counter: &mut usize, str_counter: &mut usize,
       start_counter: &mut usize, end_counter: &mut usize| {
       while *comment_counter < comment_indices.len() && comment_indices[*comment_counter] < index {
           *comment_counter += 1;
       }
       while *str_counter < str_indices.len() && str_indices[*str_counter] < index {
           *str_counter += 1;
       }
       while *start_counter < com_start_indices.len() && com_start_indices[*start_counter] < index {
           *start_counter += 1;
       }
       while *end_counter < com_end_indices.len() && com_end_indices[*end_counter] < index {
           *end_counter += 1;
       }
   };
   let skipped_com_end_symbol = |last_symbol_index, end_com_counter, cur_index| {
       has_more_ends(end_com_counter) && com_end_indices[end_com_counter] < cur_index && com_end_indices[end_com_counter] >= last_symbol_index
   };

   let mut has_string_literal = false;
   let mut slice_start_index = 0;
   let mut last_symbol_index = 0;
   loop {
       if is_str_open_m {
           last_symbol_index = str_indices[str_counter];
           let index_after = last_symbol_index + 1;
           if index_after >= line.len() {
               if relevant.is_empty() {return LineInfo::none_all(true);}
               else {return LineInfo::with_str(relevant,true);}
           } 
           
           progress_counters_after(last_symbol_index, &mut comment_counter, &mut str_counter,
                   &mut start_com_counter, &mut end_com_counter);

           is_str_open_m = false;
           str_counter += 1;
           has_string_literal = true;
           slice_start_index = index_after;
       } else if is_com_open_m {
           last_symbol_index = com_end_indices[end_com_counter];
           let index_after = last_symbol_index + language.multiline_len();
           if index_after >= line.len() {
               if relevant.is_empty() {return LineInfo::none_all(has_string_literal);}
               else {return LineInfo::with_str(relevant,has_string_literal);}
           } 

           is_com_open_m = false;
           progress_counters_after(last_symbol_index, &mut comment_counter, &mut str_counter,
                   &mut start_com_counter, &mut end_com_counter);
           end_com_counter += 1;

           if has_more_strs(str_counter) && str_indices[str_counter] == index_after {
               is_str_open_m = true;
           } else if has_more_starts(start_com_counter) && com_start_indices[start_com_counter] == index_after {
               is_com_open_m = true;
           } else {
               slice_start_index = index_after; 
           }
       } else {
           if next_symbol_is_comment(comment_counter, str_counter, start_com_counter) {
               relevant.push_str(&line[slice_start_index..comment_indices[comment_counter]]);
               if relevant.is_empty() {return LineInfo::none_all(has_string_literal);}
               else {return LineInfo::with_str(relevant,has_string_literal);}
           } else if next_symbol_is_string(comment_counter, str_counter, start_com_counter) {
               let this_index = str_indices[str_counter];
               if skipped_com_end_symbol(last_symbol_index, end_com_counter, this_index) {
                   end_com_counter += 1;
               }
               relevant.push_str(&line[slice_start_index..this_index]);
               str_counter += 1;
               if !has_more_strs(str_counter) {
                   return get_LineInfo_with_str_symbol(relevant, &str_symbols[str_counter-1]);
               }
               
               is_str_open_m = true;
               has_string_literal = true;
               last_symbol_index = this_index;
           } else if next_symbol_is_com_start(comment_counter, str_counter, start_com_counter) {
               let this_index = com_start_indices[start_com_counter];
               if skipped_com_end_symbol(last_symbol_index, end_com_counter, this_index) {
                   end_com_counter += 1;
               }

               relevant.push_str(&line[slice_start_index..this_index]);
               if !has_more_ends(end_com_counter) {
                   if relevant.is_empty() {return LineInfo::with_open_comment();}
                   else {return LineInfo::new(Some(relevant), has_string_literal, true, None);}
               }
               
               is_com_open_m = true;
               start_com_counter += 1;
               last_symbol_index = this_index;
           } else {
               relevant.push_str(&line[slice_start_index..line.len()]);
               return LineInfo::with_str(relevant, has_string_literal);
           }
       }
   }
}

fn get_LineInfo_with_str_symbol(relevant: String, str_symbol: &str) -> LineInfo {
    if relevant.is_empty() {
        LineInfo::with_open_symbol(str_symbol.to_owned())
    } else {
        LineInfo::new(Some(relevant), true, false, Some(str_symbol.to_owned()))
    }
}

fn get_com_end_indices(line: &str, language: &Language) -> Vec<usize> {
    line.match_indices(language.mutliline_comment_end_symbol.as_ref().unwrap()).map(|x| x.0).collect::<Vec<usize>>()
}

fn get_com_start_indices(line: &str, language: &Language, comment_indices: &[usize]) -> Vec<usize> {
    line.match_indices(language.mutliline_comment_start_symbol.as_ref().unwrap())
    .filter_map(|x|{
        if !is_intersecting_with_comment_symbol(x.0, comment_indices) {
            Some(x.0)
        } else {
            None
        }
    })
    .collect::<Vec<usize>>()
}

fn resolve_double_counting_of_adjacent_start_and_end_symbols(start_indices: &mut Vec<usize>,
    end_indices: &mut Vec<usize>, is_comment_open: bool, multiline_len: usize) 
{
   fn resolve_collision(start_indices: &mut Vec<usize>, end_indices: &mut Vec<usize>, start_counter: &mut usize, 
       end_counter: &mut usize, is_comment_open_m: &mut bool, multiline_len: usize)
   {
       if *is_comment_open_m {
           start_indices.remove(*start_counter);
           if *start_counter < start_indices.len() && start_indices[*start_counter] <
                   end_indices[*end_counter] + multiline_len {
               start_indices.remove(*start_counter);
           }
           *end_counter += 1;
       } else {
           end_indices.remove(*end_counter);
           if *end_counter < end_indices.len() && end_indices[*end_counter] <
                   start_indices[*start_counter] + multiline_len {
               end_indices.remove(*end_counter);
           }
           *start_counter += 1;
       }
       *is_comment_open_m = !*is_comment_open_m;
   }

   let mut is_comment_open_m = is_comment_open;
   let (mut start_counter, mut end_counter) = (0,0);
   loop {
       if start_counter == start_indices.len() || end_counter == end_indices.len() {break;}

       let start_index = start_indices[start_counter];
       let end_index = end_indices[end_counter];

       if end_index > start_index && end_index < start_index + multiline_len ||
                start_index > end_index && start_index < end_index + multiline_len {
            resolve_collision(start_indices, end_indices, &mut start_counter, &mut end_counter, &mut is_comment_open_m, multiline_len);
       } else {
           if start_index < end_index {
               start_counter += 1;
               if start_counter < start_indices.len() {
                   if start_indices[start_counter] > end_index {
                       is_comment_open_m = true;
                   }
               } else {
                   break;
               }
           }
           else {
               end_counter += 1;
               if end_counter < end_indices.len() {
                   if end_indices[end_counter] > start_counter {
                       is_comment_open_m = false;
                   }
               } else {
                   break;
               }
           }
       }
   }
}


fn add_keywords_if_any(cleansed: &str, language: &Language, file_stats: &mut FileStats) {
    fn is_acceptable_prefix(prefix: &str) -> bool {
        prefix.is_empty() || prefix.ends_with(' ') || prefix.ends_with('}') || prefix.ends_with('{') || prefix.ends_with(',')
    }

    fn is_acceptable_suffix(suffix: &str) -> bool {
        suffix.is_empty() || suffix.starts_with(' ') || suffix.starts_with('}') || suffix.starts_with('{') || suffix.starts_with(',')
    }

    for keyword in &language.keywords {
        for alias in &keyword.aliases {
            let mut indices = cleansed.match_indices(alias).map(|x| x.0).collect::<Vec<usize>>();
            if indices.is_empty() {continue;}
            let alias_len = alias.len();

            //ignore indices that are directly next to each other
            let mut counter = 0;
            while !indices.is_empty() && counter < indices.len()-1 {
                if indices[counter] + alias_len == indices[counter+1] {
                    indices.remove(counter);
                    indices.remove(counter);
                } 
                counter += 1;
            }
            if indices.is_empty() {continue};

            let mut surroundings = vec![&cleansed[0..indices[0]]];
            for i in 1..indices.len() {
                surroundings.push(&cleansed[indices[i-1]+alias_len..indices[i]]);
            }
            surroundings.push(&cleansed[indices[indices.len()-1]+alias_len..cleansed.len()]);
            
            let surroundings_len = surroundings.len();
            let mut counter = 0;
            while counter < surroundings_len-1 {
                if is_acceptable_prefix(surroundings[counter]) && is_acceptable_suffix(surroundings[counter+1]) {
                    file_stats.incr_keyword(&keyword.descriptive_name);
                }
                counter += 1;
            }
        }
    }
}

fn get_str_indices_and_symbols(line: &str, language: &Language, open_str_symbol: &Option<String>) -> (Vec<usize>,Vec<String>) {
    fn is_not_escaped(pos: usize, bytes: &[u8]) -> bool {
        let mut slashes = 0;
        let mut offset = 1;
        while pos >= offset && bytes[pos - offset] == b'\\' {
            offset += 1;
            slashes += 1;
        } 
        slashes % 2 == 0
    }

    fn add_unescaped_indices(indices: &mut Vec<usize>, symbols: &mut Vec<String>, first_symbol: &str, first_val: usize, bytes: &[u8], iter: &mut MatchIndices<&String>) {
        if first_val == 0 {
            indices.push(first_val);
            symbols.push(first_symbol.to_owned());
        } else {
            if is_not_escaped(first_val, bytes) {
                indices.push(first_val);
                symbols.push(first_symbol.to_owned());
            }
        } 
        for x in iter {
            if is_not_escaped(x.0, bytes) {
                indices.push(x.0);
                symbols.push(x.1.to_owned());
            }
        }
    }

    fn add_non_intersecting(indices_1: &mut Vec<usize>, indices_2: &mut Vec<usize>, symbols_1: &mut Vec<String>, symbols_2: &mut Vec<String>,
            open_str_symbol: &Option<String>, merged_indices: &mut Vec<usize>, merged_symbols: &mut Vec<String>, language: &Language) 
    {
        let mut is_str_open = open_str_symbol.is_some();
        let (mut first_indicies, mut second_indicies, mut first_symbols, mut second_symbols) = {
            if let Some(x) = open_str_symbol {
                if language.string_symbols[0] == *x {
                    (indices_1, indices_2, symbols_1, symbols_2)
                } else {
                    (indices_2, indices_1, symbols_2, symbols_1)
                }
            } else {
                if indices_1[0] < indices_2[0] { 
                    (indices_1, indices_2, symbols_1, symbols_2)
                } else {
                    (indices_2, indices_1, symbols_2, symbols_1)
                }
            }
        };

        let (mut first_counter, mut second_counter) = (0,0);
        loop {
            if is_str_open {
                if first_counter >= first_indicies.len() {
                    return;
                }
                merged_indices.push(first_indicies[first_counter]);
                merged_symbols.push(first_symbols[first_counter].to_owned());
                while second_counter < second_indicies.len() && second_indicies[second_counter] < first_indicies[first_counter] {
                    second_counter += 1;
                } 
                is_str_open = false;
                first_counter += 1;
            } else {
                if second_counter >= second_indicies.len() {
                    while first_counter < first_indicies.len() {
                        merged_indices.push(first_indicies[first_counter]);
                        merged_symbols.push(first_symbols[first_counter].to_owned());
                        first_counter += 1;
                    }
                    return;
                } else if first_counter >= first_indicies.len() {
                    while second_counter < second_indicies.len() {
                        merged_indices.push(second_indicies[second_counter]);
                        merged_symbols.push(second_symbols[second_counter].to_owned());

                        second_counter += 1;
                    }
                    return;
                }

                if second_indicies[second_counter] < first_indicies[first_counter] {
                    let (temp_indicies, temp_symbols, temp_counter) = (first_indicies, first_symbols, first_counter);
                    first_indicies = second_indicies;
                    first_symbols = second_symbols;
                    first_counter = second_counter;
                    second_indicies = temp_indicies;
                    second_symbols = temp_symbols;
                    second_counter = temp_counter;
                } 

                merged_indices.push(first_indicies[first_counter]);
                merged_symbols.push(first_symbols[first_counter].to_owned());
                first_counter += 1;
                is_str_open = true;
            }
        }
    }

    let line_bytes = line.as_bytes();
    if language.string_symbols.len() == 2 {
        let mut iter_1 = line.match_indices(&language.string_symbols[0]);
        let mut iter_2 = line.match_indices(&language.string_symbols[1]);
        let first_match_1 = iter_1.next();
        let first_match_2 = iter_2.next();
        let mut indices  = Vec::with_capacity(6);
        let mut symbols  = Vec::with_capacity(6);
        if first_match_1.is_none() && first_match_2.is_none() {
            (vec![],vec![])
        } else if first_match_1.is_none() {
            if open_str_symbol.is_none() {
                add_unescaped_indices(&mut indices, &mut symbols, first_match_2.unwrap().1,
                        first_match_2.unwrap().0, line_bytes, &mut iter_2);
                (indices,symbols)
            } else {
                let open_str_symbol = open_str_symbol.as_ref().unwrap();
                if *open_str_symbol == language.string_symbols[1]{
                    add_unescaped_indices(&mut indices, &mut symbols, first_match_2.unwrap().1,
                            first_match_2.unwrap().0, line_bytes, &mut iter_2);
                    (indices,symbols)
                } else {
                    (vec![],vec![])
                }
            }
        } else if first_match_2.is_none() {
            if open_str_symbol.is_none() {
                add_unescaped_indices(&mut indices, &mut symbols, first_match_1.unwrap().1,
                            first_match_1.unwrap().0, line_bytes, &mut iter_1);
                (indices,symbols)
            } else {
                let open_str_symbol = open_str_symbol.as_ref().unwrap();
                if *open_str_symbol == language.string_symbols[0]{
                    add_unescaped_indices(&mut indices, &mut symbols, first_match_1.unwrap().1,
                            first_match_1.unwrap().0, line_bytes, &mut iter_1);
                    (indices,symbols)
                } else {
                    (vec![],vec![])
                }
            }
        } else {
            let mut indices_1 = Vec::with_capacity(6);
            let mut symbols_1 = Vec::with_capacity(6);
            let mut indices_2 = Vec::with_capacity(6);
            let mut symbols_2 = Vec::with_capacity(6);
            add_unescaped_indices(&mut indices_1, &mut symbols_1, first_match_1.unwrap().1,
                    first_match_1.unwrap().0, line_bytes, &mut iter_1);
            add_unescaped_indices(&mut indices_2, &mut symbols_2, first_match_2.unwrap().1,
                    first_match_2.unwrap().0, line_bytes, &mut iter_2);
            if indices_1.is_empty() && indices_2.is_empty() {
                (vec![],vec![])
            } else if indices_2.is_empty() {
                (indices_1,symbols_1)
            } else if indices_1.is_empty() {
                (indices_2, symbols_2)
            } else {
                add_non_intersecting(&mut indices_1, &mut indices_2, &mut symbols_1, &mut symbols_2,
                        open_str_symbol, &mut indices, &mut symbols, language);
                (indices,symbols)
            }
        }
    } else {
        let mut indicies = Vec::new();
        let mut symbols = Vec::new();
        line.match_indices(&language.string_symbols[0]).for_each(|x| {
            if is_not_escaped(x.0, &line_bytes) {
                indicies.push(x.0); symbols.push(x.1.to_owned());
            }
        });
        (indicies, symbols)
    }
}

fn is_intersecting_with_multi_line_end_symbol(index: usize, symbol_len: usize, end_vec: &[usize]) -> bool {
    for i in end_vec {
        if index < symbol_len {
            if *i == 0 {return true;}
        } else {
            if *i == index - symbol_len + 1 {return true;}    
        }
    }

    false
}

fn is_intersecting_with_comment_symbol(index: usize, comments_vec: &[usize]) -> bool {
    for i in comments_vec {
        if *i == index + 1 {return true;} 
    }

    false
}


impl LineInfo {
    pub fn none_str(is_comment_open_after: bool, has_string_literal: bool, open_str_sybol_after: Option<String>) -> LineInfo{
        LineInfo {
            cleansed_string: None,
            has_string_literal,
            is_comment_open_after,
            open_str_sybol_after
        }
    }

    pub fn with_str(cleansed_string: String, has_string_literal: bool) -> LineInfo {
        LineInfo {
            cleansed_string: Some(cleansed_string),
            has_string_literal,
            is_comment_open_after : false,
            open_str_sybol_after : None
        }
    }

    pub fn with_open_comment() -> LineInfo {
        LineInfo {
            cleansed_string: None,
            has_string_literal: false,
            is_comment_open_after: true,
            open_str_sybol_after: None
        }
    }

    pub fn with_open_symbol(symbol: String) -> LineInfo {
        LineInfo {
            cleansed_string: None,
            has_string_literal: true,
            is_comment_open_after: false,
            open_str_sybol_after : Some(symbol)
        }
    }

    pub fn from_slice(slice: &str) -> LineInfo {
        LineInfo {
            cleansed_string: Some(slice.to_owned()),
            has_string_literal: false,
            is_comment_open_after : false,
            open_str_sybol_after : None
        }
    }

    pub fn from_slice_w_literal(slice: &str) -> LineInfo {
        LineInfo {
            cleansed_string: Some(slice.to_owned()),
            has_string_literal: true,
            is_comment_open_after : false,
            open_str_sybol_after : None
        }
    }

    pub fn none_all(has_string_literal: bool) -> LineInfo {
        LineInfo {
            cleansed_string: None,
            has_string_literal,
            is_comment_open_after : false,
            open_str_sybol_after : None
        }
    }

    pub fn new(cleansed_string: Option<String>, has_string_literal: bool, is_comment_open_after: bool, open_str_sybol_after: Option<String>) -> LineInfo {
        LineInfo {
            cleansed_string,
            has_string_literal,
            is_comment_open_after,
            open_str_sybol_after
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use lazy_static::lazy_static;
        
    lazy_static! {
        static ref CLASS : Keyword = Keyword {
            descriptive_name : "classes".to_owned(),
            aliases : vec!["class".to_owned()]
        };

        static ref INTERFACE : Keyword = Keyword {
            descriptive_name : "interfaces".to_owned(),
            aliases : vec!["interface".to_owned()]
        };

        static ref ENUM : Keyword = Keyword {
            descriptive_name : "enums".to_owned(),
            aliases : vec!["enum".to_owned()]
        };

        static ref STRUCT : Keyword = Keyword {
            descriptive_name : "structs".to_owned(),
            aliases : vec!["struct".to_owned()]
        };

        static ref TRAIT : Keyword = Keyword {
            descriptive_name : "traits".to_owned(),
            aliases : vec!["trait".to_owned()]
        };

        static ref JAVA : Language = Language {
            name : "java".to_owned(),
            extensions : vec!["java".to_owned()],
            string_symbols : vec!["\"".to_owned()],
            comment_symbol : "//".to_owned(),
            mutliline_comment_start_symbol : Some("/*".to_owned()),
            mutliline_comment_end_symbol : Some("*/".to_owned()),
            keywords : vec![CLASS.clone(),INTERFACE.clone()]
        };

        static ref PYTHON : Language = Language {
            name : "py".to_owned(),
            extensions : vec!["py".to_owned()],
            string_symbols : vec!["\"".to_owned(),"'".to_owned()],
            comment_symbol : "#".to_owned(),
            mutliline_comment_start_symbol : None,
            mutliline_comment_end_symbol : None,
            keywords : vec![CLASS.clone()]
        };

        static ref RUST : Language = Language {
            name : "rust".to_owned(),
            extensions : vec!["rs".to_owned()],
            string_symbols : vec!["\"".to_owned()],
            comment_symbol : "//".to_owned(),
            mutliline_comment_start_symbol : None,
            mutliline_comment_end_symbol : None,
            keywords : vec![STRUCT.clone(),ENUM.clone(),TRAIT.clone()]
        };

        static ref LANGUAGE_MAP_REF : LanguageMapRef = Arc::new(io_handler::parse_supported_languages_to_map(&mut Vec::<String>::new()).unwrap().language_map);
    }

    #[test]
    fn test_correct_parsing_of_test_dir() {
        let mut buf = String::with_capacity(150);

        let result = parse_file("test_dir/lang_files/a.txt", "Java", &mut buf, LANGUAGE_MAP_REF.clone(), &Configuration::new(vec!["a".to_owned()]));
        let result = LanguageContentInfo::from(result.unwrap());
        assert_eq!(LanguageContentInfo::new(44, 13, hashmap!("classes".to_owned()=>3,"interfaces".to_owned()=>0)), result);
        buf.clear();
        let result = parse_file("test_dir/lang_files/a.txt", "C#", &mut buf, LANGUAGE_MAP_REF.clone(), &Configuration::new(vec!["a".to_owned()]));
        let result = LanguageContentInfo::from(result.unwrap());
        assert_eq!(LanguageContentInfo::new(44, 13, hashmap!("classes".to_owned()=>3,"interfaces".to_owned()=>0)), result);
        buf.clear();
        
        let result = parse_file("test_dir/lang_files/d.txt", "C#", &mut buf, LANGUAGE_MAP_REF.clone(), &Configuration::new(vec!["a".to_owned()]));
        let result = LanguageContentInfo::from(result.unwrap());
        assert_eq!(LanguageContentInfo::new(19, 7, hashmap!("classes".to_owned()=>5,"interfaces".to_owned()=>0)), result);
        buf.clear();
        let result = parse_file("test_dir/lang_files/d.txt", "Java", &mut buf, LANGUAGE_MAP_REF.clone(), &Configuration::new(vec!["a".to_owned()]));
        let result = LanguageContentInfo::from(result.unwrap());
        assert_eq!(LanguageContentInfo::new(19, 7, hashmap!("classes".to_owned()=>5,"interfaces".to_owned()=>0)), result);
        buf.clear();

        let result = parse_file("test_dir/lang_files/b.txt", "Java", &mut buf, LANGUAGE_MAP_REF.clone(), &Configuration::new(vec!["a".to_owned()]));
        let result = LanguageContentInfo::from(result.unwrap());
        assert_eq!(LanguageContentInfo::new(19, 11, hashmap!("classes".to_owned()=>7,"interfaces".to_owned()=>0)), result);
        buf.clear();

        let result = parse_file("test_dir/lang_files/c.txt", "Python", &mut buf, LANGUAGE_MAP_REF.clone(), &Configuration::new(vec!["a".to_owned()]));
        let result = LanguageContentInfo::from(result.unwrap());
        assert_eq!(LanguageContentInfo::new(11, 6, hashmap!("classes".to_owned()=>2)), result);
        buf.clear();
    }

    fn set_occurances(map: &mut HashMap<String,usize>, classes: usize, interfaces: usize) {
        map.insert("classes".to_owned(), classes);
        map.insert("interfaces".to_owned(), interfaces);
    }
    
    #[test]
    fn finds_keywords_correctly() {
        let line = String::from("Hello world!");
        let mut file_stats =  FileStats::default(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA, &mut file_stats);
        assert_eq!(make_file_stats(0,0), file_stats);

        let line = String::from("class");
        let mut file_stats =  FileStats::default(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA, &mut file_stats);
        assert_eq!(make_file_stats(1,0), file_stats);

        let line = String::from("1class");
        let mut file_stats =  FileStats::default(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA, &mut file_stats);
        assert_eq!(make_file_stats(0,0), file_stats);

        let line = String::from("hello class word!");
        let mut file_stats =  FileStats::default(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA, &mut file_stats);
        assert_eq!(make_file_stats(1,0), file_stats);

        let line = String::from("class class class");
        let mut file_stats =  FileStats::default(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA, &mut file_stats);
        assert_eq!(make_file_stats(3,0), file_stats);

        let line = String::from("classclass");
        let mut file_stats =  FileStats::default(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA, &mut file_stats);
        assert_eq!(make_file_stats(0,0), file_stats);

        let line = String::from("hello,class{word!");
        let mut file_stats =  FileStats::default(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA, &mut file_stats);
        assert_eq!(make_file_stats(1,0), file_stats);
        
        let line = String::from("classe,");
        let mut file_stats =  FileStats::default(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA, &mut file_stats);
        assert_eq!(make_file_stats(0,0), file_stats);
        
        let line = String::from("class interfaceclass classinterface interface");
        let mut file_stats =  FileStats::default(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA, &mut file_stats);
        assert_eq!(make_file_stats(1,1), file_stats);
        
        let line = String::from("{class,interface}");
        let mut file_stats =  FileStats::default(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA, &mut file_stats);
        assert_eq!(make_file_stats(1,1), file_stats);
        
        let line = String::from("{class.interface}");
        let mut file_stats =  FileStats::default(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA, &mut file_stats);
        assert_eq!(make_file_stats(0,0), file_stats);
    }

    fn make_file_stats(class_occurances: usize, interface_occurances: usize) -> FileStats {
        fn get_keyword_map(class_occurances: usize, interface_occurances: usize) -> HashMap<String,usize> {
            let mut map = HashMap::<String,usize>::new();
            map.insert(CLASS.descriptive_name.clone(), class_occurances);
            map.insert(INTERFACE.descriptive_name.clone(), interface_occurances);
            map
        }

        FileStats {
            lines: 0,
            code_lines: 0,
            keyword_occurences : get_keyword_map(class_occurances, interface_occurances)
        }
    }

    #[test]
    fn get_str_indicies_test() {
        let single_str_opt = &Some("'".to_owned());
        let double_str_opt = &Some("\"".to_owned());
        let line = String::from("Hello");
        assert_eq!(Vec::<usize>::new(),get_str_indices_and_symbols(&line, &PYTHON, &None).0);
        let line = String::from("\"Hello\"");
        assert_eq!((vec![0,6],vec!["\"".to_owned(),"\"".to_owned()]),get_str_indices_and_symbols(&line, &PYTHON, &None));
        let line = String::from("\"'\"Hello");
        assert_eq!((vec![0,2],vec!["\"".to_owned(),"\"".to_owned()]),get_str_indices_and_symbols(&line, &PYTHON, &None));
        assert_eq!((vec![1,2],vec!["'".to_owned(),"\"".to_owned()]),get_str_indices_and_symbols(&line, &PYTHON, single_str_opt));
        assert_eq!((vec![0,1],vec!["\"".to_owned(),"'".to_owned()]),get_str_indices_and_symbols(&line, &PYTHON, double_str_opt));
        let line = String::from("''\"\"Hello");
        assert_eq!(vec![0,1,2,3],get_str_indices_and_symbols(&line, &PYTHON, &None).0);
        assert_eq!(vec![0,1],get_str_indices_and_symbols(&line, &PYTHON, single_str_opt).0);
        assert_eq!(vec![2,3],get_str_indices_and_symbols(&line, &PYTHON, double_str_opt).0);
        let line = String::from("'\"'\"''\"He'l\"lo");
        assert_eq!(vec![0,2,3,6,9],get_str_indices_and_symbols(&line, &PYTHON, &None).0);
        assert_eq!(vec![0,1,3,4,5,6,11],get_str_indices_and_symbols(&line, &PYTHON, single_str_opt).0);
        assert_eq!(vec![1,2,4,5,9,11],get_str_indices_and_symbols(&line, &PYTHON, double_str_opt).0);
        assert_eq!(vec![1,3,6,11],get_str_indices_and_symbols(&line, &JAVA, double_str_opt).0);
        let line = String::from(r#"\'\\'\\'\\\''"#);
        assert_eq!(vec![4,7,12], get_str_indices_and_symbols(&line, &PYTHON, &None).0);
        assert_eq!(vec![4,7,12], get_str_indices_and_symbols(&line, &PYTHON, single_str_opt).0);
        let line = String::from(r#"["❌🔤","💭🔜","📗","📘",]"#);
        assert!(get_str_indices_and_symbols(&line, &PYTHON, &None).0.len() == 8);
        assert!(get_str_indices_and_symbols(&line, &RUST, double_str_opt).0.len() == 8);
        let line = String::from(r#"[\'⣾\', '⣷', '⣯', '⣟', '⡿']"#); 
        assert!(get_str_indices_and_symbols(&line, &PYTHON, &None).0.len() == 8);
        assert!(get_str_indices_and_symbols(&line, &RUST, &None).0.len() == 0);
        let line = String::from(r#"['⣾", '⣷", '⣯"]"#); 
        assert_eq!(vec!["'".to_owned(),"'".to_owned(),"\"".to_owned(),"\"".to_owned()], 
                get_str_indices_and_symbols(&line, &PYTHON, &None).1);
        let line = String::from(r#"'\'\'\''"#); 
        assert_eq!(vec![0,7], get_str_indices_and_symbols(&line, &PYTHON, &None).0);
        let line = String::from(r#""\"\\"""#); //  """\"""
        assert_eq!(vec![0,5,6], get_str_indices_and_symbols(&line, &RUST, &None).0);
        assert_eq!(vec![0,5,6], get_str_indices_and_symbols(&line, &PYTHON, &None).0);
        let line = String::from(r#"\\\"\"\\""#); 
        assert_eq!(vec![8], get_str_indices_and_symbols(&line, &RUST, &None).0);
        assert_eq!(vec![8], get_str_indices_and_symbols(&line, &PYTHON, &None).0);
    }

    #[test]
    fn double_counting_resolution() {
        // /*Hello*//* world*//*
        let (mut start_indices, mut end_indices) = (vec![0,9,19],vec![7,17]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0,9,19],vec![7,17]));
        // /**//**/
        let (mut start_indices, mut end_indices) = (vec![0,4],vec![2,6]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0,4],vec![2,6]));
        // /*/**/*/
        let (mut start_indices, mut end_indices) = (vec![0,2],vec![4,6]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0,2],vec![4,6]));

        // /* */*
        let (mut start_indices, mut end_indices) = (vec![0,4],vec![3]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0],vec![3]));

        // */* /*/
        let (mut start_indices, mut end_indices) = (vec![1,4],vec![0,5]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![1],vec![5]));
        let (mut start_indices, mut end_indices) = (vec![1,4],vec![0,5]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, true, 2);
        assert_eq!((start_indices, end_indices), (vec![4],vec![0]));

        // /*/*/ */*/ /* */
        let (mut start_indices, mut end_indices) = (vec![0,2,7,11],vec![1,3,6,8,14]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0,7,11],vec![3,14])); 
        let (mut start_indices, mut end_indices) = (vec![0,2,7,11],vec![1,3,6,8,14]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, true, 2);
        assert_eq!((start_indices, end_indices), (vec![7,11],vec![1,3,14])); 
 
        // /*/*/ */*/
        let (mut start_indices, mut end_indices) = (vec![0,2,7],vec![1,3,6,8]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0,7],vec![3])); 
        let (mut start_indices, mut end_indices) = (vec![0,2,7],vec![1,3,6,8]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, true, 2);
        assert_eq!((start_indices, end_indices), (vec![7],vec![1,3]));

        // /* */*/*//*
        let (mut start_indices, mut end_indices) = (vec![0,4,6,9],vec![3,5,7]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0,6,9],vec![3]));
        let (mut start_indices, mut end_indices) = (vec![0,4,6,9],vec![3,5,7]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, true, 2);
        assert_eq!((start_indices, end_indices), (vec![0,6,9],vec![3]));
    }
    
    #[test]
    fn gets_bounds_PYTHON() {
        let line = String::from("[\"\\\"\\\"\\\"\",\"'''\",\"\\\"\",\"'\",]");
        assert_eq!(LineInfo::new(Some("[,,,,]".to_owned()),true,false,None),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        let line = String::from("\\''\''");
        assert_eq!(LineInfo::new(Some("\\\'".to_owned()),true,false,Some("\'".to_owned())), get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        assert_eq!(LineInfo::none_all(true), get_bounds_only_single_line_comments(&line, &PYTHON, &Some("\'".to_owned())));
        let line = String::from("\'\\'\\'\\\''"); 
        assert_eq!(LineInfo::new(None,true,false,None), get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        
        let single_str_opt = &Some("'".to_owned());
        let double_str_opt = &Some("\"".to_owned());
        let single_str_li = LineInfo::with_open_symbol("'".to_string());
        let double_str_li = LineInfo::with_open_symbol("\"".to_string());
    
        let line = String::from("Hello world!");
        assert_eq!(LineInfo::from_slice("Hello world!"),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        assert_eq!(single_str_li,get_bounds_only_single_line_comments(&line, &PYTHON, single_str_opt));
        
        //testing comments
        let line = String::from("#Hello world!");
        assert_eq!(single_str_li,get_bounds_only_single_line_comments(&line, &PYTHON, single_str_opt));
        let line = String::from("Hello world!#");
        assert_eq!(LineInfo::from_slice("Hello world!"),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        let line = String::from("Hello# world!");
        assert_eq!(LineInfo::from_slice("Hello"),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        assert_eq!(single_str_li,get_bounds_only_single_line_comments(&line, &PYTHON, single_str_opt));
        let line = String::from("Hello## world!");
        assert_eq!(LineInfo::from_slice("Hello"),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        let line = String::from("#Hello# world!");
        assert_eq!(single_str_li,get_bounds_only_single_line_comments(&line, &PYTHON, single_str_opt));
        
        //testing strings 
        let line = String::from("\"Hello world!#");
        assert_eq!(double_str_li,get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        let line = String::from("\"Hello\" world!");
        assert_eq!(LineInfo::from_slice_w_literal(" world!"),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        assert_eq!(LineInfo::new(Some("Hello".to_owned()), true, false, Some("\"".to_owned())),get_bounds_only_single_line_comments(&line, &PYTHON, double_str_opt));
        let line = String::from("Hello world!\"");
        assert_eq!(LineInfo::new(Some("Hello world!".to_owned()), true, false, Some("\"".to_owned())),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        let line = String::from("\"'Hello'\" world!");
        assert_eq!(LineInfo::from_slice_w_literal(" world!"),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        let line = String::from("'Hello' world!");
        assert_eq!(LineInfo::from_slice_w_literal(" world!"),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        let line = String::from("'\"He'llo'\" world!'");
        assert_eq!(LineInfo::from_slice_w_literal("llo"),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        assert_eq!(LineInfo::new(Some("He".to_owned()), true, false, Some("\"".to_owned())),get_bounds_only_single_line_comments(&line, &PYTHON, double_str_opt));
        let line = String::from(r#""""Hello""#);
        assert_eq!(LineInfo::new(None, true, false, None), get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        assert_eq!(LineInfo::new(Some("Hello".to_owned()), true, false, Some("\"".to_owned())), get_bounds_only_single_line_comments(&line, &PYTHON, &double_str_opt));
        let line = String::from(r#"['⣯', '⣟"#); 
        assert_eq!(LineInfo::new(Some("[, ".to_owned()),true,false,Some("\'".to_owned())), get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        
        //test mixed
        let line = String::from("'Hello#' world!'");
        assert_eq!(LineInfo::new(Some(" world!".to_owned()), true, false, Some("'".to_owned())),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        assert_eq!(LineInfo::from_slice_w_literal("Hello"),get_bounds_only_single_line_comments(&line, &PYTHON, single_str_opt));
        let line = String::from("'Hello'# world!'");
        assert_eq!(LineInfo::none_all(true),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        assert_eq!(LineInfo::from_slice_w_literal("Hello"),get_bounds_only_single_line_comments(&line, &PYTHON, single_str_opt));
        let line = String::from("''#Hello");
        assert_eq!(LineInfo::none_all(true),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        let line = String::from("'''#'''Hello world!'");
        assert_eq!(LineInfo::new(Some("Hello world!".to_owned()), true, false, Some("'".to_owned())),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        assert_eq!(LineInfo::none_all(true),get_bounds_only_single_line_comments(&line, &PYTHON, single_str_opt));
        assert_eq!(LineInfo::with_open_symbol("\"".to_owned()),get_bounds_only_single_line_comments(&line, &PYTHON, double_str_opt));
        let line = String::from("Hello'###'\"world!\"");
        assert_eq!(LineInfo::from_slice_w_literal("Hello"),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        assert_eq!(LineInfo::none_all(true),get_bounds_only_single_line_comments(&line, &PYTHON, single_str_opt));
        assert_eq!(LineInfo::new(Some("world!".to_owned()), true, false, Some("\"".to_owned())),get_bounds_only_single_line_comments(&line, &PYTHON, double_str_opt));
        let line = String::from("\"//'''\"Hello'\"world!");
        assert_eq!(LineInfo::new(Some("Hello".to_owned()), true, false, Some("'".to_owned())),get_bounds_only_single_line_comments(&line, &PYTHON, &None));
        assert_eq!(LineInfo::from_slice_w_literal("world!"),get_bounds_only_single_line_comments(&line, &PYTHON, single_str_opt));
        assert_eq!(LineInfo::new(Some("//".to_owned()), true, false, Some("\"".to_owned())),get_bounds_only_single_line_comments(&line, &PYTHON, double_str_opt));
    }
    
    #[test]
    fn gets_bounds_JAVA() {
        let double_str_opt = &Some("\"".to_owned());

        let line = String::from("Hello world!");
        assert_eq!(LineInfo::with_open_comment(),get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::with_open_symbol("\"".to_string()),get_bounds_w_multiline_comments(&line, &JAVA, true, double_str_opt));
        assert_eq!(LineInfo::from_slice("Hello world!"),get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        
        //testing only multiline comment combinations
        let line = String::from("*/Hello world!");
        assert_eq!(LineInfo::from_slice("Hello world!"),get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::from_slice("*/Hello world!"),get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("Hello/* ffd /**//*erer */ world!");
        assert_eq!(LineInfo::from_slice(" world!"),get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::from_slice("Hello world!"),get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("Hello*//**//**/ world!");
        assert_eq!(LineInfo::from_slice(" world!"),get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::from_slice("Hello*/ world!"),get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("*//*Hello/**/ world!");
        assert_eq!(LineInfo::from_slice(" world!"),get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::from_slice("*/ world!"),get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("Hello world*/");
        assert_eq!(LineInfo::none_all(false), get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        let line = String::from("*/Hello world!/**/");
        assert_eq!(LineInfo::from_slice("Hello world!"), get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        let line = String::from("Hello world*//**/");
        assert_eq!(LineInfo::none_all(false), get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        let line = String::from("*/He/**//*llo world*/!/**/");
        assert_eq!(LineInfo::from_slice("He!"), get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        let line = String::from("Hello world*/!");
        assert_eq!(LineInfo::from_slice("!"), get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        let line = String::from("/*H*/ello world/*!");
        assert_eq!(LineInfo::new(Some("ello world".to_string()), false, true, None), get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::new(Some("ello world".to_string()), false, true, None), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        
        //testing only string symbols
        let line = String::from("\"");
        assert_eq!(LineInfo::with_open_symbol("\"".to_string()), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("\"Hello\"");
        assert_eq!(LineInfo::new(Some("Hello".to_string()), true, false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &JAVA, true, double_str_opt));
        assert_eq!(LineInfo::none_all(true), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("\"\"Hello");
        assert_eq!(LineInfo::with_open_symbol("\"".to_string()), get_bounds_w_multiline_comments(&line, &JAVA, true, double_str_opt));
        assert_eq!(LineInfo::from_slice_w_literal("Hello"), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("\"\"");
        assert_eq!(LineInfo::with_open_symbol("\"".to_string()), get_bounds_w_multiline_comments(&line, &JAVA, true, double_str_opt));
        assert_eq!(LineInfo::none_all(true), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("\"\"Hello");
        assert_eq!(LineInfo::from_slice_w_literal("Hello"), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line  = String::from("Hel\"\"lo");
        assert_eq!(LineInfo::from_slice_w_literal("Hello"), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("\"\"He\"\"\"ll\"o");
        assert_eq!(LineInfo::from_slice_w_literal("Heo"), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from(r#""""Hello""#);
        assert_eq!(LineInfo::new(None, true, false, None), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        assert_eq!(LineInfo::new(Some("Hello".to_owned()), true, false, Some("\"".to_owned())), get_bounds_w_multiline_comments(&line, &JAVA, true, &double_str_opt));
        
        //testing only comments
        let line = String::from("//");
        assert_eq!(LineInfo::none_all(false), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("Hello//");
        assert_eq!(LineInfo::from_slice("Hello"), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        assert_eq!(LineInfo::with_open_comment(), get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::with_open_symbol("\"".to_string()), get_bounds_w_multiline_comments(&line, &JAVA, true, double_str_opt));
        let line = String::from("//Hello");
        assert_eq!(LineInfo::none_all(false), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("////Hello");
        assert_eq!(LineInfo::none_all(false), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("He//llo//");
        assert_eq!(LineInfo::from_slice("He"), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        
        //testing mixed
        let line = String::from("\"\"\"//\"\"\"Hello world!");
        assert_eq!(LineInfo::from_slice_w_literal("Hello world!"),get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        assert_eq!(LineInfo::none_all(true),get_bounds_w_multiline_comments(&line, &JAVA, true, double_str_opt));
        let line = String::from("\"\"one\"//\"\"\"Hello world!");
        assert_eq!(LineInfo::from_slice_w_literal("oneHello world!"),get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        let line = String::from("\"He\"/*l*/lo//fd");
        assert_eq!(LineInfo::from_slice_w_literal("lo"), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        assert_eq!(LineInfo::new(Some("He".to_string()), true, false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &JAVA, true, double_str_opt));
        assert_eq!(LineInfo::from_slice("lo"), get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        let line = String::from("//\"/**/dfd\"");
        assert_eq!(LineInfo::none_all(false), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        assert_eq!(LineInfo::new(Some("dfd".to_string()), true, false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::new(Some("dfd".to_string()), true, false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &JAVA, true, double_str_opt));
        
        let line  = String::from(
            "Hello /* \
            mefm \" */ \" \
            //*/world!"
        );
        assert_eq!(LineInfo::new(Some("Hello  ".to_string()), true, false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &JAVA, true, &None));
        assert_eq!(LineInfo::new(Some(" ".to_string()), true, false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::new(Some(" */ ".to_string()), true, false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &JAVA, true, double_str_opt));
    }
}