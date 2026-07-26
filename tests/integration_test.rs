use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use crossbeam_deque::{Injector, Worker};
use mezura::*;
use mezura::config_manager::Threads;

#[test]
fn test_whole_workflow () {
    let current_dir = env!("CARGO_MANIFEST_DIR").replace("\\", "/");
    let config = config_manager::create_config_from_args(&format!("{current_dir}/src,{current_dir}/tests --threads 1 3 ")).unwrap();
    let language_map = io_handler::parse_supported_languages_to_map(&LOCAL_APP_PATHS.languages_dir).unwrap().0;
    let language_map_len = language_map.len(); 

    assert_eq!(Threads::new(1,3), config.threads);
    assert_eq!(vec![format!("{current_dir}/src"), format!("{}/tests", current_dir)], config.dirs);
    assert!(!language_map.is_empty());

    let config = Arc::new(config);
    let mut files_present = FilesPresent::default();
    let faulty_files_ref : FaultyFilesListMut  = Arc::new(Mutex::new(Vec::new()));
    let finish_condition_ref = Arc::new(AtomicBool::new(false));
    let language_map = Arc::new(language_map);
    let languages_content_info_ref = Arc::new(Mutex::new(make_language_stats(language_map.clone())));
    let files_injector = Arc::new(Injector::new());
    let dirs_injector = Arc::new(Injector::new());
    let idle_producers = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let languages_metadata_map = Arc::new(Mutex::new(make_language_metadata(&language_map)));

    assert!(languages_metadata_map.lock().unwrap().len() == language_map_len);

    let extension_lang_map: ExtensionLangMap = Arc::new(make_extension_language_map(&language_map));
    calculate_single_file_stats_or_add_to_injector(&config, &dirs_injector, &files_injector, &mut files_present, &extension_lang_map, &languages_metadata_map);

    let (total_files_num, relevant_files_num, _) = producer::search_for_files(0, files_injector.clone(), dirs_injector.clone(),
         Worker::new_fifo(), idle_producers, extension_lang_map, languages_metadata_map.clone(), config.clone());

    finish_condition_ref.store(true, Ordering::Relaxed);
    consumer::start_parsing_files(0, files_injector, faulty_files_ref.clone(), finish_condition_ref, languages_content_info_ref.clone(),
         language_map.clone(), config);
    
    let mut content_info_map_guard = languages_content_info_ref.lock();
    let content_info_map = content_info_map_guard.as_deref_mut().unwrap();

    let mut languages_metadata_map_guard = languages_metadata_map.lock();
    let languages_metadata_map = languages_metadata_map_guard.as_deref_mut().unwrap();

    remove_languages_with_0_files(content_info_map, languages_metadata_map);
    
    assert!(relevant_files_num != 0 && total_files_num != 0);
    let first_lang_metadata = languages_metadata_map.iter().next().unwrap().1;
    assert!(first_lang_metadata.files != 0 && first_lang_metadata.bytes != 0);
    assert!(faulty_files_ref.clone().lock().unwrap().is_empty());

    let mut keyword_num = 0;
    for content_info in content_info_map.iter() {
        content_info.1.keyword_occurences.iter().for_each(|x| keyword_num += x.1);
    } 
    assert!(keyword_num != 0);
}

