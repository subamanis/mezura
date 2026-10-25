use std::fs;
use std::path::Path;

use crate::engine::file_parser::BYTE_ORDER_MARK;
use crate::engine::path_patterns::{PathPatternMatcher, TakingBack, is_a_path_pattern};
use crate::engine::targets::spell_out;

// In pytest's own order of precedence. The first file present decides, and the first four decide
// even when they hold nothing. A '[pytest]' table and '[tool.pytest]' take TOML lists only, since
// pytest 9 refuses a string there. '[tool.pytest.ini_options]' splits a string as an ini value is.
const PYTEST_CONFIGURATION_FILES : [ConfigurationFile; 7] = [
    ConfigurationFile { name: "pytest.toml", shape: Shape::Toml("pytest"), decides_even_when_empty: true },
    ConfigurationFile { name: ".pytest.toml", shape: Shape::Toml("pytest"), decides_even_when_empty: true },
    ConfigurationFile { name: "pytest.ini", shape: Shape::Ini("pytest"), decides_even_when_empty: true },
    ConfigurationFile { name: ".pytest.ini", shape: Shape::Ini("pytest"), decides_even_when_empty: true },
    ConfigurationFile { name: "pyproject.toml", shape: Shape::Pyproject, decides_even_when_empty: false },
    ConfigurationFile { name: "tox.ini", shape: Shape::Ini("pytest"), decides_even_when_empty: false },
    ConfigurationFile { name: "setup.cfg", shape: Shape::Ini("tool:pytest"), decides_even_when_empty: false },
];
pub(crate) const PYTEST_CONFIGURATION_FILE_NAMES : [&str; PYTEST_CONFIGURATION_FILES.len()] = collect_configuration_file_names();
pub(crate) const CONFTEST : &str = "conftest.py";
const TESTPATHS : &str = "testpaths";
const PYTHON_FILES : &str = "python_files";
const NATIVE_TABLE : &str = "tool.pytest";
const INI_OPTIONS_TABLE : &str = "tool.pytest.ini_options";

pub(crate) struct PytestConfiguration {
    pub file: &'static str,
    pub testpaths: Option<Vec<String>>,
    pub python_files: Option<Vec<String>>,
}

impl PytestConfiguration {
    // Under a 'conftest.py' a declared name applies only when it says 'test' itself, since a bare
    // '__init__.py' at a root would take every package file of the project
    pub(crate) fn collect_test_shaped_names(&self) -> Option<Vec<String>> {
        let shaped = self.python_files.as_ref()?.iter().filter(|name| is_test_shaped(name)).cloned().collect::<Vec<_>>();
        (!shaped.is_empty()).then_some(shaped)
    }
}

pub(crate) fn read_pytest_configuration(directory: &Path, is_present: impl Fn(&str) -> bool) -> Option<PytestConfiguration> {
    for file in &PYTEST_CONFIGURATION_FILES {
        if !is_present(file.name) {
            continue;
        }
        let bytes = fs::read(directory.join(file.name)).ok()?;
        let text = std::str::from_utf8(bytes.strip_prefix(BYTE_ORDER_MARK).unwrap_or(&bytes)).ok()?;
        let found = match file.shape {
            Shape::Ini(section) => read_ini(text, section),
            Shape::Toml(table) => read_toml(text, &[(table, StringValue::Refused)]),
            Shape::Pyproject => read_toml(text, &[(NATIVE_TABLE, StringValue::Refused), (INI_OPTIONS_TABLE, StringValue::Split)])
        };
        match found {
            Section::Present(declared) => return Some(build_configuration(file.name, declared, directory)),
            Section::Absent if file.decides_even_when_empty => return Some(build_configuration(file.name, Declared::default(), directory)),
            Section::Absent => ()
        }
    }
    None
}

struct ConfigurationFile {
    name: &'static str,
    shape: Shape,
    decides_even_when_empty: bool,
}

enum Shape {
    Ini(&'static str),
    Toml(&'static str),
    Pyproject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StringValue {
    Split,
    Refused,
}

#[derive(Debug, PartialEq, Eq)]
enum Section {
    Absent,
    Present(Declared),
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Declared {
    testpaths: Option<Vec<String>>,
    python_files: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Last {
    Nothing,
    Section,
    Key(Option<usize>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    Wanted,
    InPytestTable,
    Elsewhere,
}

#[derive(Debug, PartialEq, Eq)]
enum Value {
    Str(String),
    Strings(Vec<String>),
    Other,
}

struct Unreadable;

struct WantedTable {
    name: &'static str,
    strings: StringValue,
    seen: bool,
    has_content: bool,
    values: [Option<Value>; 2],
}

const fn collect_configuration_file_names() -> [&'static str; PYTEST_CONFIGURATION_FILES.len()] {
    let mut names = [""; PYTEST_CONFIGURATION_FILES.len()];
    let mut at = 0;
    while at < names.len() {
        names[at] = PYTEST_CONFIGURATION_FILES[at].name;
        at += 1;
    }
    names
}

fn build_configuration(file: &'static str, declared: Declared, directory: &Path) -> PytestConfiguration {
    let spelled = spell_out(directory);
    let spelled = spelled.trim_end_matches('/');
    PytestConfiguration {
        file,
        testpaths: declared.testpaths.as_deref().and_then(|entries| resolve_testpaths(entries, spelled)),
        python_files: declared.python_files.as_deref().and_then(accept_python_files)
    }
}

// Read the way pytest's iniconfig reads it. An inline '#' stays in the value.
fn read_ini(text: &str, section: &str) -> Section {
    let mut declared: [Option<String>; 2] = [None, None];
    let mut in_section = false;
    let mut seen_section = false;
    let mut last = Last::Nothing;
    for line in text.lines() {
        let content = line.trim_start();
        if content.is_empty() || content.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with('[') {
            let header = line.split(['#', ';']).next().unwrap_or_default().trim_end();
            let Some(name) = header.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) else { return Section::Absent };
            in_section = name == section;
            if in_section {
                if seen_section { return Section::Absent; }
                seen_section = true;
            }
            last = Last::Section;
        } else if line.starts_with(char::is_whitespace) {
            match last {
                Last::Key(Some(slot)) => {
                    let value = declared[slot].get_or_insert_default();
                    if !value.is_empty() { value.push('\n'); }
                    value.push_str(content.trim_end());
                },
                Last::Key(None) => (),
                _ => return Section::Absent
            }
        } else {
            if last == Last::Nothing { return Section::Absent; }
            let Some((name, value)) = split_key_and_value(line) else { return Section::Absent };
            last = Last::Key(None);
            if in_section && let Some(slot) = find_wanted_key(name) {
                if declared[slot].is_some() { return Section::Absent; }
                declared[slot] = Some(value.to_owned());
                last = Last::Key(Some(slot));
            }
        }
    }
    if !seen_section {
        return Section::Absent;
    }
    let [testpaths, python_files] = declared;
    Section::Present(Declared { testpaths: testpaths.as_deref().and_then(split_ini_value), python_files: python_files.as_deref().and_then(split_ini_value) })
}

// A 'pyproject.toml' holding both pytest tables with content reads as nothing, as pytest refuses it
fn read_toml(text: &str, wanted: &[(&'static str, StringValue)]) -> Section {
    let mut tables = wanted.iter().map(|&(name, strings)| WantedTable { name, strings, seen: false, has_content: false, values: [None, None] })
            .collect::<Vec<_>>();
    // A file the reader lost its place in may hold the table further down, and pytest would decide there
    if walk_toml(&mut Cursor { text, at: 0 }, &mut tables).is_err() {
        return Section::Present(Declared::default());
    }
    if !tables.iter().any(|table| table.seen) {
        return Section::Absent;
    }
    if tables.iter().filter(|table| table.has_content).count() > 1 {
        return Section::Present(Declared::default());
    }
    let Some(table) = tables.iter_mut().find(|table| table.has_content) else { return Section::Present(Declared::default()) };
    let [testpaths, python_files] = std::mem::take(&mut table.values);
    Section::Present(Declared { testpaths: testpaths.and_then(|value| take_entries(value, table.strings)),
            python_files: python_files.and_then(|value| take_entries(value, table.strings)) })
}

fn walk_toml(cursor: &mut Cursor<'_>, tables: &mut [WantedTable]) -> Result<(), Unreadable> {
    let mut current: Option<usize> = None;
    loop {
        cursor.skip_blank_lines_and_comments();
        let Some(byte) = cursor.peek() else { return Ok(()) };
        if byte == b'[' {
            let (name, array_of_tables) = cursor.read_header()?;
            current = None;
            let Some(name) = name else { continue };
            if let Some(at) = tables.iter().position(|table| table.name == name) {
                if array_of_tables || tables[at].seen { return Err(Unreadable); }
                tables[at].seen = true;
                current = Some(at);
            } else if let Some(at) = tables.iter().position(|table| name.starts_with(table.name) && name[table.name.len()..].starts_with('.')) {
                tables[at].seen = true;
                tables[at].has_content = true;
            }
            continue;
        }
        let (wanted_key, bare) = cursor.read_key()?;
        let Some(at) = current else {
            cursor.read_value(Reading::Elsewhere, false)?;
            cursor.finish_line()?;
            continue;
        };
        if !bare { return Err(Unreadable); }
        tables[at].has_content = true;
        let value = cursor.read_value(if wanted_key.is_some() { Reading::Wanted } else { Reading::InPytestTable }, false)?;
        cursor.finish_line()?;
        if let Some(slot) = wanted_key {
            if tables[at].values[slot].is_some() { return Err(Unreadable); }
            tables[at].values[slot] = Some(value);
        }
    }
}

struct Cursor<'a> {
    text: &'a str,
    at: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn skip_blank_lines_and_comments(&mut self) {
        loop {
            while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) { self.at += 1; }
            if self.peek() != Some(b'#') { return; }
            self.skip_to_line_end();
        }
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r')) { self.at += 1; }
    }

    fn skip_to_line_end(&mut self) {
        while self.peek().is_some_and(|byte| byte != b'\n') { self.at += 1; }
    }

    fn finish_line(&mut self) -> Result<(), Unreadable> {
        self.skip_spaces();
        match self.peek() {
            None | Some(b'\n') => Ok(()),
            Some(b'#') => { self.skip_to_line_end(); Ok(()) },
            Some(_) => Err(Unreadable)
        }
    }

    fn read_header(&mut self) -> Result<(Option<String>, bool), Unreadable> {
        let array_of_tables = self.text[self.at..].starts_with("[[");
        self.at += if array_of_tables { 2 } else { 1 };
        let start = self.at;
        while self.peek().is_some_and(|byte| byte != b']' && byte != b'\n') { self.at += 1; }
        if self.peek() != Some(b']') { return Err(Unreadable); }
        let inside = &self.text[start..self.at];
        self.at += 1;
        if array_of_tables {
            if self.peek() != Some(b']') { return Err(Unreadable); }
            self.at += 1;
        }
        self.finish_line()?;
        let parts = inside.split('.').map(str::trim).collect::<Vec<_>>();
        let plain = parts.iter().all(|part| !part.is_empty() && !part.contains(['"', '\'', ' ', '\t']));
        Ok((plain.then(|| parts.join(".")), array_of_tables))
    }

    fn read_key(&mut self) -> Result<(Option<usize>, bool), Unreadable> {
        let start = self.at;
        loop {
            match self.peek() {
                None | Some(b'\n') => return Err(Unreadable),
                Some(b'=') => break,
                Some(quote @ (b'"' | b'\'')) => {
                    self.at += 1;
                    while self.peek().is_some_and(|byte| byte != quote && byte != b'\n') { self.at += 1; }
                    if self.peek() != Some(quote) { return Err(Unreadable); }
                    self.at += 1;
                },
                Some(_) => self.at += 1
            }
        }
        let key = self.text[start..self.at].trim();
        self.at += 1;
        let bare = !key.is_empty() && key.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
        Ok((find_wanted_key(key), bare))
    }

    fn read_value(&mut self, reading: Reading, in_array: bool) -> Result<Value, Unreadable> {
        self.skip_spaces();
        let rest = &self.text[self.at..];
        if rest.starts_with("\"\"\"") || rest.starts_with("'''") {
            if reading == Reading::Wanted { return Err(Unreadable); }
            self.skip_multi_line_string(&rest[..3])?;
            return Ok(Value::Other);
        }
        match self.peek() {
            Some(b'"') => self.read_basic_string(reading == Reading::Wanted).map(Value::Str),
            Some(b'\'') => self.read_literal_string().map(Value::Str),
            Some(b'[') => self.read_array(reading),
            Some(b'{') if reading != Reading::Elsewhere => Err(Unreadable),
            Some(b'{') => { self.skip_inline_table()?; Ok(Value::Other) },
            Some(_) if reading == Reading::Wanted => Err(Unreadable),
            Some(_) => {
                let stops: &[u8] = if in_array { b"\n#,]" } else { b"\n#" };
                while self.peek().is_some_and(|byte| !stops.contains(&byte)) { self.at += 1; }
                Ok(Value::Other)
            },
            None => Err(Unreadable)
        }
    }

    fn read_basic_string(&mut self, wanted: bool) -> Result<String, Unreadable> {
        self.at += 1;
        let start = self.at;
        loop {
            match self.peek() {
                None | Some(b'\n') => return Err(Unreadable),
                Some(b'\\') if wanted => return Err(Unreadable),
                Some(b'\\') => self.at += 2,
                Some(b'"') => {
                    let content = self.text[start..self.at].to_owned();
                    self.at += 1;
                    return Ok(content);
                },
                Some(_) => self.at += 1
            }
        }
    }

    fn read_literal_string(&mut self) -> Result<String, Unreadable> {
        self.at += 1;
        let start = self.at;
        while self.peek().is_some_and(|byte| byte != b'\'' && byte != b'\n') { self.at += 1; }
        if self.peek() != Some(b'\'') { return Err(Unreadable); }
        let content = self.text[start..self.at].to_owned();
        self.at += 1;
        Ok(content)
    }

    // TOML lets one or two quotes stand just inside the closing three
    fn skip_multi_line_string(&mut self, delimiter: &str) -> Result<(), Unreadable> {
        let quote = delimiter.as_bytes()[0];
        self.at += 3;
        loop {
            let rest = &self.text[self.at..];
            let Some(found) = rest.find(delimiter) else { return Err(Unreadable) };
            let backslashes = rest[..found].bytes().rev().take_while(|byte| *byte == b'\\').count();
            if quote == b'"' && backslashes % 2 == 1 {
                self.at += found + 1;
                continue;
            }
            self.at += found + 3;
            let mut extra = 0;
            while extra < 2 && self.peek() == Some(quote) {
                self.at += 1;
                extra += 1;
            }
            return Ok(());
        }
    }

    fn read_array(&mut self, reading: Reading) -> Result<Value, Unreadable> {
        self.at += 1;
        let mut elements = Vec::new();
        let mut all_strings = true;
        loop {
            self.skip_blank_lines_and_comments();
            match self.peek() {
                None => return Err(Unreadable),
                Some(b']') => { self.at += 1; break; },
                Some(_) => ()
            }
            match self.read_value(reading, true)? {
                Value::Str(element) => elements.push(element),
                _ if reading == Reading::Wanted => return Err(Unreadable),
                _ => all_strings = false
            }
            self.skip_blank_lines_and_comments();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => (),
                _ => return Err(Unreadable)
            }
        }
        Ok(if all_strings { Value::Strings(elements) } else { Value::Other })
    }

    fn skip_inline_table(&mut self) -> Result<(), Unreadable> {
        self.at += 1;
        let mut depth = 1;
        while depth > 0 {
            match self.peek() {
                None | Some(b'\n') => return Err(Unreadable),
                Some(b'"') => { self.read_basic_string(false)?; continue; },
                Some(b'\'') => { self.read_literal_string()?; continue; },
                Some(b'{' | b'[') => depth += 1,
                Some(b'}' | b']') => depth -= 1,
                Some(_) => ()
            }
            self.at += 1;
        }
        Ok(())
    }
}

fn split_key_and_value(line: &str) -> Option<(&str, &str)> {
    let (name, value) = match line.split_once('=') {
        Some((name, value)) if !name.contains(':') => (name, value),
        _ => line.split_once(':')?
    };
    Some((name.trim(), value.trim()))
}

fn find_wanted_key(name: &str) -> Option<usize> {
    match name {
        TESTPATHS => Some(0),
        PYTHON_FILES => Some(1),
        _ => None
    }
}

// pytest hands an ini value to shlex, so a quote or a backslash in it would make the two readers
// disagree on where one entry ends
fn split_ini_value(value: &str) -> Option<Vec<String>> {
    let entries = value.split_ascii_whitespace().map(unquote_ini_entry).collect::<Option<Vec<_>>>()?;
    (!entries.is_empty()).then_some(entries)
}

// The one quoted shape shlex and a split on blanks agree on, a whole entry inside a single pair
fn unquote_ini_entry(entry: &str) -> Option<String> {
    let inner = match entry.as_bytes() {
        [quote @ (b'"' | b'\''), .., last] if last == quote && entry.len() >= 2 => &entry[1..entry.len() - 1],
        _ => entry
    };
    (!inner.contains(['"', '\'', '\\'])).then(|| inner.to_owned())
}

fn take_entries(value: Value, strings: StringValue) -> Option<Vec<String>> {
    match (value, strings) {
        (Value::Strings(entries), _) => Some(entries),
        (Value::Str(text), StringValue::Split) => split_ini_value(&text),
        _ => None
    }
}

fn resolve_testpaths(entries: &[String], directory: &str) -> Option<Vec<String>> {
    let mut scopes = Vec::with_capacity(entries.len());
    for entry in entries {
        let entry = entry.trim().trim_end_matches('/');
        let entry = if entry == "." { "" } else { entry.strip_prefix("./").unwrap_or(entry) };
        if entry.contains(['*', '?', '[', '\\', ':']) || entry.starts_with('/')
                || entry.split('/').any(|part| part.is_empty() || part == "." || part == "..") && !entry.is_empty() {
            return None;
        }
        scopes.push(if entry.is_empty() { directory.to_owned() } else { format!("{directory}/{entry}") });
    }
    (!scopes.is_empty()).then_some(scopes)
}

fn accept_python_files(entries: &[String]) -> Option<Vec<String>> {
    let names = entries.iter().map(|entry| entry.trim().to_owned()).collect::<Vec<_>>();
    if names.is_empty() || names.iter().any(|name| name.is_empty() || name.contains(['!', '{', '\\']) || name.ends_with('/')
            || is_a_path_pattern(name)) {
        return None;
    }
    PathPatternMatcher::compile(&names, TakingBack::Refused).ok()?;
    Some(names)
}

fn is_test_shaped(name: &str) -> bool {
    name.as_bytes().windows(4).any(|window| window.eq_ignore_ascii_case(b"test"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|entry| (*entry).to_owned()).collect()
    }

    fn declared(testpaths: Option<&[&str]>, python_files: Option<&[&str]>) -> Section {
        Section::Present(Declared { testpaths: testpaths.map(owned), python_files: python_files.map(owned) })
    }

    #[test]
    fn an_ini_section_is_found_by_its_exact_name_and_its_values_read_as_iniconfig_reads_them() {
        assert_eq!(declared(Some(&["tests", "integration"]), None),
                read_ini("[tox]\nenvlist = py\n[pytest]\ntestpaths = tests integration\n", "pytest"));
        assert_eq!(declared(Some(&["tests"]), Some(&["check_*.py"])),
                read_ini("[metadata]\nname = x\n[tool:pytest]\ntestpaths: tests\npython_files = check_*.py\n", "tool:pytest"));
        assert_eq!(Section::Absent, read_ini("[pytest]\ntestpaths = tests\n", "tool:pytest"), "a '[pytest]' in a setup.cfg was read");
        assert_eq!(Section::Absent, read_ini("[tox]\nenvlist = py\n", "pytest"));
        assert_eq!(Section::Present(Declared::default()), read_ini("[pytest]\n", "pytest"), "a bare section is pytest's all the same");
        assert_eq!(Section::Present(Declared::default()), read_ini("[pytest] ; the runner\naddopts = -q\n", "pytest"));
        assert_eq!(declared(Some(&["tests", "integration/"]), None),
                read_ini("[pytest]\naddopts = -q\ntestpaths =\n    tests\n    integration/\n", "pytest"), "a continued value was cut short");
        assert_eq!(declared(Some(&["tests"]), None),
                read_ini("[pytest]\ntestpaths =\n    tests\n    # integration\n", "pytest"), "an indented comment continued the value");
        assert_eq!(declared(None, Some(&["a.py"])),
                read_ini("[pytest]\naddopts =\n    -q\n    tests\npython_files = a.py\n", "pytest"), "a continuation of another key reached testpaths");
        assert_eq!(declared(Some(&["tests"]), None), read_ini("[pytest]\ntestpaths = tests\n[other]\npython_files = a.py\n", "pytest"),
                "a key under another section was read");
        assert_eq!(declared(Some(&["tests"]), Some(&["a.py"])), read_ini("[pytest]\r\ntestpaths = tests\r\npython_files = a.py\r\n", "pytest"));
        assert_eq!(declared(Some(&["tests", "#", "unit"]), None), read_ini("[pytest]\ntestpaths = tests # unit\n", "pytest"),
                "an inline '#' was stripped where iniconfig keeps it");
        assert_eq!(declared(None, None), read_ini("[pytest]\ntestpaths = 'my tests'\npython_files = a\\ b.py\n", "pytest"),
                "a value shlex would read differently was taken");
        assert_eq!(declared(Some(&["astropy_healpix", "docs"]), Some(&["test_*.py"])),
                read_ini("[tool:pytest]\ntestpaths = \"astropy_healpix\" \"docs\"\npython_files = 'test_*.py'\n", "tool:pytest"),
                "an entry inside one pair of quotes was refused where shlex reads it plainly");
        assert_eq!(declared(None, None), read_ini("[pytest]\ntestpaths = \"tests'\n", "pytest"));
        assert_eq!(declared(None, None), read_ini("[pytest]\ntestpaths =\n", "pytest"));
        assert_eq!(Section::Absent, read_ini("[pytest]\ntestpaths = tests\ntestpaths = other\n", "pytest"), "a key written twice was taken");
        assert_eq!(Section::Absent, read_ini("testpaths = tests\n[pytest]\n", "pytest"), "a value before any section was taken");
        assert_eq!(Section::Absent, read_ini("[pytest]\n    tests\n", "pytest"), "a continuation of nothing was taken");
        assert_eq!(Section::Absent, read_ini("[pytest]\nno equals sign here\n", "pytest"));
    }

    #[test]
    fn a_toml_table_is_found_by_its_header_and_ends_at_the_next_one() {
        let ini_options = &[(INI_OPTIONS_TABLE, StringValue::Split)][..];
        let pyproject = [(NATIVE_TABLE, StringValue::Refused), (INI_OPTIONS_TABLE, StringValue::Split)];
        assert_eq!(declared(Some(&["tests"]), Some(&["test_*.py", "test_*/__init__.py"])),
                read_toml("[tool.pytest.ini_options]\ntestpaths = \"tests\"\npython_files = [\n    \"test_*.py\",\n    'test_*/__init__.py', # packages\n]\n[tool.ruff]\nline-length = 100\n", ini_options));
        assert_eq!(declared(Some(&["tests", "integration"]), None),
                read_toml("[ tool . pytest.ini_options ]\ntestpaths = \"tests integration\"\n", ini_options), "blanks around the dots were not forgiven");
        assert_eq!(declared(Some(&["tests"]), None), read_toml("[project]\nname = \"x\"\nauthors = [{ name = \"A\", email = \"a@x.y\" }]\n\
                [tool.pytest.ini_options]\nfilterwarnings = [\n    \"error:usefixtures\\\\(\\\\) in .* without arguments:UserWarning\", # pytest\n    \
                'ignore:.*:DeprecationWarning',\n]\ntestpaths = \"tests\"\n[tool.black]\nextend-exclude = '''\n/(\n  [build]\n)/\n'''\n", ini_options),
                "the key after an array holding escapes, a literal string and a comment was not read, or the multi-line string after the table was not stepped over");
        assert_eq!(declared(Some(&["tests"]), None), read_toml("[tool.pytest.ini_options]\nminversion = 6.0\nxfail_strict = true\n\
                timeout = 30 # seconds\nmarkers = [\"slow: takes long\", 'net']\ntestpaths = [\"tests\"]\n", ini_options),
                "a key of another type in the pytest table made it unreadable");
        assert_eq!(Section::Absent, read_toml("[tool.ruff]\ntestpaths = \"tests\"\n", ini_options), "a key outside the table was read");
        assert_eq!(Section::Absent, read_toml("[project]\nname = \"x\"\n", ini_options));
        assert_eq!(declared(Some(&["tests"]), None), read_toml("[tool.pytest.ini_options]\naddopts = \"\"\"\n-q\n\"\"\"\ntestpaths = \"tests\"\n", ini_options),
                "a multi-line string of another key in the table was not stepped over");
        assert_eq!(declared(None, None), read_toml("[tool.pytest.ini_options]\ntestpaths = \"\"\"tests\"\"\"\n", ini_options));
        assert_eq!(declared(Some(&["aa"]), None), read_toml("[tool.x]\nnote = \"\"\"say \"hi\"\"\"\"\nescaped = \"\"\"a \\\"\"\" b\"\"\"\n\
                [tool.pytest.ini_options]\ntestpaths = \"aa\"\n", ini_options),
                "a multi-line string ending in quotes, or holding an escaped one, lost the reader its place");
        assert_eq!(declared(None, None), read_toml("[tool.x]\nnote = \"\"\"never closed\n[tool.pytest.ini_options]\ntestpaths = \"aa\"\n", ini_options),
                "a file the reader lost its place in read as holding no pytest table");
        assert_eq!(declared(None, None), read_toml("[tool.pytest.ini_options]\nmarkers.slow = \"x\"\ntestpaths = \"tests\"\n", ini_options),
                "a dotted key in the table did not make it unreadable");
        assert_eq!(declared(None, None), read_toml("[tool.pytest.ini_options]\ntestpaths = \"tests\"\ntestpaths = \"other\"\n", ini_options));
        assert_eq!(declared(None, None), read_toml("[tool.pytest.ini_options]\nx = { a = 1 }\ntestpaths = \"tests\"\n", ini_options));
        assert_eq!(declared(None, None), read_toml("[tool.pytest.ini_options]\ntestpaths = \"tests\" trailing\n", ini_options));
        assert_eq!(declared(Some(&["tests"]), None), read_toml("[tool.pytest.ini_options]\ntestpaths = \"tests\" # unit\n", ini_options));
        assert_eq!(declared(None, None), read_toml("[tool.pytest.ini_options]\ntestpaths = \"te\\\"sts\"\npython_files = [\"a.py\"]\n", ini_options),
                "an escape in a wanted string was taken");
        assert_eq!(declared(None, None), read_toml("[tool.pytest.ini_options]\ntestpaths = [\"tests\", 1]\npython_files = [\"a.py\"]\n", ini_options));
        assert_eq!(declared(Some(&[]), None), read_toml("[tool.pytest.ini_options]\ntestpaths = []\n", ini_options));

        assert_eq!(declared(Some(&["tests"]), None), read_toml("[tool.pytest]\ntestpaths = [\"tests\"]\n", &pyproject));
        assert_eq!(declared(None, Some(&["a.py"])), read_toml("[tool.pytest]\ntestpaths = \"tests\"\npython_files = [\"a.py\"]\n", &pyproject),
                "a string in the native table was split where pytest 9 refuses it");
        assert_eq!(declared(None, None), read_toml("[tool.pytest]\ntestpaths = [\"tests\"]\n[tool.pytest.ini_options]\ntestpaths = \"tests\"\n", &pyproject),
                "both tables with content read, where pytest refuses the file");
        assert_eq!(declared(Some(&["tests"]), None), read_toml("[tool.pytest]\n[tool.pytest.ini_options]\ntestpaths = \"tests\"\n", &pyproject),
                "an empty native table counted as content");
        assert_eq!(declared(None, None), read_toml("[tool.pytest.markers]\nslow = \"x\"\n", &pyproject), "a child table of the native one was not pytest's");
        assert_eq!(declared(Some(&["tests"]), None), read_toml("[pytest]\ntestpaths = [\"tests\"]\n", &[("pytest", StringValue::Refused)]));
        assert_eq!(declared(None, None), read_toml("[pytest]\ntestpaths = [\"tests\"\n", &[("pytest", StringValue::Refused)]), "an array left open read a value");
        assert_eq!(declared(None, None), read_toml("[[pytest]]\ntestpaths = [\"tests\"]\n", &[("pytest", StringValue::Refused)]));
    }

    #[test]
    fn testpaths_resolve_below_the_file_and_python_files_are_names_the_matcher_takes() {
        let resolved = |entries: &[&str]| resolve_testpaths(&owned(entries), "D:/proj");
        assert_eq!(Some(owned(&["D:/proj/tests", "D:/proj/integration", "D:/proj/pkg/tests"])), resolved(&["tests/", "./integration", "pkg/tests"]));
        assert_eq!(Some(owned(&["D:/proj", "D:/proj"])), resolved(&[".", ""]));
        assert_eq!(None, resolved(&["tests", "../other"]));
        assert_eq!(None, resolved(&["/tests"]));
        assert_eq!(None, resolved(&["D:/tests"]));
        assert_eq!(None, resolved(&["tests/*"]));
        assert_eq!(None, resolved(&["a//b"]));
        assert_eq!(None, resolved(&[]));

        let accepted = |entries: &[&str]| accept_python_files(&owned(entries));
        assert_eq!(Some(owned(&["test_*.py", "test_*/__init__.py", "__init__.py"])), accepted(&["test_*.py", "test_*/__init__.py", "__init__.py"]));
        assert_eq!(None, accepted(&["./tests/test_*.py"]), "a place was taken for a name");
        assert_eq!(None, accepted(&["test_*.py", "!helper.py"]));
        assert_eq!(None, accepted(&["test_{a,b}.py"]));
        assert_eq!(None, accepted(&["tests/"]));
        assert_eq!(None, accepted(&["test_[.py"]), "a pattern the matcher refuses was accepted");
        assert_eq!(None, accepted(&[]));
    }

    #[test]
    fn under_a_conftest_py_only_a_declared_name_saying_test_applies() {
        let configuration = |python_files: &[&str]| PytestConfiguration { file: "pytest.ini", testpaths: None, python_files: Some(owned(python_files)) };
        assert_eq!(Some(owned(&["test_*/__init__.py"])), configuration(&["__init__.py", "test_*/__init__.py"]).collect_test_shaped_names());
        assert_eq!(None, configuration(&["__init__.py", "*.py"]).collect_test_shaped_names());
        assert!(["test_*/__init__.py", "*_tests.py", "Test*.py"].iter().all(|name| is_test_shaped(name)));
        assert!(!["__init__.py", "*.py", "tes_t.py"].iter().any(|name| is_test_shaped(name)));
    }

    #[test]
    fn the_first_file_present_in_pytests_order_decides_even_when_it_holds_nothing() {
        let root = std::env::temp_dir().join("mezura-pytest-configuration-precedence");
        let _ = fs::remove_dir_all(&root);
        for (file, contents) in [("ini-over-toml/pytest.ini", "[pytest]\n"), ("ini-over-toml/pyproject.toml", "[tool.pytest.ini_options]\ntestpaths = \"tests\"\n"),
                ("tox-without-section/tox.ini", "[tox]\nenvlist = py\n"), ("tox-without-section/setup.cfg", "[tool:pytest]\ntestpaths = tests\n"),
                ("toml-over-ini/pytest.toml", "[pytest]\ntestpaths = [\"a\"]\n"), ("toml-over-ini/pytest.ini", "[pytest]\ntestpaths = b\n"),
                ("pyproject-without-table/pyproject.toml", "[project]\nname = \"x\"\n"), ("pyproject-without-table/tox.ini", "[pytest]\ntestpaths = tests\n"),
                ("bom/pytest.ini", "\u{feff}[pytest]\ntestpaths = tests\n")] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
        }
        let present = |directory: &str| {
            let directory = root.join(directory);
            let names = fs::read_dir(&directory).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
            read_pytest_configuration(&directory, move |file| names.iter().any(|name| name == file))
        };
        let scope = |directory: &str, below: &str| vec![format!("{}/{below}", spell_out(&root.join(directory)))];

        let found = present("ini-over-toml").unwrap();
        assert_eq!(("pytest.ini", None, None), (found.file, found.testpaths, found.python_files), "a bare pytest.ini lost to the pyproject.toml beside it");
        let found = present("tox-without-section").unwrap();
        assert_eq!(("setup.cfg", Some(scope("tox-without-section", "tests"))), (found.file, found.testpaths), "a tox.ini without [pytest] decided");
        let found = present("toml-over-ini").unwrap();
        assert_eq!(("pytest.toml", Some(scope("toml-over-ini", "a"))), (found.file, found.testpaths));
        let found = present("pyproject-without-table").unwrap();
        assert_eq!(("tox.ini", Some(scope("pyproject-without-table", "tests"))), (found.file, found.testpaths), "a pyproject.toml without a pytest table decided");
        let found = present("bom").unwrap();
        assert_eq!(Some(scope("bom", "tests")), found.testpaths, "a byte order mark hid the section");
        assert!(read_pytest_configuration(&root.join("bom"), |_| false).is_none(), "a file the listing does not hold was read");

        fs::remove_dir_all(&root).unwrap();
    }
}
