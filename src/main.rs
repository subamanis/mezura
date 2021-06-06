use std::{path::Path, process, time::SystemTime};

use colored::*;

use code_stats::{cmd_arg_parser::{self}, Args, extension_reader, putils::*};

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

    let args = match cmd_arg_parser::read_args_cmd() {
        Ok(args) => args,
        Err(_) => get_args_from_stdin()
    };

    let start = SystemTime::now();

    if let Err(x) = code_stats::run(args, extensions_map) {
        println!("{}",x.formatted());
    }

    println!("\nExecution time: {:.2} secs.",SystemTime::now().duration_since(start).unwrap().as_secs_f32());

    utils::wait_for_input();
}

fn get_args_from_stdin() -> Args {
    loop {
        println!("\nPlease provide a file name or a root directory path, and optional parameters.\nType --help for the parameter list ");
        match cmd_arg_parser::read_args_console() {
            Err(e) => e.print_self(),
            Ok(args) => {
                let path = Path::new(&args.path);
                if path.is_dir() || path.is_file(){
                    break args;
                } else {
                    println!("{}","\nPath provided is not a valid directory or file.".red());
                }
            }
        }
    }
}