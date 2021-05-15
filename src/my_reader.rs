use std::{fs::File, io::{self, prelude::*}};

pub struct BufReader {
    reader: io::BufReader<File>,
}

impl BufReader {
    pub fn open(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
        let file = File::open(path)?;
        let reader = io::BufReader::new(file);

        Ok(Self { reader })
    }

    pub fn read_line_exists(&mut self, buffer: &mut String) -> bool {
        match self.read_line(buffer) {
            Err(_) => false,
            Ok(x) => {
                x != 0 
            }
        }
    }

    pub fn read_line_and_compare(&mut self, buffer: &mut String, other : &str) -> bool {
        match self.read_line(buffer) {
            Ok(_) => {
                buffer.trim_end() == other
            },
            Err(_) => false
        }
    }

    pub fn read_line(&mut self, buffer: &mut String) -> Result<usize, io::Error> {
        buffer.clear();
        self.reader.read_line(buffer)
    }

    pub fn read_lines_exist(&mut self, num :usize, buffer: &mut String) -> bool {
        for _ in 0..num {
            if !self.read_line_exists(buffer) {return false;}
        }
        
        true
    }
    
    pub fn read_lines(&mut self, num :usize, buffer: &mut String) -> Result<(),std::io::Error> {
        for _ in 0..num {
            if let Err(x) = self.read_line(buffer) {return Err(x);}
        }
        
        Ok(())
    }

    pub fn get_line_sliced(&mut self, buffer: &mut String) -> Result<Vec<String>, ()> {
        if self.read_line_exists(buffer) {
            let buffer = buffer.trim_end();
            let mut vec = buffer.split_whitespace().map(|s| s.to_string()).collect::<Vec<String>>();
            if vec.len() == 0 {return Ok(vec![String::new()]);}
            let last_index = vec.len()-1;
            vec[last_index] = vec[last_index].trim_end().to_owned();
            Ok(vec) 
        } else {
            Err(())
        }
    }
}


// #[cfg(test)]
// mod tests {
//     fn read_a
// }