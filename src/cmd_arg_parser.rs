#[derive(Debug)]
pub struct ProgramArguments {
    pub path : String,
    pub exclude_dirs : Option<Vec<String>>
}

impl ProgramArguments {
    pub fn new(mut list : Vec<String>) -> Result<ProgramArguments,()> {
        if list.len() == 0 {
            Err(())
        }
        else {
            if list.len() == 1 {
                Ok(ProgramArguments {path : list.remove(0), exclude_dirs : None})
            } else {
                Ok(ProgramArguments {path : list.remove(0), exclude_dirs : Some(list)})
            }
        }
    }
}

impl PartialEq for ProgramArguments {
    fn eq(&self, other :&Self) -> bool {
        self.path == other.path && self.exclude_dirs == other.exclude_dirs
    }
}

pub fn read_args_cmd() -> Result<ProgramArguments,()> {
    let args  = std::env::args().skip(1).collect::<Vec<String>>();
    if args.is_empty() {return Err(());}
    let args = get_distinct_arguments(args.join(" "));

    ProgramArguments::new(args)
}

pub fn read_args_console() -> Result<ProgramArguments,()> {
    let mut line = String::with_capacity(30);
    std::io::stdin().read_line(&mut line).unwrap();
    if line.trim().is_empty() {
        Err(())
    } else {
        let args = get_distinct_arguments(line);
        if args.is_empty() {Err(())} 
        else {ProgramArguments::new(args)}
    }
}

fn get_distinct_arguments(line: String) -> Vec<String> {
    if let Some(dirs_pos) = line.find("--dirs") {
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
    fn creating_program_arguments() {
        assert_eq!(Err(()), ProgramArguments::new(vec![]));
        assert_eq!(program_args(vec!("path")), ProgramArguments::new(vec!["path".to_string()]));
        assert_eq!(program_args(vec!("path","exclude1","exclude2")), ProgramArguments::new(vec!["path".to_string(),"exclude1".to_string(),"exclude2".to_string()]));
    }

    fn program_args(mut vec : Vec<&str>) -> Result<ProgramArguments,()> {
        if vec.len() == 1 {
            Ok(ProgramArguments {path : vec.remove(0).to_string(), exclude_dirs : None})
        } else {
            Ok(ProgramArguments {path : vec.remove(0).to_string(), exclude_dirs : Some(vec.iter().map(|x| x.to_string()).collect::<Vec<String>>())})
        }
    }
}
