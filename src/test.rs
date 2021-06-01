// use std::{fs::File, future::Future, task::Poll};

// // use walkdir::*;

fn main() {
    // async_main();
}

// async fn async_main() {
//     let path = "dfd";
    
//     let result = do_work().await;
//     let f = File::open(path).unwrap();

// }

// async fn do_work() -> impl Future {
//     //do some stuff

// }

// struct AnEnum {
//     lines : usize
// }

// impl Future for AnEnum {
//     type Output = usize;

//     fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
//         Poll::Ready(1242)
//     }
// }

// // async fn open(path: &str) -> File {
// //     File::open(path).unwrap().await
// // }

// // pub fn test_fn() {
// //     for entry in WalkDir::new("C:\\Users\\petro\\Documents\\Unity Projects") {
// //         if let Ok(dir) = entry {
// //             dir.
// //         } else {
// //             continue;
// //         }
// //     }

// // }

// // fn is_relevant(entry: &DirEntry) {
// //     let a = entry.file_name().to_str().unwrap();
// //     if a.
// // }