use std::{sync::{Arc, Mutex}, thread::JoinHandle, time::Duration};
use std::thread;

pub fn main() {
    let files : Vec<String> = vec![];
    // let files : Vec<String> = vec!["ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),
    // "ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),
    // "ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned(),"ena".to_owned()];
    let files_ref = Arc::new(Mutex::new(files));
    let condition = false;
    let condition_ref = Arc::new(Mutex::new(condition));

    let mut handles = vec![];

    for i in 0..7 {
        handles.push(make_consumer(i, Arc::clone(&files_ref), Arc::clone(&condition_ref)).unwrap());
    }

    produce(&files_ref, &condition_ref);

    for handle in handles {
        handle.join().unwrap();
    }

    println!("Result: {:?}", *files_ref.lock().unwrap());
}

fn produce(files_ref: &Mutex<Vec<String>>, condition_ref: &Mutex<bool>) {
    for i in 0..60 {
        thread::sleep(Duration::from_millis(1));
        files_ref.lock().unwrap().push(String::from("Petros"));
        println!("{}) Produce",i);
    }
    *condition_ref.lock().unwrap() = true;
} 

fn make_consumer(id: i32, files_ref: Arc<Mutex<Vec<String>>>, condition_ref : Arc<Mutex<bool>>) -> Result<JoinHandle<()>, std::io::Error> {
    return thread::Builder::new().name(id.to_string()).spawn(move || {
        loop {
            let mut files = files_ref.lock().unwrap();
            if files.is_empty() {
                if *condition_ref.lock().unwrap() {
                    break;
                } else {
                    drop(files);
                    thread::sleep(Duration::from_millis(2));
                    continue;
                }
            }

            parse_file(files.remove(0));
            println!("Thread {} consume",id);
        }
    });
}

fn parse_file(file_name: String) -> usize {
    thread::sleep(Duration::from_millis(6));
    
    10
}