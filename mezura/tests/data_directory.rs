use std::path::Path;
use std::process::{Command, Output};

#[test]
fn version_help_and_changelog_leave_the_data_directory_untouched() {
    let dir = std::env::temp_dir().join("mezura-version-help-and-changelog");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    for command in ["--version", "--help", "--changelog"] {
        let output = run_mezura(&dir, &[command]);
        assert!(output.status.success(), "'{command}' failed: {}", String::from_utf8_lossy(&output.stderr));
        let written = std::fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name()).collect::<Vec<_>>();
        assert!(written.is_empty(), "'{command}' wrote into the data directory: {written:?}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_data_directory_a_newer_release_wrote_stops_the_run_until_restore() {
    let dir = std::env::temp_dir().join("mezura-newer-release");
    let _ = std::fs::remove_dir_all(&dir);
    let data = dir.join("data");
    let target = dir.join("target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("main.rs"), "fn main() {}\n").unwrap();
    let count_the_target = || run_mezura(&data, &[target.to_str().unwrap()]);
    assert!(run_mezura(&data, &["--restore"]).status.success());
    let ours = std::fs::read_to_string(data.join("installed.txt")).unwrap();

    write_version_line(&data, "v99.0.0");
    let theirs = std::fs::read_to_string(data.join("installed.txt")).unwrap();
    assert!(run_mezura(&data, &["--version"]).status.success(), "'--version' stopped at the newer manifest");
    let refused = count_the_target();
    assert!(!refused.status.success(), "a run went on over a directory a newer release wrote");
    assert!(String::from_utf8_lossy(&refused.stderr).contains("v99.0.0"));
    assert_eq!(theirs, std::fs::read_to_string(data.join("installed.txt")).unwrap(), "the refused run wrote the manifest");

    assert!(run_mezura(&data, &["--restore"]).status.success());
    assert_eq!(ours, std::fs::read_to_string(data.join("installed.txt")).unwrap(), "'--restore' left the newer manifest");
    assert!(count_the_target().status.success());

    let _ = std::fs::remove_dir_all(&dir);
}

fn run_mezura(data_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mezura")).args(args).env("MEZURA_DATA_DIR", data_dir).output().unwrap()
}

fn write_version_line(data_dir: &Path, version_line: &str) {
    let path = data_dir.join("installed.txt");
    let manifest = std::fs::read_to_string(&path).unwrap();
    let written = manifest.lines().find(|line| !line.trim().is_empty() && !line.starts_with('#')).unwrap();
    std::fs::write(&path, manifest.replacen(written, version_line, 1)).unwrap();
}
