use std::collections::HashMap;

use mezura_core::{CountingModel, EngineConfig, Languages, Target, Threads, language_file, languages, run};

const LANGUAGES_DIR : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/languages/");

// Two directories, one producer and three consumers, through the published surface and nothing else.
// What a command line would have parsed into an EngineConfig is asserted where the parsing lives.
// The languages come from a directory here, which is the door for a caller with files of its own;
// every test below it takes the shipped ones, which is the door for everybody else.
#[test]
fn a_run_over_two_directories_counts_files_lines_and_keywords() {
    let current_dir = env!("CARGO_MANIFEST_DIR").replace("\\", "/");
    let config = EngineConfig {
        threads: Threads::new(1, 3),
        ..EngineConfig::new([format!("{current_dir}/src"), format!("{current_dir}/tests")])
    };

    assert_eq!(Threads::new(1, 3), config.threads);
    assert_eq!(vec![Target::of(format!("{current_dir}/src")), Target::of(format!("{current_dir}/tests"))], config.targets);

    let languages_on_disk = language_file::parse_languages_in_dir(LANGUAGES_DIR).unwrap().0;
    assert!(!languages_on_disk.is_empty());

    let (languages, _) = Languages::resolve(&config, languages_on_disk, &Default::default());
    let result = run(&config, languages, None, |_| {}).unwrap();

    assert!(result.files_present.total_files != 0 && result.files_present.relevant_files != 0);
    assert!(result.faulty_files.is_empty());

    let first_language = result.per_language.values().next().unwrap();
    assert!(first_language.files != 0 && first_language.bytes != 0);

    // Readable from outside at all, which they were not before the surface was chosen. Against the
    // classes and not against the three columns: 'extra' is what is left of the lines after the
    // other two, so the columns can only add up to the lines they were worked out from, whatever
    // the walk did. The classes are the second measurement and can disagree.
    assert_eq!(result.total.lines, result.total.classes.calculate_lines());
    let model = CountingModel::Content;
    assert_eq!(result.total.lines, result.total.calculate_code_lines(model)
            + result.total.calculate_comment_lines(model) + result.total.calculate_extra_lines(model));

    let keyword_num = result.per_language.values()
            .flat_map(|info| info.keyword_occurences.values()).copied().sum::<usize>();
    assert!(keyword_num != 0);
}

// The copies baked into the library are the files of 'data/languages' and not a set that drifted
// away from them, which is the whole promise of 'Languages::shipped': a caller that never touches
// the disk counts with what a caller reading that directory counts with.
#[test]
fn the_shipped_languages_are_the_files_of_the_languages_directory() {
    let mut from_disk = language_file::parse_languages_in_dir(LANGUAGES_DIR).unwrap().0;
    let mut baked_in = languages::parse_shipped_languages();
    assert!(!baked_in.is_empty());

    from_disk.sort_by(|a, b| a.name.cmp(&b.name));
    baked_in.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(from_disk, baked_in);

    // and the rule that settles a contested extension travels with them
    assert!(!languages::parse_shipped_extension_priority().by_extension.is_empty());
}

// A language a caller builds by hand is counted under the name it carries. It used to be counted
// under whichever key the caller happened to file it under, so a name typed twice in two shapes
// left the report calling the language something its definition never said.
#[test]
fn a_language_of_my_own_is_counted_under_the_name_it_carries() {
    let root = std::env::temp_dir().join("mezura-own-language");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.pal"), "// a comment\nlet x = 1;\n").unwrap();

    let config = EngineConfig {
        threads: Threads::new(1, 1),
        ..EngineConfig::new([root.to_string_lossy().replace('\\', "/")])
    };
    let mine = mezura_core::Language::new("PetrosLang", ["pal"], ["\""], ["//"], &[],
            [mezura_core::Keyword::new("bindings", ["let"])]);

    let (languages, _) = Languages::resolve(&config, [mine], &Default::default());
    let result = run(&config, languages, None, |_| {}).unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(vec!["PetrosLang"], result.per_language.keys().collect::<Vec<_>>());
    assert_eq!(2, result.total.lines);
    assert_eq!(Some(&1), result.per_language["PetrosLang"].keyword_occurences.get("bindings"));
}

// A container file weighs whole on its own language's row, and the sections inside it come back
// decomposed in 'embedded', never added to any row: that is the whole counting model, asserted
// through the public surface. The second run narrows itself to the container language alone, which
// is the case the machinery keeps definitions aside for: the sections must still resolve.
#[test]
fn a_container_file_is_one_file_of_its_language_and_its_sections_are_the_decomposition() {
    let root = std::env::temp_dir().join("mezura-embedded-sections");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("page.web"), "<p>hello</p>\n<script>\n// a js comment\nvar x = 1;\n</script>\n").unwrap();
    std::fs::write(root.join("plain.js"), "// a comment\nvar y = 2;\n").unwrap();

    let web = mezura_core::Language::new("Web", ["web"], [""; 0], [""; 0], &[("<!--", "-->")], [])
            .with_nested_languages(&[mezura_core::NestedLanguage::of("<script", "</script>", "js")]);
    let js = mezura_core::Language::new("JS", ["js"], ["\""], ["//"], &[("/*", "*/")], []);
    let definitions = || [web.clone(), js.clone()];

    let config = EngineConfig {
        threads: Threads::new(1, 1),
        ..EngineConfig::new([root.to_string_lossy().replace('\\', "/")])
    };
    let (languages, _) = Languages::resolve(&config, definitions(), &Default::default());
    let result = run(&config, languages, None, |_| {}).unwrap();

    // the rows: the container holds all five of its lines, the plain js file its two, nothing twice
    assert_eq!((1, 5), (result.per_language["Web"].files, result.per_language["Web"].lines));
    assert_eq!((1, 2), (result.per_language["JS"].files, result.per_language["JS"].lines));
    assert_eq!(7, result.total.lines);

    // the decomposition: two js lines inside one container file, with their own reading
    let section = &result.nested_languages["Web"]["JS"];
    assert_eq!((1, 2, 1, 1), (section.files, section.lines,
            section.calculate_code_lines(CountingModel::Content),
            section.calculate_comment_lines(CountingModel::Content)));
    assert!(section.bytes > 0 && section.bytes < 30);

    // narrowed to the container language alone, the sections still resolve and still decompose
    let narrowed = EngineConfig { languages_of_interest: vec!["Web".to_owned()], ..config };
    let (languages, _) = Languages::resolve(&narrowed, definitions(), &Default::default());
    let result = run(&narrowed, languages, None, |_| {}).unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(vec!["Web"], result.per_language.keys().collect::<Vec<_>>());
    assert_eq!(2, result.nested_languages["Web"]["JS"].lines,
            "a narrowed run lost the decomposition of its own container files");
}

// A region that held nothing and one written in the container's own language fold into the file's
// own share: zero lines say nothing, and a container is not a breakdown of itself. An excluded
// language stays named, decided on 2026-08-13: excluding means its files are not counted, while the
// breakdown of a counted file is a fact about it, and folding it away would move those lines into
// the container's own share, which would be a lie about what "itself" holds.
#[test]
fn an_empty_or_self_section_gets_no_row_and_an_excluded_one_keeps_its_name() {
    let root = std::env::temp_dir().join("mezura-suppressed-sections");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("page.web"), "<script>\nvar x = 1;\nvar y = 2;\n</script>\n<style>\np {}\n</style>\n").unwrap();
    std::fs::write(root.join("empty.web"), "<script src=\"x.js\">\n</script>\n").unwrap();
    std::fs::write(root.join("inner.web"), "<template>\n<p>hi</p>\n</template>\n").unwrap();

    let web = mezura_core::Language::new("Web", ["web"], [""; 0], [""; 0], &[("<!--", "-->")], [])
            .with_nested_languages(&[mezura_core::NestedLanguage::of("<script", "</script>", "js"),
                    mezura_core::NestedLanguage::of("<style", "</style>", "css"),
                    mezura_core::NestedLanguage::of("<template", "</template>", "web")]);
    let js = mezura_core::Language::new("JS", ["js"], ["\""], ["//"], &[("/*", "*/")], []);
    let css = mezura_core::Language::new("CSS", ["css"], [""; 0], [""; 0], &[("/*", "*/")], []);
    let definitions = || [web.clone(), js.clone(), css.clone()];

    let config = EngineConfig { threads: Threads::new(1, 1),
            ..EngineConfig::new([root.to_string_lossy().replace('\\', "/")]) };
    let (languages, _) = Languages::resolve(&config, definitions(), &Default::default());
    let result = run(&config, languages, None, |_| {}).unwrap();

    let sections = &result.nested_languages["Web"];
    assert_eq!((1, 2), (sections["JS"].files, sections["JS"].lines), "the empty region counted as a JS file");
    assert_eq!(1, sections["CSS"].files);
    assert!(!sections.contains_key("Web"), "the container was named as its own breakdown");

    // Excluded by extension spelling, so the exclusion resolves the way '--exclude-languages js'
    // does, and neither the breakdown nor the container's own row moves by a line
    let web_lines = result.per_language["Web"].lines;
    let excluded = EngineConfig { excluded_languages: vec!["js".to_owned()], ..config };
    let (languages, _) = Languages::resolve(&excluded, definitions(), &Default::default());
    let result = run(&excluded, languages, None, |_| {}).unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(2, result.nested_languages["Web"]["JS"].lines,
            "excluding a language hid it from the breakdown of a counted file");
    assert_eq!(web_lines, result.per_language["Web"].lines);
}

// 'Languages::shipped' is the door that also applies the rule for an extension two languages both
// claim, and nothing was proving it did: every other test counts trees of '.rs' where nothing is
// contested, so replacing that rule with an empty one passed the whole suite. '.m' is claimed by
// both Objective-C and MATLAB, and the file that ships names the winner.
#[test]
fn the_shipped_rule_for_a_contested_extension_is_actually_applied() {
    let root = std::env::temp_dir().join("mezura-contested-extension");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.m"), "// one line of something\n").unwrap();

    let config = EngineConfig {
        threads: Threads::new(1, 1),
        ..EngineConfig::new([root.to_string_lossy().replace('\\', "/")])
    };

    let named_by_the_rule = languages::parse_shipped_extension_priority().by_extension.get("m")
            .and_then(|order| order.first().cloned())
            .expect("'m' is no longer settled by the shipped priority file, so pick another extension");

    let (languages, _) = Languages::shipped(&config);
    let counted = run(&config, languages, None, |_| {}).unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(vec![&named_by_the_rule], counted.per_language.keys().collect::<Vec<_>>(),
            "'.m' went to a language the priority file does not name");
    // and the alphabetical tiebreak would have given it to the other one, so the assertion above
    // really is about the rule and not about a coincidence
    assert_ne!("MATLAB", named_by_the_rule);
}

// The whole run and the sum of its modules are the same measurement, so they have to agree about
// what is in it. Every language the run selected is given a bucket with its own keyword names set to
// zero, and the run total used to be summed before those empty buckets were dropped while a module
// total was summed after: a tree of one language came back reporting a count of nought for the
// keywords of forty languages that never appeared, and the module beside it reported none of them.
// It was invisible while the total carried no keywords at all, which is what it used to do.
#[test]
fn the_run_total_names_the_same_keywords_as_the_modules_it_is_made_of() {
    let root = std::env::temp_dir().join("mezura-total-keywords");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.rs"), "struct A {}\nstruct B {}\n").unwrap();

    let config = EngineConfig {
        threads: Threads::new(1, 1),
        ..EngineConfig::new([root.to_string_lossy().replace('\\', "/")])
    };
    let (languages, _) = Languages::shipped(&config);
    let counted = run(&config, languages, None, |_| {}).unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    let named = |keywords: &HashMap<String, usize>| {
        let mut names = keywords.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    };

    assert_eq!(vec!["Rust"], counted.per_language.keys().collect::<Vec<_>>());
    assert_eq!(Some(&2), counted.total.keyword_occurences.get("structs"));
    assert_eq!(named(&counted.per_language["Rust"].keyword_occurences), named(&counted.total.keyword_occurences),
            "the total of a run over one language names keywords that language does not have");
    assert_eq!(named(&counted.modules[0].total.keyword_occurences), named(&counted.total.keyword_occurences),
            "the total of the run and the total of its only module disagree about which keywords exist");
    assert!(!counted.total.keyword_occurences.contains_key("classes"),
            "a keyword of a language that never appeared reached the total: {:?}",
            named(&counted.total.keyword_occurences));
}

// Two spellings of one name, both claiming the same extension, is the case where every mechanism
// for choosing between claimants had nothing left to say. The name folded case before it was
// matched, so the two were indistinguishable to '--force-language' and to the priority file, and what
// answered was the alphabetical fallback, which prefers the capital: a user who typed the whole name
// of the language they wanted got the other one, with its comment symbols, and the only warning on
// the screen advised the two flags that had just failed. Nothing said the pair existed either, since
// the duplicate check was the one comparison in the crate that did not fold case.
//
// Counted through 'run' and not through the resolver, because what is wrong is the number: the two
// definitions disagree about multiline comments, so the same file is 1 comment line under one of
// them and 1 code line under the other, and that is what the assertions read.
#[test]
fn two_spellings_of_one_name_are_reported_and_force_lang_still_picks_the_one_it_was_given() {
    let root = std::env::temp_dir().join("mezura-two-spellings");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.pal"), "/* only a comment */\n").unwrap();

    // Identical but for the multiline comment, which is what makes the one line count differently
    let capital = mezura_core::Language::new("Pal", ["pal"], ["\""], ["//"], &[("/*", "*/")], []);
    let lower = mezura_core::Language::new("pal", ["pal"], ["\""], ["//"], &[], []);

    let counted_forcing = |wanted: &str| {
        let config = EngineConfig {
            threads: Threads::new(1, 1),
            forced_languages: HashMap::from([("pal".to_owned(), wanted.to_owned())]),
            ..EngineConfig::new([root.to_string_lossy().replace('\\', "/")])
        };
        let (languages, warnings) = Languages::resolve(&config,
                [capital.clone(), lower.clone()], &Default::default());
        (run(&config, languages, None, |_| {}).unwrap(), warnings)
    };

    let (as_capital, warnings) = counted_forcing("Pal");
    let (as_lower, _) = counted_forcing("pal");
    std::fs::remove_dir_all(&root).unwrap();

    let model = CountingModel::Content;
    assert_eq!(1, as_capital.total.calculate_comment_lines(model), "'--force-language pal=Pal' did not use 'Pal'");
    assert_eq!(0, as_capital.total.calculate_code_lines(model));

    assert_eq!(1, as_lower.total.calculate_code_lines(model), "'--force-language pal=pal' was given 'Pal' instead");
    assert_eq!(0, as_lower.total.calculate_comment_lines(model));

    assert!(warnings.iter().any(|warning| warning.code == mezura_core::warnings::Code::DuplicateLanguage),
            "two spellings of one name went unreported: {warnings:?}");
}

// Asking for some languages has to actually leave the others out, and the only proof of that is a
// run that counts a tree of many and comes back with few. Everything that tested this before called
// the private filtering function directly, so the whole suite passed while 'resolve' computed the
// narrowed list and then threw it away: '--languages Rust' would have counted everything, under
// every language's name, and nothing would have said a word.
#[test]
fn asking_for_some_languages_leaves_the_others_out_of_the_result() {
    let corpus = format!("{}/tests/fixtures/lang", env!("CARGO_MANIFEST_DIR").replace("\\", "/"));
    let counted_with = |narrowing: fn(&mut EngineConfig)| {
        let mut config = EngineConfig { threads: Threads::new(1, 2), ..EngineConfig::new([&corpus]) };
        narrowing(&mut config);
        let (languages, _) = Languages::shipped(&config);
        let mut names = run(&config, languages, None, |_| {}).unwrap()
                .per_language.into_keys().collect::<Vec<_>>();
        names.sort();
        names
    };

    // the control: the corpus really does hold many languages, or nothing below means anything
    let everything = counted_with(|_| {});
    assert!(everything.len() > 5, "the fixture corpus is too narrow to prove anything: {everything:?}");
    assert!(everything.contains(&"Rust".to_owned()) && everything.contains(&"Java".to_owned()), "{everything:?}");

    assert_eq!(vec!["Rust"], counted_with(|config| config.languages_of_interest = vec!["Rust".to_owned()]));
    // and by a spelling that differs in case, which is the same language
    assert_eq!(vec!["Rust"], counted_with(|config| config.languages_of_interest = vec!["rUsT".to_owned()]));

    let without_rust = counted_with(|config| config.excluded_languages = vec!["Rust".to_owned()]);
    assert!(!without_rust.contains(&"Rust".to_owned()), "an excluded language was counted anyway");
    assert_eq!(everything.len() - 1, without_rust.len(), "excluding one language removed more than one");
}

// The two arguments of 'run' have to describe the same question. They used to be allowed to
// disagree, and the result was the worst thing a counter can produce: resolving against a
// configuration naming Rust and then counting with one naming Python reported Rust, under the name
// Rust, with no error and no warning. Deriving one configuration from another is what the struct
// update syntax below is for, so the two drifting apart is ordinary use and not an exotic misuse.
#[test]
fn languages_resolved_against_another_configuration_are_refused() {
    let current_dir = env!("CARGO_MANIFEST_DIR").replace("\\", "/");
    let of = |wanted: &str| EngineConfig {
        languages_of_interest: vec![wanted.to_owned()],
        threads: Threads::new(1, 2),
        ..EngineConfig::new([format!("{current_dir}/src")])
    };

    let (languages, _) = Languages::shipped(&of("Rust"));
    let err = run(&of("Python"), languages, None, |_| {}).unwrap_err();
    assert!(matches!(err, mezura_core::RunError::LanguagesFromAnotherConfig), "got: {err:?}");

    // The same names in another order and another case are the same question, and refusing that
    // would turn a guard against wrong answers into a guard against working code
    let mut shuffled = of("Rust");
    shuffled.languages_of_interest = vec!["RUST".to_owned()];
    let (languages, _) = Languages::shipped(&of("Rust"));
    assert!(run(&shuffled, languages, None, |_| {}).is_ok(), "a difference in case alone was refused");

    // And so is counting a second directory with the languages resolved for the first: 'dirs' is
    // not part of what resolution reads, so one resolve serves as many runs as you like
    let (languages, _) = Languages::shipped(&of("Rust"));
    let elsewhere = EngineConfig { ..of("Rust") };
    let elsewhere = EngineConfig { targets: vec![Target::of(format!("{current_dir}/tests"))], ..elsewhere };
    assert!(run(&elsewhere, languages, None, |_| {}).is_ok(), "changing only the directory was refused");
}

// A run that named its modules and found nothing in them still has to say which modules it was
// asked about. The empty answer went through a shortcut that hardcoded an empty module list, so the
// document dropped the whole block and a reader could not tell "these two parts are empty" apart
// from "no parts were named".
#[test]
fn a_run_that_names_modules_and_finds_nothing_still_reports_them() {
    let root = std::env::temp_dir().join("mezura-empty-modules-test");
    let (api, web) = (root.join("api"), root.join("web"));
    std::fs::create_dir_all(&api).unwrap();
    std::fs::create_dir_all(&web).unwrap();
    let path_of = |p: &std::path::Path| p.to_str().unwrap().replace('\\', "/");

    let config = EngineConfig {
        targets: vec![Target::named("api", path_of(&api)), Target::named("web", path_of(&web))],
        threads: Threads::new(1, 2),
        ..Default::default()
    };

    let (languages, _) = Languages::shipped(&config);
    let result = run(&config, languages, None, |_| {}).unwrap();

    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(0, result.files_present.relevant_files);
    let mut names = result.modules.iter().filter_map(|x| x.name.as_deref()).collect::<Vec<_>>();
    names.sort();
    assert_eq!(vec!["api", "web"], names, "the modules that were asked about are missing from the result");
    assert!(result.modules.iter().all(|x| x.total.lines == 0));
}

#[test]
fn the_counts_of_each_file_are_kept_only_when_they_were_asked_for() {
    let root = std::env::temp_dir().join("mezura-by-file-test");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("inner")).unwrap();
    std::fs::write(root.join("a.rs"), "fn a() {}\n// one\n\nfn b() {}\n").unwrap();
    std::fs::write(root.join("inner").join("b.rs"), "fn c() {}\n").unwrap();
    std::fs::write(root.join("page.html"), "<html>\n<style>\np {color: red;}\n</style>\n</html>\n").unwrap();
    let root_str = root.to_string_lossy().replace('\\', "/");

    let counted = |collect_files: bool| {
        let config = EngineConfig { collect_files, threads: Threads::new(1, 2),
                ..EngineConfig::new([root_str.clone()]) };
        let (languages, _) = Languages::shipped(&config);
        run(&config, languages, None, |_| {}).unwrap()
    };

    let without = counted(false);
    assert!(without.modules.iter().all(|module| module.files.is_empty()),
            "the files were kept without being asked for");

    let result = counted(true);
    std::fs::remove_dir_all(&root).unwrap();

    let files_of = |language: &str| result.modules.iter()
            .flat_map(|module| module.files.get(language)).flatten().collect::<Vec<_>>();
    let mut paths = result.modules.iter().flat_map(|module| module.files.values()).flatten()
            .map(|file| file.path.clone()).collect::<Vec<_>>();
    paths.sort();
    assert_eq!(vec![format!("{root_str}/a.rs"), format!("{root_str}/inner/b.rs"),
            format!("{root_str}/page.html")], paths);

    // The rows of one language add up to what that language's own row says
    let rust = files_of("Rust");
    let whole = result.per_language.get("Rust").unwrap();
    let code_of = |stats: &mezura_core::Stats| stats.calculate_code_lines(CountingModel::Content);
    let comments_of = |stats: &mezura_core::Stats| stats.calculate_comment_lines(CountingModel::Content);
    assert_eq!(whole.lines, rust.iter().map(|file| file.stats.lines).sum::<usize>());
    assert_eq!(code_of(whole), rust.iter().map(|file| code_of(&file.stats)).sum::<usize>());
    assert_eq!(comments_of(whole), rust.iter().map(|file| comments_of(&file.stats)).sum::<usize>());
    assert_eq!(whole.files, rust.len());

    // The CSS inside the page is booked beside the file and is already inside its lines
    let page = files_of("HTML")[0];
    let css = page.nested_languages.get("CSS").expect("the style block was not kept with its file");
    assert!(css.lines > 0 && css.lines < page.stats.lines,
            "the section holds {} of the file's {} lines", css.lines, page.stats.lines);

    assert_eq!(result.total.files, paths.len());
}

// A run with one module cannot say this: every index into the per-module lists is zero there, so a
// list handed to the wrong module would pass every assertion above.
#[test]
fn each_module_keeps_the_files_that_were_counted_under_it() {
    let root = std::env::temp_dir().join("mezura-by-file-modules-test");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("web")).unwrap();
    std::fs::create_dir_all(root.join("api")).unwrap();
    std::fs::write(root.join("web").join("app.js"), "let a = 1;\nlet b = 2;\n").unwrap();
    std::fs::write(root.join("web").join("view.js"), "let c = 3;\n").unwrap();
    std::fs::write(root.join("api").join("main.rs"), "fn main() {}\n").unwrap();
    let root_str = root.to_string_lossy().replace('\\', "/");

    let config = EngineConfig { collect_files: true, threads: Threads::new(2, 4),
            targets: vec![Target::named("web", format!("{root_str}/web")),
                    Target::named("api", format!("{root_str}/api"))],
            ..EngineConfig::new([root_str.clone()]) };
    let (languages, _) = Languages::shipped(&config);
    let result = run(&config, languages, None, |_| {}).unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    for module in &result.modules {
        let name = module.name.as_deref().expect("both targets were named");
        for file in module.files.values().flatten() {
            assert!(file.path.starts_with(&format!("{root_str}/{name}/")),
                    "'{}' was kept under the module '{name}'", file.path);
        }
        for (language, whole) in &module.per_language {
            let of_this_language = module.files.get(language).unwrap();
            assert_eq!(whole.lines, of_this_language.iter().map(|file| file.stats.lines).sum::<usize>());
            assert_eq!(whole.files, of_this_language.len());
        }
    }
    assert_eq!(vec![1, 2], {
        let mut counts = result.modules.iter()
                .map(|module| module.files.values().map(Vec::len).sum::<usize>()).collect::<Vec<_>>();
        counts.sort();
        counts
    });
}

// A caller's own exclude pattern that does not parse used to bring the process down through an
// 'expect' whose message blamed an argument parsing that never ran: only the command line validates
// these, and this call never went through it. A mistake in the configuration comes back on the
// Result like every other mistake in the configuration.
#[test]
fn an_exclude_pattern_that_does_not_parse_is_an_error_not_a_panic() {
    let current_dir = env!("CARGO_MANIFEST_DIR").replace("\\", "/");
    let config = EngineConfig {
        exclude_dirs: vec!["target".to_owned(), "[invalid".to_owned()],
        threads: Threads::new(1, 1),
        ..EngineConfig::new([format!("{current_dir}/src")])
    };

    let (languages, _) = Languages::shipped(&config);

    let err = run(&config, languages, None, |_| {}).unwrap_err();
    // Named as the caller wrote it, not in the anchored form the matcher builds internally
    assert!(matches!(&err, mezura_core::RunError::InvalidExcludePattern(p) if p == "[invalid"),
            "expected InvalidExcludePattern carrying the pattern as written, got: {err:?}");
}

// Files that were all found and all failed to parse are an answer about the corpus, not a failure
// of the run: the counting worked, the counts are zero, and the faulty list says why. Returning it
// as an error is what left '--output json' printing nothing at all in exactly this case, because
// the error arm never reaches the printer.
#[test]
fn a_run_where_every_file_fails_to_parse_is_an_answer_not_an_error() {
    let root = std::env::temp_dir().join("mezura-all-faulty-test");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.rs"), [0xFF, 0xFE, 0x80, b'\n']).unwrap();
    std::fs::write(root.join("b.rs"), [0xFF, 0xFE, 0x80, b'\n']).unwrap();
    let root_str = root.to_str().unwrap().replace('\\', "/");

    let config = EngineConfig { threads: Threads::new(1, 2), ..EngineConfig::new([root_str]) };
    let (languages, _) = Languages::shipped(&config);

    let result = run(&config, languages, None, |_| {}).unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(2, result.faulty_files.len());
    assert_eq!(2, result.files_present.relevant_files);
    assert!(result.all_relevant_files_were_faulty());
    assert!(result.total.lines == 0 && result.per_language.is_empty());

    // The empty scan answers the same question with a no: nothing failed, there was nothing
    let empty = std::env::temp_dir().join("mezura-all-faulty-empty");
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).unwrap();
    let config = EngineConfig {
        threads: Threads::new(1, 1),
        ..EngineConfig::new([empty.to_str().unwrap().replace('\\', "/")])
    };
    let (languages, _) = Languages::shipped(&config);
    let result = run(&config, languages, None, |_| {}).unwrap();
    std::fs::remove_dir_all(&empty).unwrap();

    assert!(!result.all_relevant_files_were_faulty());
}

// A configuration with no targets at all is a malformed question, not an empty answer: the command
// line can never produce one, because a bare run falls back to the working directory, so it is a
// library caller forgetting dirs, and an Ok full of zeros would dress the mistake up as a
// measurement.
#[test]
fn a_run_with_no_targets_is_an_error_not_an_empty_answer() {
    let config = EngineConfig::default();
    let (languages, _) = Languages::shipped(&config);

    let err = run(&config, languages, None, |_| {}).unwrap_err();
    assert!(matches!(err, mezura_core::RunError::NoTargets), "got: {err:?}");
}

// The flag says no more walking will happen, and a run that refuses before walking is exactly
// that: a watcher waiting on it must not wait forever because the run never started
#[test]
fn the_walk_flag_rises_even_when_the_run_refuses() {
    let progress = std::sync::Arc::new(mezura_core::ScanProgress::default());
    let config = EngineConfig::default();
    let (languages, _) = Languages::shipped(&config);

    let refused = run(&config, languages, Some(progress.clone()), |_| {});
    assert!(refused.is_err());
    assert!(progress.is_walk_done(), "a refused run left the walk flag down forever");

    // and the same for a run that dies resolving its targets, which is a later refusal
    let config = EngineConfig::new(["./does-not-exist-walk-flag"]);
    let (languages, _) = Languages::shipped(&config);
    let progress = std::sync::Arc::new(mezura_core::ScanProgress::default());
    assert!(run(&config, languages, Some(progress.clone()), |_| {}).is_err());
    assert!(progress.is_walk_done());
}

// The declared targets are the run's to resolve, so a mistake in them comes back on its Result
// like every other mistake in the configuration, carrying the path exactly as it was declared, and
// each kind of mistake keeps its own variant so a caller can tell them apart.
#[test]
fn a_target_that_names_nothing_is_a_run_error() {
    let config = EngineConfig::new(["./does-not-exist-run"]);
    let (languages, _) = Languages::shipped(&config);

    let err = run(&config, languages, None, |_| {}).unwrap_err();
    assert!(matches!(&err, mezura_core::RunError::InvalidTargets(mezura_core::TargetError::InvalidPath(p)) if p == "./does-not-exist-run"),
            "got: {err:?}");

    // one place under two names travels the same road
    let root = std::env::temp_dir().join("mezura-run-contested");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root_str = root.to_str().unwrap().replace('\\', "/");
    let config = EngineConfig {
        targets: vec![Target::named("a", &root_str), Target::named("b", &root_str)],
        ..Default::default()
    };
    let (languages, _) = Languages::shipped(&config);
    let err = run(&config, languages, None, |_| {}).unwrap_err();
    std::fs::remove_dir_all(&root).unwrap();
    assert!(matches!(&err, mezura_core::RunError::InvalidTargets(mezura_core::TargetError::Contested(..))), "got: {err:?}");
}

// What was measured is on the result, resolved: the declared form answers a different question,
// since the same relative path declared over two different working directories is two different
// measurements, and a pattern's matches change while its text does not.
#[test]
fn the_result_reports_the_resolved_targets_the_run_walked() {
    let root = std::env::temp_dir().join("mezura-result-targets");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("sub1")).unwrap();
    std::fs::create_dir_all(root.join("sub2")).unwrap();
    std::fs::write(root.join("sub1").join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(root.join("sub2").join("b.rs"), "fn b() {}\n").unwrap();
    let root_str = root.to_str().unwrap().replace('\\', "/");

    let config = EngineConfig {
        threads: Threads::new(1, 1),
        ..EngineConfig::new([format!("{root_str}/sub*")])
    };
    let (languages, _) = Languages::shipped(&config);
    let result = run(&config, languages, None, |_| {}).unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    let mut walked = result.targets.iter().map(|x| x.path.clone()).collect::<Vec<_>>();
    walked.sort();
    assert_eq!(vec![format!("{root_str}/sub1"), format!("{root_str}/sub2")], walked);
    // and the configuration still holds what was declared
    assert_eq!(vec![Target::of(format!("{root_str}/sub*"))], config.targets);
}

// A pattern's match may itself be named like a pattern: 'a?b' legitimately matches a directory
// literally called 'a[b'. Resolution is existence-first, so the match is a place that exists and
// is taken as itself, never re-read as a pattern of its own, however many times it is resolved.
#[test]
fn a_resolved_match_named_like_a_pattern_is_counted_not_re_expanded() {
    let root = std::env::temp_dir().join("mezura-bracket-match");
    let _ = std::fs::remove_dir_all(&root);
    let inner = root.join("a[b");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(inner.join("x.rs"), "fn x() { let a = 1; }\n").unwrap();
    let pattern = root.to_str().unwrap().replace('\\', "/") + "/a?b";

    let config = EngineConfig { threads: Threads::new(1, 1), ..EngineConfig::new([pattern]) };
    let (languages, _) = Languages::shipped(&config);
    let result = run(&config, languages, None, |_| {}).unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(1, result.total.files, "the directory the pattern matched was not counted");
}

// Nobody can ask for a thread count the run cannot work with. Zero producers left every directory
// sitting in the queue with nothing to drain it, so a scan of a real tree came back saying it found
// nothing at all; zero consumers was worse, returning a result that claimed relevant files and zero
// of everything in the same breath. Both are silent wrong answers, which is the one thing a counter
// must never produce, and the command line never saw either because it validates its own input.
#[test]
fn a_thread_count_outside_the_supported_range_cannot_reach_the_run() {
    let current_dir = env!("CARGO_MANIFEST_DIR").replace("\\", "/");
    let count_with = |producers: usize, consumers: usize| {
        let config = EngineConfig {
            threads: Threads::new(producers, consumers),
            ..EngineConfig::new([format!("{current_dir}/src")])
        };
        let (languages, _) = Languages::shipped(&config);
        let result = run(&config, languages, None, |_| {}).unwrap();
        (result.files_present.relevant_files, result.total.lines)
    };

    let sane = count_with(1, 2);
    assert!(sane.0 > 0 && sane.1 > 0, "the control run counted nothing");

    assert_eq!(sane, count_with(0, 0), "zero of both");
    assert_eq!(sane, count_with(0, 4), "zero producers");
    assert_eq!(sane, count_with(2, 0), "zero consumers");
    // Far above the cap, which used to reach Vec::with_capacity and the spawn loop as written
    assert_eq!(sane, count_with(usize::MAX, usize::MAX), "absurdly many");

    // And what was actually used is readable, rather than the number that was asked for
    let config = EngineConfig {
        threads: Threads::new(0, 100_000),
        ..EngineConfig::new([format!("{current_dir}/src")])
    };
    assert_eq!((1, 128), (config.threads.producers(), config.threads.consumers()));
}

// What the run actually used, which the caller cannot know: the requested counts are their own
// config, but the operating system is allowed to grant fewer, and the run carries on with what it
// was given. On a result that exists this is also how many finished whole, because a worker that
// dies turns the whole run into an error instead.
#[test]
fn the_result_reports_the_threads_the_run_actually_used() {
    let current_dir = env!("CARGO_MANIFEST_DIR").replace("\\", "/");
    let config = EngineConfig {
        threads: Threads::new(2, 3),
        ..EngineConfig::new([format!("{current_dir}/src")])
    };

    let (languages, _) = Languages::shipped(&config);
    let result = run(&config, languages, None, |_| {}).unwrap();
    assert_eq!(Threads::new(2, 3), result.performance.threads);

    // and the empty scan reports its threads too, since they ran all the same
    let empty = std::env::temp_dir().join("mezura-threads-empty");
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).unwrap();
    let config = EngineConfig {
        threads: Threads::new(1, 2),
        ..EngineConfig::new([empty.to_string_lossy().replace("\\", "/")])
    };
    let (languages, _) = Languages::shipped(&config);
    let result = run(&config, languages, None, |_| {}).unwrap();
    std::fs::remove_dir_all(&empty).unwrap();
    assert_eq!(0, result.files_present.relevant_files);
    assert_eq!(Threads::new(1, 2), result.performance.threads);
}

// The walk is announced while the counting of what it queued is still running, which is the only
// moment the callback exists for. Called exactly once, whatever the run finds, and with the walk's
// final counts: it used to fire after everything had been joined, where the same figures are
// already sitting on the result and the announcement had nothing left to announce.
#[test]
fn the_traversal_callback_fires_once_with_what_the_walk_found() {
    let current_dir = env!("CARGO_MANIFEST_DIR").replace("\\", "/");
    let config = EngineConfig {
        threads: Threads::new(1, 2),
        ..EngineConfig::new([format!("{current_dir}/src")])
    };
    let (languages, _) = Languages::shipped(&config);

    let mut announced = Vec::new();
    let result = run(&config, languages, None, |scan| announced.push(scan)).unwrap();

    assert_eq!(1, announced.len(), "the callback did not fire exactly once");
    assert_eq!(result.files_present, announced[0]);
    assert!(announced[0].relevant_files > 0);
}

// A walk that found nothing still finished, and the caller is told so: the announcement is about
// the walk and not about whether it was worth anything. Asserted because the arity and the value
// above hold just as well at the end of a run, so nothing there notices a callback that quietly
// grew a condition.
#[test]
fn the_traversal_callback_fires_even_when_the_walk_finds_nothing() {
    let empty = std::env::temp_dir().join("mezura-callback-empty");
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).unwrap();

    let config = EngineConfig {
        threads: Threads::new(1, 1),
        ..EngineConfig::new([empty.to_string_lossy().replace('\\', "/")])
    };
    let (languages, _) = Languages::shipped(&config);

    let mut announced = Vec::new();
    let result = run(&config, languages, None, |scan| announced.push(scan)).unwrap();
    std::fs::remove_dir_all(&empty).unwrap();

    assert_eq!(vec![result.files_present], announced, "an empty walk was never announced");
    assert_eq!(0, announced[0].relevant_files);
}

// The callback runs while the consumers are still draining, so the clock that stops when they are
// joined has been holding whichever of the two took longer. What the result reports is the counting,
// so the caller's own wait comes back out of it: a run measured as 9ms with an instant callback was
// being reported as 1,201ms with a slow one, which turned 5,500 files a second into 41 and
// fabricated a 'Metrics' block that only exists for runs over a second.
#[test]
fn a_slow_traversal_callback_is_not_charged_to_the_counting() {
    let root = std::env::temp_dir().join("mezura-callback-clock");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for i in 0..20 {
        std::fs::write(root.join(format!("f{i}.rs")), "fn a() { let x = 1; }\n").unwrap();
    }
    let config = EngineConfig {
        threads: Threads::new(1, 2),
        ..EngineConfig::new([root.to_string_lossy().replace('\\', "/")])
    };

    let counted = |held_for: std::time::Duration| {
        let (languages, _) = Languages::shipped(&config);
        run(&config, languages, None, move |_| std::thread::sleep(held_for)).unwrap()
    };

    let held_for = std::time::Duration::from_millis(600);
    let prompt = counted(std::time::Duration::ZERO);
    let slow = counted(held_for);
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(20, prompt.total.files);
    assert_eq!(prompt.total, slow.total);
    // Generously, since what is being ruled out is the whole 600ms landing in the figure and not a
    // few milliseconds of scheduling noise on a loaded machine
    assert!(slow.performance.duration_millis < held_for.as_millis() / 2,
            "a callback that slept {}ms turned a {}ms count into {}ms, so the caller is being charged \
             to the parser", held_for.as_millis(), prompt.performance.duration_millis,
            slow.performance.duration_millis);
}
