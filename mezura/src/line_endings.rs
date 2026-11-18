// What a checkout would write for each file of a revision, so a file read from git's objects has a checkout's bytes
use std::collections::HashMap;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use super::git::build_git_read;

const ATTRIBUTES : [&str; 6] = ["text", "crlf", "eol", "filter", "ident", "working-tree-encoding"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Conversion {
    Untouched,
    Auto,
    Text,
    ByGit
}

pub(crate) struct CheckoutSettings {
    pub symlinks: bool,
    autocrlf: bool,
    text_ends_in_crlf: bool,
    attributes_outside_the_revision: bool
}

impl CheckoutSettings {
    pub(crate) fn read(repository: &str) -> CheckoutSettings {
        let (settings, of_the_system, of_the_person) = (
            start_plain_git(repository, &["config", "-z", "--get-regexp", r"^core\.(symlinks|autocrlf|eol)$"]),
            start_plain_git(repository, &["var", "GIT_ATTR_SYSTEM"]),
            start_plain_git(repository, &["var", "GIT_ATTR_GLOBAL"]));
        let git_folder = Path::new(repository).join(".git");
        let of_the_clone = match git_folder.is_dir() {
            true => Some(git_folder.join("info").join("attributes")),
            false => read_answer_of(start_plain_git(repository, &["rev-parse", "--git-path", "info/attributes"]))
                    .map(|path| Path::new(repository).join(path.trim()))
        };
        let settings = read_answer_of(settings).unwrap_or_default();
        let find = |name: &str| settings.split('\0').rev().find_map(|setting| {
            let (key, value) = setting.split_once('\n').map_or((setting, None), |(key, value)| (key, Some(value)));
            (key == name).then_some(value)
        });
        let autocrlf = find("core.autocrlf");
        let autocrlf_is_input = autocrlf.flatten().is_some_and(|value| value.eq_ignore_ascii_case("input"));
        let autocrlf_is_on = !autocrlf_is_input && autocrlf.is_some_and(is_true_to_git);
        let eol = find("core.eol").flatten().unwrap_or_default().to_ascii_lowercase();
        let files = [read_answer_of(of_the_system).map(|path| PathBuf::from(path.trim())),
                read_answer_of(of_the_person).map(|path| PathBuf::from(path.trim())), of_the_clone];

        CheckoutSettings {
            symlinks: find("core.symlinks").is_none_or(is_true_to_git),
            autocrlf: autocrlf_is_on,
            text_ends_in_crlf: autocrlf_is_on || !autocrlf_is_input && match eol.as_str() {
                "crlf" => true,
                "lf" => false,
                _ => cfg!(windows)
            },
            attributes_outside_the_revision: files.iter().any(|file| match file.as_ref().map(std::fs::read) {
                Some(Ok(attributes)) => names_a_conversion(&attributes),
                Some(Err(error)) => error.kind() != ErrorKind::NotFound,
                None => true
            })
        }
    }
}

// A git before 2.40 reads no attributes of a revision, and there 'core.autocrlf' decides for every file
pub(crate) fn decide_conversions(settings: &CheckoutSettings, repository: &str, commit: &str, paths: &[&[u8]], links: &[usize],
        read_blob: impl Fn(usize, &mut Vec<u8>) -> std::io::Result<()>) -> (Vec<Conversion>, HashMap<usize, String>)
{
    let mut attributes = Vec::new();
    let attributes_apply = settings.attributes_outside_the_revision || paths.iter().enumerate()
            .filter(|(_, path)| path.rsplit(|byte| *byte == b'/').next() == Some(b".gitattributes".as_slice()))
            .any(|(at, _)| {
                attributes.clear();
                read_blob(at, &mut attributes).is_err() || names_a_conversion(&attributes)
            });
    let (mut conversions, paths_git_writes_out) = attributes_apply.then(|| ask_git_for_attributes(settings, repository, commit, paths))
            .flatten().unwrap_or_else(|| (match settings.autocrlf {
                true => vec![Conversion::Auto; paths.len()],
                false => Vec::new()
            }, HashMap::new()));
    for link in links {
        if let Some(conversion) = conversions.get_mut(*link) {
            *conversion = Conversion::Untouched;
        }
    }
    (conversions, paths_git_writes_out)
}

pub(crate) fn convert_line_endings(conversion: Conversion, bytes: &mut Vec<u8>) {
    let bare_line_feeds = match conversion {
        Conversion::Auto => count_line_feeds_of_text(bytes).unwrap_or(0),
        Conversion::Text => (0..bytes.len()).filter(|at| bytes[*at] == b'\n' && (*at == 0 || bytes[at - 1] != b'\r')).count(),
        Conversion::Untouched | Conversion::ByGit => 0
    };
    let (mut from, mut to) = (bytes.len(), bytes.len() + bare_line_feeds);
    bytes.resize(to, 0);
    while from < to {
        from -= 1;
        to -= 1;
        bytes[to] = bytes[from];
        if bytes[from] == b'\n' && (from == 0 || bytes[from - 1] != b'\r') {
            to -= 1;
            bytes[to] = b'\r';
        }
    }
}

pub(crate) fn read_as_git_writes_it(repository: &str, commit: &str, path: &str, id: &str, buf: &mut Vec<u8>) -> std::io::Result<()> {
    let outcome = build_git_read(repository).arg(format!("--attr-source={commit}")).args(["cat-file", "--filters"])
            .arg(format!("--path={path}")).arg(id).output()?;
    if !outcome.status.success() {
        return Err(std::io::Error::other(String::from_utf8_lossy(&outcome.stderr).trim().to_owned()));
    }
    buf.extend_from_slice(&outcome.stdout);
    Ok(())
}

fn start_plain_git(repository: &str, arguments: &[&str]) -> Option<Child> {
    Command::new("git").env("GIT_TERMINAL_PROMPT", "0").arg("-C").arg(repository).args(arguments)
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()
}

fn read_answer_of(git: Option<Child>) -> Option<String> {
    let outcome = git?.wait_with_output().ok()?;
    outcome.status.success().then(|| String::from_utf8_lossy(&outcome.stdout).into_owned())
}

fn is_true_to_git(value: Option<&str>) -> bool {
    value.is_none_or(|value| ["true", "yes", "on"].iter().any(|word| value.eq_ignore_ascii_case(word))
            || value.parse::<i64>().is_ok_and(|number| number != 0))
}

// Where no file of attributes names one of the six or git's macro 'binary', no path has one, through another macro either
fn names_a_conversion(attributes: &[u8]) -> bool {
    attributes.split(|byte| *byte == b'\n').filter(|line| !line.trim_ascii_start().starts_with(b"#"))
            .flat_map(|line| line.split(u8::is_ascii_whitespace))
            .any(|word| {
                let name = word.strip_prefix(b"-").or_else(|| word.strip_prefix(b"!")).unwrap_or(word);
                let name = name.split(|byte| *byte == b'=').next().unwrap_or_default();
                name == b"binary" || ATTRIBUTES.iter().any(|attribute| attribute.as_bytes() == name)
            })
}

fn ask_git_for_attributes(settings: &CheckoutSettings, repository: &str, commit: &str, paths: &[&[u8]])
        -> Option<(Vec<Conversion>, HashMap<usize, String>)>
{
    let mut git = build_git_read(repository).arg("check-attr").arg(format!("--source={commit}")).args(["-z", "--stdin"])
            .args(ATTRIBUTES).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let (mut input, mut answers) = (git.stdin.take()?, BufReader::new(git.stdout.take()?));
    let mut conversions = Vec::with_capacity(paths.len());
    let mut paths_git_writes_out = HashMap::new();
    let answered_every_path = std::thread::scope(|scope| {
        scope.spawn(move || paths.iter().try_for_each(|path| input.write_all(path).and_then(|()| input.write_all(&[0]))));
        let mut values : [Vec<u8>; 6] = Default::default();
        for (at, path) in paths.iter().enumerate() {
            for value in &mut values {
                value.clear();
                let read = [answers.skip_until(0), answers.skip_until(0), answers.read_until(0, value)];
                if read.iter().any(|field| !matches!(field, Ok(1..))) {
                    let _ = git.kill();
                    return false;
                }
                value.pop();
            }
            let conversion = decide_conversion(settings, &values);
            if conversion == Conversion::ByGit {
                paths_git_writes_out.insert(at, String::from_utf8_lossy(path).into_owned());
            }
            conversions.push(conversion);
        }
        true
    });
    (git.wait().ok()?.success() && answered_every_path).then_some((conversions, paths_git_writes_out))
}

// A filter other than lfs is a program of the repository's configuration, so its path is never handed to git
fn decide_conversion(settings: &CheckoutSettings, [text, crlf, eol, filter, ident, encoding]: &[Vec<u8>; 6]) -> Conversion {
    let is_encoded = !matches!(encoding.as_slice(), b"unspecified" | b"unset" | b"set")
            && !encoding.eq_ignore_ascii_case(b"utf-8") && !encoding.eq_ignore_ascii_case(b"utf8");
    if (ident == b"set" || is_encoded) && matches!(filter.as_slice(), b"unspecified" | b"unset" | b"lfs") {
        return Conversion::ByGit;
    }
    let text = if matches!(text.as_slice(), b"set" | b"unset" | b"auto" | b"input") { text } else { crlf };
    match (text.as_slice(), eol.as_slice()) {
        (b"unset", _) | (_, b"lf") => Conversion::Untouched,
        (b"auto", b"crlf") => Conversion::Auto,
        (_, b"crlf") => Conversion::Text,
        (b"set", _) if settings.text_ends_in_crlf => Conversion::Text,
        (b"auto", _) if settings.text_ends_in_crlf => Conversion::Auto,
        (b"set" | b"auto" | b"input", _) => Conversion::Untouched,
        _ if settings.autocrlf => Conversion::Auto,
        _ => Conversion::Untouched
    }
}

// Git's test for a text blob, with the end of file byte of DOS at the very end left out of the count
fn count_line_feeds_of_text(bytes: &[u8]) -> Option<usize> {
    let (mut line_feeds, mut printable, mut unprintable) = (0usize, 0usize, 0usize);
    for byte in bytes {
        match byte {
            b'\r' | 0 => return None,
            b'\n' => line_feeds += 1,
            8 | 9 | 12 | 27 | 32..=126 | 128.. => printable += 1,
            _ => unprintable += 1
        }
    }
    if bytes.last() == Some(&0x1A) {
        unprintable -= 1;
    }
    (printable >> 7 >= unprintable).then_some(line_feeds)
}
