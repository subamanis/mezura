use colored::Colorize;

#[derive(Debug,PartialEq)]
pub struct ProgramArgs {
    pub path: String,
    pub exclude_dirs: Option<Vec<String>>,
    pub dirs_of_interest: Option<Vec<String>>,
    pub extensions_of_interest: Option<Vec<String>>,
    pub threads: Option<usize>
}

impl ProgramArgs {
    pub fn new(path: String, exclude_dirs: Option<Vec<String>>, dirs_of_interest: Option<Vec<String>>,
        extensions_of_interest: Option<Vec<String>>, threads: Option<usize>) -> ProgramArgs 
    {
        ProgramArgs {path, exclude_dirs, dirs_of_interest, extensions_of_interest, threads}
    }
}

#[derive(Debug)]
pub enum ArgParsingError {
    NoArgsProvided,
    UnrecognisedParameter(String)
}

impl ArgParsingError {
    pub fn print_self(&self) {
        match self {
            Self::NoArgsProvided => println!("{}",format!("No arguments provided.").red()),
            Self::UnrecognisedParameter(p) => println!("{}", format!("--{} is not recognised as a command.",p).red())
        }
    }
}

pub fn read_args_cmd() -> Result<ProgramArgs,ArgParsingError> {
    let args  = std::env::args().skip(1).collect::<Vec<String>>();
    if args.is_empty() {return Err(ArgParsingError::NoArgsProvided)}
    let line = args.join(" ");
    let line = line.trim();
    if line.is_empty() {return Err(ArgParsingError::NoArgsProvided)}

    new_get_arguments(line)
}

pub fn read_args_console() -> Result<ProgramArgs,ArgParsingError> {
    let mut line = String::with_capacity(30);
    std::io::stdin().read_line(&mut line).unwrap();
    if line.trim().is_empty() {
        Err(ArgParsingError::NoArgsProvided)
    } else {
        new_get_arguments(&line)
    }
}


fn new_get_arguments(line: &str) -> Result<ProgramArgs, ArgParsingError> {
    fn get_if_not_empty(str: &str) -> Option<String> {
        if str.is_empty() {None}
        else {Some(str.to_owned())}
    }

    let options = line.split("--").collect::<Vec<_>>();
    let path = options[0].trim().to_owned();

    let (mut exclude_dirs, mut dirs_of_interest, mut extensions_of_interest, mut threads) = 
        (None, None, None, None);
    for i in 1..options.len() {
        if options[i].starts_with("exclude") {
            exclude_dirs = Some(options[i].split(" ").into_iter().skip(1)
             .filter_map(|e| get_if_not_empty(e))
             .collect::<Vec<_>>());
        } else if options[i].starts_with("dirs"){
            dirs_of_interest = Some(options[i].split(" ").into_iter().skip(1)
            .filter_map(|e| get_if_not_empty(e))
            .collect::<Vec<_>>());
        } else if options[i].starts_with("extensions"){
            extensions_of_interest = Some(options[i].split(" ").into_iter().skip(1)
            .filter_map(|e| get_if_not_empty(e))
            .collect::<Vec<_>>());
        } else if options[i].starts_with("threads") {
            if options[i].len() > 1 {
                let threads_str = options[i].split(" ").skip(1).next().unwrap().trim(); 
                if let Ok(x) = threads_str.parse::<usize>() {
                    threads = Some(x);
                }
            }
        } else {
            //@TODO: -help?
            return Err(ArgParsingError::UnrecognisedParameter(options[i].split(" ").next().unwrap_or(options[i]).trim().to_owned()));
        }
    }
    
    Ok(ProgramArgs::new(path, exclude_dirs, dirs_of_interest, extensions_of_interest, threads))
}


fn get_distinct_arguments(line: String) -> Vec<String> {
    if let Some(dirs_pos) = line.find("--exclude") {
        let parts = line.split_at(dirs_pos);
        let mut args = vec![parts.0.trim().to_owned()];
        for dir in parts.1.split_whitespace() {
            if dir != "--dirs"{
                args.push(dir.to_owned());
            }
        }
        args
    } else {
        vec![line.trim().to_owned()]
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_arg_parsing() {
        //@TODO: trim() and check for empty line
        assert_eq!(ProgramArgs::new("path".to_owned(),None,None,None,None),new_get_arguments("path").unwrap());
        assert_eq!(ProgramArgs::new("path path1 path2".to_owned(),None,None,None,None),new_get_arguments("path path1 path2").unwrap());
        assert!(new_get_arguments("path --something").is_err());
        assert!(new_get_arguments("path --threads 3 --dirs eh --something --exclude e").is_err());
        assert_eq!(ProgramArgs::new("path".to_owned(),Some(vec!["ex1".to_owned(),"ex2".to_owned()]),Some(vec!["dir1".to_owned(),"dir2".to_owned()]),
            Some(vec!["java".to_owned(),"cs".to_owned()]), Some(4)), new_get_arguments("path --threads 4 --exclude ex1 ex2 --dirs dir1 dir2 --extensions java cs").unwrap());
        assert_eq!(ProgramArgs::new("path".to_owned(),Some(vec!["ex2".to_owned()]), None, Some(vec!["java".to_owned(),"cs".to_owned()]), None),
            new_get_arguments("path   --exclude ex2  --extensions java    cs").unwrap());
    }
}

