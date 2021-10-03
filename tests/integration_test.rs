// use std::collections::{HashMap, LinkedList};
// use std::sync::atomic::AtomicBool;
// use std::sync::{Arc, Mutex};
// use mezura::*;

// #[test]
// fn test_whole_workflow () {
//     let current_dir = env!("CARGO_MANIFEST_DIR").replace("\\", "/");
//     let mut config = config_manager::create_config_from_args(&format!("{}/src,{}/tests --threads 3 ",current_dir, current_dir)).unwrap();
//     let language_map = io_handler::parse_supported_languages_to_map(&mut config.languages_of_interest).unwrap().language_map;
//     let language_map_len = language_map.len(); 

//     assert_eq!(3, config.threads);
//     assert_eq!(vec![format!("{}/src",current_dir), format!("{}/tests", current_dir)], config.dirs);
//     assert!(language_map.len() != 0);

//     let files_ref : LinkedListRef = Arc::new(Mutex::new(LinkedList::new()));
//     let faulty_files_ref : FaultyFilesRef  = Arc::new(Mutex::new(Vec::new()));
//     let finish_condition_ref : BoolRef = Arc::new(AtomicBool::new(false));
//     let language_map_ref : LanguageMapRef = Arc::new(language_map);
//     let languages_content_info_ref = Arc::new(Mutex::new(make_language_stats(language_map_ref.clone())));
//     let mut languages_metadata = make_language_metadata(language_map_ref.clone());

//     assert!(languages_metadata.len() == language_map_len);

//     let (total_files_num, relevant_files_num) = producer::add_relevant_files(
//         files_ref.clone(), &mut languages_metadata, finish_condition_ref.clone(), &language_map_ref, &config);

//     consumer::start_parsing_files(files_ref, faulty_files_ref.clone(), finish_condition_ref, languages_content_info_ref.clone(),
//          language_map_ref.clone(), &config);
    
//     let mut content_info_map_guard = languages_content_info_ref.lock();
//     let content_info_map = content_info_map_guard.as_deref_mut().unwrap();

//     remove_languages_with_0_files(content_info_map, &mut languages_metadata);
    
//     assert!(relevant_files_num != 0 && total_files_num != 0);
//     let first_lang_metadata = languages_metadata.iter().next().unwrap().1;
//     assert!(first_lang_metadata.files != 0 && first_lang_metadata.bytes != 0);
//     assert!(faulty_files_ref.clone().lock().unwrap().len() == 0);

//     let mut keyword_num = 0;
//     for content_info in content_info_map.iter() {
//         content_info.1.keyword_occurences.iter().for_each(|x| keyword_num += x.1);
//     } 
//     assert!(keyword_num != 0);
// }

// fn remove_languages_with_0_files(content_info_map: &mut HashMap<String,LanguageContentInfo>,
//     languages_metadata_map: &mut HashMap<String, LanguageMetadata>) 
// {
//    let mut empty_languages = Vec::new();
//    for element in languages_metadata_map.iter() {
//        if element.1.files == 0 {
//            empty_languages.push(element.0.to_owned());
//        }
//    }

//    for ext in empty_languages {
//        languages_metadata_map.remove(&ext);
//        content_info_map.remove(&ext);
//    }
// }
