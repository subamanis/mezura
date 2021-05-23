use std::{path::Path, process};

use colored::*;

use code_stats::{cmd_arg_parser, extension_reader, utils};

fn main() {
    control::set_virtual_terminal(true).unwrap();

    let extensions_map = match extension_reader::parse_supported_extensions_to_map() {
        Err(x) => {
            println!("{}", x.formatted());
            utils::wait_for_input();
            process::exit(1);
        },
        Ok(x) => {
            if !x.1.is_empty(){
                let mut err_msg : String = String::from("\nFormatting problems detected in extension files: ");
                err_msg.push_str(&utils::get_contents(&x.1));
                err_msg.push_str(". These files will not be taken into consideration.\n");
                println!("{}",err_msg.yellow());
            }

            x.0
        }
    };

    let args = cmd_arg_parser::read_args_cmd().unwrap_or_else(|_| {
        loop {
            println!("\nPlease provide a file name or a root directory path, and optional exclude directories\n(e.g. C:\\users\\user\\Desktop\\project --dirs exclude_dir1 exclude_dir2)",);
            if let Ok(x) = cmd_arg_parser::read_args_console() {
                let path = Path::new(&x.path);
                if path.is_dir() || path.is_file(){
                    break x
                } else {
                    println!("{}","\nPath provided is not a valid directory or file.".red());
                }
            } else {
                println!("{}","No arguments provided.".red());
            }
        }
    });

    if let Err(x) = code_stats::run(args, extensions_map) {
        println!("{}",x.formatted());
    }

    utils::wait_for_input();
}



//  [Extension]            [Files]                      [Lines]                       [Size]
//  ________________________________________________________________________________________________________________________________
//     java          [-||||||.....-58%]  47       [-||||||||...-78%]  494      [-|||||......-58%]  47 
//       cs          [-|||||||||..-74%]  85       [-|||........- 9%]  63       [-||||.......-58%]  47
//       py          [-|||........- 9%]  11       [-|||........- 9%]  51       [-|||||||||..-74%]  85