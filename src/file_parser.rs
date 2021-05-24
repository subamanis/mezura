use std::{collections::binary_heap::Iter, ops::{DerefMut, Index}, str::{self, MatchIndices}};

use crate::*;


#[inline]
pub fn parse_file(file_name: &String, buf: &mut String, extension_map: ExtMapRef) -> Result<FileStats,ParseFilesError> {
    let extension_str = match Path::new(&file_name).extension() {
        Some(x) => match x.to_str() {
            Some(y) => y,
            None => return Err(ParseFilesError::FaultyFile)
        },
        None => return Err(ParseFilesError::FaultyFile)
    };
    
    let reader = BufReader::new(match File::open(file_name){
        Ok(f) => f,
        Err(_) => return Err(ParseFilesError::FaultyFile)
    });

    parse_lines(reader, buf, &extension_map.get(extension_str).unwrap())
}

#[inline]
fn parse_lines(mut reader: BufReader<File>, buf: &mut String, extension: &Extension) -> Result<FileStats,ParseFilesError> {
    let mut file_stats = FileStats::default(&extension.keywords);
    let (mut is_string_closed, mut is_comment_closed) = (true, true);
    loop {
        buf.clear();
        match reader.read_line(buf) {
            Ok(u) => if u == 0 {return Ok(file_stats)},
            Err(_) => return Err(ParseFilesError::FaultyFile)
        }
        file_stats.incr_lines();

        let line = buf.trim();
        if line.len() == 0 {
            continue;
        }

        let words = buf.split_whitespace().collect::<Vec<&str>>();        
        for word in words {
        }
    }
}

fn get_str_indices(line: &String, extension: &Extension, open_str_symbol: &Option<String>) -> Vec<usize> {
    fn add_unescaped_indices(indices: &mut Vec<usize>, first_val: usize, bytes: &[u8], iter: &mut MatchIndices<&String>) {
        if first_val == 0 {
            indices.push(first_val);
        } else {
            if bytes[first_val-1] != b'\\' {
                indices.push(first_val);
            }
        } 
        while let Some(x) = iter.next() {
            if bytes[x.0-1] != b'\\' {
                indices.push(x.0);
            }
        }
    }

    fn add_non_intersecting(
         indices_1: &mut Vec<usize>, indices_2: &mut Vec<usize>, open_str_symbol: &Option<String>,
         merged_indices: &mut Vec<usize>, extension: &Extension) 
    {
        let mut is_str_open = if open_str_symbol.is_some() {true} else {false};
        let (mut first, mut second) = {
            if let Some(x) = open_str_symbol {
                if extension.string_symbols[0] == *x {
                    (indices_1, indices_2)
                } else {
                    (indices_2, indices_1)
                }
            } else {
                if indices_1[0] < indices_2[0] {
                    (indices_1, indices_2)
                } else {
                    (indices_2, indices_1)
                }
            }
        };

        let (mut first_counter, mut second_counter) = (0,0);
        let is_string_open = open_str_symbol.is_none();
        loop {
            if is_str_open {
                if first_counter >= first.len() {
                    return;
                }
                merged_indices.push(first[first_counter]);
                while second_counter < second.len() && second[second_counter] < first[first_counter] {
                    second_counter += 1;
                } 
                is_str_open = false;
                first_counter += 1;
            } else {
                if second_counter >= second.len() {
                    while first_counter < first.len() {
                        merged_indices.push(first[first_counter]);
                        first_counter += 1;
                    }
                    return;
                } else if first_counter >= first.len() {
                    while second_counter < second.len() {
                        merged_indices.push(second[second_counter]);
                        second_counter += 1;
                    }
                    return;
                }

                if second[second_counter] < first[first_counter] {
                    let (temp, temp_counter) = (first, first_counter);
                    first = second;
                    first_counter = second_counter;
                    second = temp;
                    second_counter = temp_counter;
                } 

                merged_indices.push(first[first_counter]);
                first_counter += 1;
                is_str_open = true;
            }
        }
    }

    if extension.string_symbols.len() == 2 {
        let mut iter_1 = line.match_indices(&extension.string_symbols[0]);
        let mut iter_2 = line.match_indices(&extension.string_symbols[1]);
        let first_index_1 = iter_1.next();
        let first_index_2 = iter_2.next();
        let mut indices  = Vec::with_capacity(6);
        let lines_bytes = line.as_bytes();
        if first_index_1.is_none() && first_index_2.is_none() {
            Vec::<usize>::new()
        } else if first_index_1.is_none() {
            if open_str_symbol.is_none() {
                add_unescaped_indices(&mut indices, first_index_2.unwrap().0, lines_bytes, &mut iter_2);
                indices
            } else {
                let open_str_symbol = open_str_symbol.as_ref().unwrap();
                if *open_str_symbol == extension.string_symbols[1]{
                    add_unescaped_indices(&mut indices, first_index_2.unwrap().0, lines_bytes, &mut iter_2);
                    indices
                } else {
                    Vec::<usize>::new()
                }
            }
        } else if first_index_2.is_none() {
            if open_str_symbol.is_none() {
                add_unescaped_indices(&mut indices, first_index_1.unwrap().0, lines_bytes, &mut iter_1);
            indices
            } else {
                let open_str_symbol = open_str_symbol.as_ref().unwrap();
                if *open_str_symbol == extension.string_symbols[0]{
                    add_unescaped_indices(&mut indices, first_index_1.unwrap().0, lines_bytes, &mut iter_1);
                    indices
                } else {
                    Vec::<usize>::new()
                }
            }
        } else {
            let mut indices_1 = Vec::<usize>::with_capacity(6);
            let mut indices_2 = Vec::<usize>::with_capacity(6);
            let first_index_1 = first_index_1.unwrap().0;
            let first_index_2 = first_index_2.unwrap().0;
            add_unescaped_indices(&mut indices_1, first_index_1, lines_bytes, &mut iter_1);
            add_unescaped_indices(&mut indices_2, first_index_2, lines_bytes, &mut iter_2);
            add_non_intersecting(&mut indices_1, &mut indices_2, open_str_symbol, &mut indices, extension);

            // if first_index_1 < first_index_2 {
            //     add_unescaped_indices(&mut indices, first_index_1, lines_bytes, &mut iter_1);
            // } else {
            //     add_unescaped_indices(&mut indices, first_index_2, lines_bytes, &mut iter_2);
            // }
            indices
        }
    } else {
        line.match_indices(&extension.string_symbols[0]).map(|x| x.0).collect()
    }
}

fn is_intersecting_with_multi_line_end_symbol(index: usize, symbol_len: usize, end_vec: &Vec<usize>) -> bool {
    for i in end_vec {
        if index < symbol_len {
            if *i == 0 {return true;}
        } else {
            if *i == index - symbol_len + 1 {return true;}    
        }
    }

    false
}

fn is_intersecting_with_comment_symbol(index: usize, comments_vec: &Vec<usize>) -> bool {
    for i in comments_vec {
        if *i == index + 1 {return true;} 
    }

    false
}

#[derive(Debug, PartialEq)]
struct LineInfo {
    cleansed_string: Option<String>,
    is_comment_open_after: bool,
    open_str_sybol_after: Option<String>
}

impl LineInfo {
    pub fn none_str(is_comment_open_after: bool, open_str_sybol_after: Option<String>) -> LineInfo{
        LineInfo {
            cleansed_string: None,
            is_comment_open_after,
            open_str_sybol_after: open_str_sybol_after
        }
    }

    pub fn with_str(cleansed_string: String) -> LineInfo {
        LineInfo {
            cleansed_string: Some(cleansed_string),
            is_comment_open_after : false,
            open_str_sybol_after : None
        }
    }

    pub fn with_open_comment() -> LineInfo {
        LineInfo {
            cleansed_string: None,
            is_comment_open_after: true,
            open_str_sybol_after: None
        }
    }

    pub fn with_open_symbol(symbol: String) -> LineInfo {
        LineInfo {
            cleansed_string: None,
            is_comment_open_after: false,
            open_str_sybol_after : Some(symbol)
        }
    }

    pub fn from_slice(slice: &str) -> LineInfo {
        LineInfo {
            cleansed_string: Some(slice.to_owned()),
            is_comment_open_after : false,
            open_str_sybol_after : None
        }
    }

    pub fn none_all() -> LineInfo {
        LineInfo {
            cleansed_string: None,
            is_comment_open_after : false,
            open_str_sybol_after : None
        }
    }

    pub fn new(cleansed_string: Option<String>, is_comment_open_after: bool, open_str_sybol_after: Option<String>) -> LineInfo {
        LineInfo {
            cleansed_string,
            is_comment_open_after,
            open_str_sybol_after: open_str_sybol_after
        }
    }
}

fn get_bounds_only_single_line_comments(line: &String, extension: &Extension, open_str_symbol: &Option<String>) -> LineInfo {
    let str_indices = get_str_indices(line, extension, open_str_symbol);
    if open_str_symbol.is_some() && str_indices.is_empty() {
        return LineInfo::none_str(false, open_str_symbol.to_owned());
    }

    let comment_indices = line.match_indices(&extension.comment_symbol).map(|x| x.0).collect::<Vec<usize>>();
    if str_indices.is_empty() && comment_indices.is_empty() {
        return LineInfo::with_str(line.to_owned());
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

    let mut slice_start_index = 0;
    let mut is_str_open_m = open_str_symbol.is_some();
    let (mut str_counter, mut comment_counter) = (0,0);
    loop {
        if is_str_open_m {
            let mut index_after = str_indices[str_counter] + 1;
            str_counter += 1;
            is_str_open_m = false;
            loop {
                if index_after >= line.len() {
                    if is_str_open_m {
                        let str_symbol = line.chars().nth(str_indices[str_indices.len()-1]).unwrap().to_string();
                        if relevant.is_empty() {return LineInfo::with_open_symbol(str_symbol)}
                        else {return LineInfo::new(Some(relevant), false, Some(str_symbol));}
                    } else {
                        if relevant.is_empty() {return LineInfo::none_all();}
                        else {return LineInfo::with_str(relevant);}
                    }
                }
                
                //@TODO: instead of contains, check with something like contains after the current index.
                if str_indices.contains(&index_after) {
                    is_str_open_m = !is_str_open_m;
                    str_counter += 1;
                } else if comment_indices.contains(&index_after) {
                    if !is_str_open_m {
                        if relevant.is_empty() {return LineInfo::none_str(false, None);}
                        else {return LineInfo::new(Some(relevant), false, None);}
                    } else {
                        comment_counter += 1;
                    }
                } else {
                    break;
                }
                index_after += 1;
            }

            if !has_more_strs(str_counter) && is_str_open_m {
                if relevant.is_empty() {return LineInfo::with_open_symbol(line.chars().nth(str_indices[str_indices.len()-1]).unwrap().to_string());}
                else {return LineInfo::new(Some(relevant), false, Some(line.chars().nth(str_indices[str_indices.len()-1]).unwrap().to_string()));}
            }

            advance_comment_counter_until(index_after, &mut comment_counter);

            slice_start_index = index_after;
        } else {
            if next_symbol_is_string(comment_counter, str_counter) {
                let this_index = str_indices[str_counter];
                relevant.push_str(&line[slice_start_index..this_index]);
                str_counter += 1;
                if !has_more_strs(str_counter) {
                    if relevant.is_empty() {return LineInfo::with_open_symbol(line.chars().nth(this_index).unwrap().to_string());}
                    else {return LineInfo::new(Some(relevant), false, Some(line.chars().nth(this_index).unwrap().to_string()));}
                }
                
                is_str_open_m = true;
            } else if next_symbol_is_comment(comment_counter, str_counter) {
                relevant.push_str(&line[slice_start_index..comment_indices[comment_counter]]);
                
                if relevant.is_empty() {return LineInfo::none_str(false, None);}
                else {return LineInfo::new(Some(relevant), false, None);}
            } else {
                relevant.push_str(&line[slice_start_index..line.len()]);
                return LineInfo::with_str(relevant);
            }
        }
    }
}


fn get_bounds_w_multiline_comments(line: &String, extension: &Extension, is_comment_closed: bool,
     open_str_symbol: &Option<String>) -> LineInfo
{
    let com_end_indices = line.match_indices(extension.mutliline_comment_end_symbol.as_ref().unwrap()).map(|x| x.0).collect::<Vec<usize>>();
    let str_indices = get_str_indices(line, extension, open_str_symbol);
    
    if is_comment_closed {
        if open_str_symbol.is_some() && str_indices.is_empty() {
            return LineInfo::none_str(false, open_str_symbol.to_owned());
        } 
    } else {
        if com_end_indices.is_empty() {
            return LineInfo::with_open_comment();
        }
    }
    
    let comment_indices = line.match_indices(&extension.comment_symbol).
    filter_map(|x| {
        if !is_intersecting_with_multi_line_end_symbol(x.0, extension.multiline_len(), &com_end_indices) {
            Some(x.0)
        } else {
            None
        }
    })
    .collect::<Vec<usize>>();
    let com_start_indices = line.match_indices(extension.mutliline_comment_start_symbol.as_ref().unwrap())
    .filter_map(|x|{
        if !is_intersecting_with_comment_symbol(x.0, &comment_indices) {
            Some(x.0)
        } else {
            None
        }
    })
    .collect::<Vec<usize>>();
    
    if str_indices.is_empty() && comment_indices.is_empty() && com_start_indices.is_empty() && com_end_indices.is_empty() {
        return LineInfo::with_str(line.to_owned());
    }
    
    let mut relevant = String::with_capacity(line.len());
    let (mut start_com_counter, mut end_com_counter, mut str_counter, mut comment_counter) = (0,0,0,0); 
    let (mut is_com_open_m, mut is_str_open_m) = (!is_comment_closed, open_str_symbol.is_some());

    //defining utility closures
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

    let mut slice_start_index = 0;
    let mut last_symbol_index = 0;
    loop {
        if is_str_open_m {
            last_symbol_index = str_indices[str_counter];
            let index_after = last_symbol_index + 1;
            if index_after >= line.len() {
                if relevant.is_empty() {return LineInfo::none_all();}
                else {return LineInfo::with_str(relevant);}
            } 
            is_str_open_m = false;
            progress_counters_after(last_symbol_index, &mut comment_counter, &mut str_counter,
                &mut start_com_counter, &mut end_com_counter);
            str_counter += 1;
            slice_start_index = index_after;
        } else if is_com_open_m {
            last_symbol_index = com_end_indices[end_com_counter];
            let index_after = last_symbol_index + extension.multiline_len();
            if index_after >= line.len() {
                if relevant.is_empty() {return LineInfo::none_all();}
                else {return LineInfo::with_str(relevant);}
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
                if relevant.is_empty() {return LineInfo::none_all();}
                else {return LineInfo::with_str(relevant);}
            } else if next_symbol_is_string(comment_counter, str_counter, start_com_counter) {
                let this_index = str_indices[str_counter];
                if skipped_com_end_symbol(last_symbol_index, end_com_counter, this_index) {
                    end_com_counter += 1;
                }
                relevant.push_str(&line[slice_start_index..this_index]);
                str_counter += 1;
                if !has_more_strs(str_counter) {
                    let str_symbol = line.chars().nth(this_index).unwrap().to_string();
                    if relevant.is_empty() {return LineInfo::with_open_symbol(str_symbol);}
                    else {return LineInfo::new(Some(relevant),false, Some(str_symbol));}
                }
                
                is_str_open_m = true;
                last_symbol_index = this_index;
            } else if next_symbol_is_com_start(comment_counter, str_counter, start_com_counter) {
                let this_index = com_start_indices[start_com_counter];
                if skipped_com_end_symbol(last_symbol_index, end_com_counter, this_index) {
                    end_com_counter += 1;
                }
                relevant.push_str(&line[slice_start_index..this_index]);
                if !has_more_ends(end_com_counter) {
                    if relevant.is_empty() {return LineInfo::with_open_comment();}
                    else {return LineInfo::new(Some(relevant),true, None);}
                }
                
                is_com_open_m = true;
                start_com_counter += 1;
                last_symbol_index = this_index;
            } else {
                relevant.push_str(&line[slice_start_index..line.len()]);
                return LineInfo::with_str(relevant);
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use crate::file_parser::get_str_indices;
    use super::*;

    #[test]
    fn get_str_indices_test() {
        let single_str_opt = &Some("'".to_owned());
        let double_str_opt = &Some("\"".to_owned());
        let line = String::from("Hello");
        assert_eq!(Vec::<usize>::new(),get_str_indices(&line, &crate::PYTHON, &None));
        let line = String::from("\"Hello\"");
        assert_eq!(vec![0,6],get_str_indices(&line, &crate::PYTHON, &None));
        let line = String::from("\"'\"Hello");
        assert_eq!(vec![0,2],get_str_indices(&line, &crate::PYTHON, &None));
        assert_eq!(vec![1,2],get_str_indices(&line, &crate::PYTHON, single_str_opt));
        assert_eq!(vec![0,1],get_str_indices(&line, &crate::PYTHON, double_str_opt));
        let line = String::from("''\"\"Hello");
        assert_eq!(vec![0,1,2,3],get_str_indices(&line, &crate::PYTHON, &None));
        assert_eq!(vec![0,1],get_str_indices(&line, &crate::PYTHON, single_str_opt));
        assert_eq!(vec![2,3],get_str_indices(&line, &crate::PYTHON, double_str_opt));
        let line = String::from("'\"'\"''\"He'l\"lo");
        assert_eq!(vec![0,2,3,6,9],get_str_indices(&line, &crate::PYTHON, &None));
        assert_eq!(vec![0,1,3,4,5,6,11],get_str_indices(&line, &crate::PYTHON, single_str_opt));
        assert_eq!(vec![1,2,4,5,9,11],get_str_indices(&line, &crate::PYTHON, double_str_opt));
        assert_eq!(vec![1,3,6,11],get_str_indices(&line, &crate::JAVA, double_str_opt));
    }

    #[test]
    fn get_bounds_PYTHON() {
        let single_str_opt = &Some("'".to_owned());
        let double_str_opt = &Some("\"".to_owned());
        let single_str_li = LineInfo::with_open_symbol("'".to_string());
        let double_str_li = LineInfo::with_open_symbol("\"".to_string());

        let line = String::from("Hello world!");
        assert_eq!(LineInfo::from_slice("Hello world!"),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        assert_eq!(single_str_li,get_bounds_only_single_line_comments(&line, &crate::PYTHON, single_str_opt));
        
        //testing comments
        let line = String::from("#Hello world!");
        assert_eq!(single_str_li,get_bounds_only_single_line_comments(&line, &crate::PYTHON, single_str_opt));
        let line = String::from("Hello world!#");
        assert_eq!(LineInfo::from_slice("Hello world!"),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        let line = String::from("Hello# world!");
        assert_eq!(LineInfo::from_slice("Hello"),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        assert_eq!(single_str_li,get_bounds_only_single_line_comments(&line, &crate::PYTHON, single_str_opt));
        let line = String::from("Hello## world!");
        assert_eq!(LineInfo::from_slice("Hello"),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        let line = String::from("#Hello# world!");
        assert_eq!(single_str_li,get_bounds_only_single_line_comments(&line, &crate::PYTHON, single_str_opt));
        
        //testing strings 
        let line = String::from("\"Hello world!#");
        assert_eq!(double_str_li,get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        let line = String::from("\"Hello\" world!");
        assert_eq!(LineInfo::from_slice(" world!"),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        assert_eq!(LineInfo::new(Some("Hello".to_owned()), false, Some("\"".to_owned())),get_bounds_only_single_line_comments(&line, &crate::PYTHON, double_str_opt));
        let line = String::from("Hello world!\"");
        assert_eq!(LineInfo::new(Some("Hello world!".to_owned()), false, Some("\"".to_owned())),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        let line = String::from("\"'Hello'\" world!");
        assert_eq!(LineInfo::from_slice(" world!"),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        let line = String::from("'Hello' world!");
        assert_eq!(LineInfo::from_slice(" world!"),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        let line = String::from("'\"He'llo'\" world!'");
        assert_eq!(LineInfo::from_slice("llo"),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        assert_eq!(LineInfo::new(Some("He".to_owned()), false, Some("\"".to_owned())),get_bounds_only_single_line_comments(&line, &crate::PYTHON, double_str_opt));
        
        //test mixed
        let line = String::from("'Hello#' world!'");
        assert_eq!(LineInfo::new(Some(" world!".to_owned()), false, Some("'".to_owned())),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        assert_eq!(LineInfo::from_slice("Hello"),get_bounds_only_single_line_comments(&line, &crate::PYTHON, single_str_opt));
        let line = String::from("'Hello'# world!'");
        assert_eq!(LineInfo::none_all(),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        assert_eq!(LineInfo::from_slice("Hello"),get_bounds_only_single_line_comments(&line, &crate::PYTHON, single_str_opt));
        let line = String::from("'''#'''Hello world!'");
        assert_eq!(LineInfo::new(Some("Hello world!".to_owned()), false, Some("'".to_owned())),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        assert_eq!(LineInfo::none_all(),get_bounds_only_single_line_comments(&line, &crate::PYTHON, single_str_opt));
        assert_eq!(LineInfo::with_open_symbol("\"".to_owned()),get_bounds_only_single_line_comments(&line, &crate::PYTHON, double_str_opt));
        let line = String::from("Hello'###'\"world!\"");
        assert_eq!(LineInfo::from_slice("Hello"),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        assert_eq!(LineInfo::none_all(),get_bounds_only_single_line_comments(&line, &crate::PYTHON, single_str_opt));
        assert_eq!(LineInfo::new(Some("world!".to_owned()), false, Some("\"".to_owned())),get_bounds_only_single_line_comments(&line, &crate::PYTHON, double_str_opt));
        let line = String::from("\"//'''\"Hello'\"world!");
        assert_eq!(LineInfo::new(Some("Hello".to_owned()), false, Some("'".to_owned())),get_bounds_only_single_line_comments(&line, &crate::PYTHON, &None));
        assert_eq!(LineInfo::from_slice("world!"),get_bounds_only_single_line_comments(&line, &crate::PYTHON, single_str_opt));
        assert_eq!(LineInfo::new(Some("//".to_owned()), false, Some("\"".to_owned())),get_bounds_only_single_line_comments(&line, &crate::PYTHON, double_str_opt));
    }
    
    #[test]
    fn gets_bounds_JAVA() {
        let single_str_opt = &Some("'".to_owned());
        let double_str_opt = &Some("\"".to_owned());
        let single_str_li = LineInfo::with_open_symbol("'".to_string());
        let double_str_li = LineInfo::with_open_symbol("\"".to_string());

        let line = String::from("Hello world!");
        assert_eq!(LineInfo::with_open_comment(),get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        assert_eq!(LineInfo::with_open_symbol("\"".to_string()),get_bounds_w_multiline_comments(&line, &crate::JAVA, true, double_str_opt));
        assert_eq!(LineInfo::from_slice("Hello world!"),get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        
        //testing only multiline comment combinations
        let line = String::from("*/Hello world!");
        assert_eq!(LineInfo::from_slice("Hello world!"),get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        assert_eq!(LineInfo::from_slice("*/Hello world!"),get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("Hello/* ffd /**//*erer */ world!");
        assert_eq!(LineInfo::from_slice(" world!"),get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        assert_eq!(LineInfo::from_slice("Hello world!"),get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("Hello*//**//**/ world!");
        assert_eq!(LineInfo::from_slice(" world!"),get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        assert_eq!(LineInfo::from_slice("Hello*/ world!"),get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("*//*Hello/**/ world!");
        assert_eq!(LineInfo::from_slice(" world!"),get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        assert_eq!(LineInfo::from_slice("*/ world!"),get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("Hello world*/");
        assert_eq!(LineInfo::none_all(), get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        let line = String::from("*/Hello world!/**/");
        assert_eq!(LineInfo::from_slice("Hello world!"), get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        let line = String::from("Hello world*//**/");
        assert_eq!(LineInfo::none_all(), get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        let line = String::from("*/He/**//*llo world*/!/**/");
        assert_eq!(LineInfo::from_slice("He!"), get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        let line = String::from("Hello world*/!");
        assert_eq!(LineInfo::from_slice("!"), get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        let line = String::from("/*H*/ello world/*!");
        assert_eq!(LineInfo::new(Some("ello world".to_string()), true, None), get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        assert_eq!(LineInfo::new(Some("ello world".to_string()), true, None), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        
        //testing only string symbols
        let line = String::from("\"");
        assert_eq!(LineInfo::with_open_symbol("\"".to_string()), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("\"Hello\"");
        assert_eq!(LineInfo::new(Some("Hello".to_string()), false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, double_str_opt));
        assert_eq!(LineInfo::none_all(), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("\"\"Hello");
        assert_eq!(LineInfo::with_open_symbol("\"".to_string()), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, double_str_opt));
        assert_eq!(LineInfo::from_slice("Hello"), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("\"\"");
        assert_eq!(LineInfo::with_open_symbol("\"".to_string()), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, double_str_opt));
        assert_eq!(LineInfo::none_all(), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("\"\"Hello");
        assert_eq!(LineInfo::from_slice("Hello"), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line  = String::from("Hel\"\"lo");
        assert_eq!(LineInfo::from_slice("Hello"), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("\"\"He\"\"\"ll\"o");
        assert_eq!(LineInfo::from_slice("Heo"), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        
        //testing only comments
        let line = String::from("//");
        assert_eq!(LineInfo::none_all(), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("Hello//");
        assert_eq!(LineInfo::from_slice("Hello"), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        assert_eq!(LineInfo::with_open_comment(), get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        assert_eq!(LineInfo::with_open_symbol("\"".to_string()), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, double_str_opt));
        let line = String::from("//Hello");
        assert_eq!(LineInfo::none_all(), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("////Hello");
        assert_eq!(LineInfo::none_all(), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("He//llo//");
        assert_eq!(LineInfo::from_slice("He"), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        
//         //testing mixed
        let line = String::from("\"\"\"//\"\"\"Hello world!");
        assert_eq!(LineInfo::from_slice("Hello world!"),get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        assert_eq!(LineInfo::none_all(),get_bounds_w_multiline_comments(&line, &crate::JAVA, true, double_str_opt));
        let line = String::from("\"\"one\"//\"\"\"Hello world!");
        assert_eq!(LineInfo::from_slice("oneHello world!"),get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        let line = String::from("\"He\"/*l*/lo//fd");
        assert_eq!(LineInfo::from_slice("lo"), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        assert_eq!(LineInfo::new(Some("He".to_string()), false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, double_str_opt));
        assert_eq!(LineInfo::from_slice("lo"), get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        let line = String::from("//\"/**/dfd\"");
        assert_eq!(LineInfo::none_all(), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        assert_eq!(LineInfo::new(Some("dfd".to_string()), false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        assert_eq!(LineInfo::new(Some("dfd".to_string()), false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, double_str_opt));
        
        let line  = String::from(
            "Hello /* \
            mefm \" */ \" \
            //*/world!"
        );
        assert_eq!(LineInfo::new(Some("Hello  ".to_string()), false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, &None));
        assert_eq!(LineInfo::new(Some(" ".to_string()), false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &crate::JAVA, false, &None));
        assert_eq!(LineInfo::new(Some(" */ ".to_string()), false, Some("\"".to_string())), get_bounds_w_multiline_comments(&line, &crate::JAVA, true, double_str_opt));
    }
}

