use std::{path::Path, process, time::SystemTime};

use colored::*;

use code_stats::{config_manager::{self}, Configuration, data_reader, putils::*};

fn main() {
    control::set_virtual_terminal(true).unwrap();

    if let Err(x) = verify_required_dirs() {
        println!("{}",x);
        utils::wait_for_input();
        process::exit(1);
    }

    let config = match config_manager::read_args_cmd() {
        Ok(config) => config,
        Err(x) => {
            println!("\n{}",x.formatted());
            get_args_from_stdin()
        } 
    };

    let extensions_map = match data_reader::parse_supported_extensions_to_map(&config.extensions_of_interest) {
        Err(x) => {
            println!("\n{}", x.formatted());
            utils::wait_for_input();
            process::exit(1);
        },
        Ok(x) => {
            if !x.1.is_empty(){
                let mut err_msg : String = String::from("\nFormatting problems detected in extension files: ");
                err_msg.push_str(&utils::get_contents(&x.1));
                err_msg.push_str(". These files will not be taken into consideration.");
                println!("{}",err_msg.yellow());
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
    if let Some(data_dir) = data_reader::DATA_DIR.clone() {
        if !Path::new(&(data_dir + "/extensions")).is_dir() {
            return Err("'extensions' directory not found inside 'data'.".red().to_string())
        } 
        
        Ok(())
    } else {
        return Err("'data' directory not found in any known location.".red().to_string())
    }
}