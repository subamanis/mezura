use std::{path::Path, process, time::{Instant}};

use colored::*;

use mezura::{config_manager::{self}, Configuration, io_handler, putils::*};

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

    let languages_map = match io_handler::parse_supported_languages_to_map(&mut config.languages_of_interest) {
        Err(x) => {
            println!("\n{}", x.formatted());
            utils::wait_for_input();
            process::exit(1);
        },
        Ok(x) => {
            if !x.1.is_empty() {
                let mut warn_msg = String::from("\nFormatting problems detected in language files: ");

                warn_msg.push_str(&x.1.join(", "));
                warn_msg.push_str(". These files will not be taken into consideration.");
                println!("{}",warn_msg.yellow());
            }
            
            if !x.2.is_empty() {
                let relevant = x.2.iter().filter_map(|s| if !x.1.contains(&(s.to_owned()+".txt")){Some(s.to_owned())} else {None}).collect::<Vec<_>>();
                if !relevant.is_empty() {
                    let warn_msg = format!("\nThese languages don't exist as language files: {}",relevant.join(", "));
                    println!("{}",warn_msg.yellow());
                }
            }

            x.0
        }
    };

    // println!("{:#?}", languages_map);

    let instant = Instant::now();
    match mezura::run(config, languages_map) {
        Ok(x) => {
            let perf = format!("\nExecution time: {:.2} secs ", instant.elapsed().as_secs_f32());
            let metrics = match x {
                Some(x) => format!("(Parsing: {} files/s | {} lines/s)", with_seperators(x.files_per_sec), with_seperators(x.lines_per_sec)),
                None => String::new()
            };
            println!("{}",perf + &metrics);
        },
        Err(x) => println!("{}",x.formatted())
    }

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
        if !Path::new(&(data_dir + "/languages")).is_dir() {
            return Err("'languages' directory not found inside 'data'.".red().to_string())
        } 
        
        Ok(())
    } else {
        Err("'data' directory not found in any known location.".red().to_string())
    }
}