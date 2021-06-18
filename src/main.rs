use std::{path::Path, process, time::SystemTime};

use colored::*;

use code_stats::{config_manager::{self}, Configuration, io_handler, putils::*};

fn main() {
    control::set_virtual_terminal(true).unwrap();

    if let Err(x) = verify_required_dirs() {
        println!("{}",x);
        utils::wait_for_input();
        process::exit(1);
    }

    let mut config = match config_manager::read_args_cmd() {
        Ok(config) => config,
        Err(x) => {
            println!("\n{}",x.formatted());
            get_args_from_stdin()
        } 
    };

    let extensions_map = match io_handler::parse_supported_extensions_to_map(&mut config.extensions_of_interest) {
        Err(x) => {
            println!("\n{}", x.formatted());
            utils::wait_for_input();
            process::exit(1);
        },
        Ok(x) => {
            if !x.1.is_empty() {
                let mut warn_msg = String::from("\nFormatting problems detected in extension files: ");
                warn_msg.push_str(&utils::get_contents(&x.1));
                warn_msg.push_str(". These files will not be taken into consideration.");
                println!("{}",warn_msg.yellow());
            }
            
            if !x.2.is_empty() {
                let relevant = x.2.iter().filter_map(|s| if !x.1.contains(&(s.to_owned()+".txt")){Some(s.to_owned())} else {None}).collect::<Vec<_>>();
                if !relevant.is_empty() {
                    let warn_msg = format!("\nThese extensions don't exist as extension files: {}",relevant.join(", "));
                    println!("{}",warn_msg.yellow());
                }
            }

            x.0
        }
    };

    let start = SystemTime::now();
    if let Err(x) = code_stats::run(config, extensions_map) {
        println!("{}",x.formatted());
    }
    println!("\nExecution time: {:.2} secs.",SystemTime::now().duration_since(start).unwrap().as_secs_f32());

    utils::wait_for_input();
}

fn get_args_from_stdin() -> Configuration {
    loop {
        println!("\nPlease provide a file name or a root directory path, and optional parameters.\nType --help for the parameter list ");
        match config_manager::read_args_console() {
            Err(e) => println!("{}",e.formatted()),
            Ok(config) => break config
        }
    }
}

fn verify_required_dirs() -> Result<(),String> {
    if let Some(data_dir) = io_handler::DATA_DIR.clone() {
        if !Path::new(&(data_dir + "/extensions")).is_dir() {
            return Err("'extensions' directory not found inside 'data'.".red().to_string())
        } 
        
        Ok(())
    } else {
        Err("'data' directory not found in any known location.".red().to_string())
    }
}