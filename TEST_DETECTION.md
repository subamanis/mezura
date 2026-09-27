# Test detection

mezura tells the test code of a language from the rest of it. Every language that holds any gets a
`tests` row under it in the report, the total gets one that adds them up, and the same figures are
in the JSON document, in a `--diff` and in `--explain`. This page says what is counted as test code
without being asked and why each of those is certain, how the rest is declared with `--tests`, and
for each language what is found, what is refused and what is missed.

The rule behind all of it: **a production line counted as a test hurts more than a test line left
uncounted.** So on its own mezura takes only what the code itself states or what the build tool
defines. Everything that is a convention, a framework's habit or a team's taste is declared by
whoever runs the count, and stays declared in the project's own configuration.

- [What is found without asking](#what-is-found-without-asking)
- [Declaring the rest with --tests](#declaring-the-rest-with---tests)
- [Language by language](#language-by-language)
- [What is refused for every language](#what-is-refused-for-every-language)
- [Where the count shows](#where-the-count-shows)


## What is found without asking

Three kinds of thing, and nothing else.

### 1. Markers in the code

Rust, D and Zig write tests inside the source file, behind a marker the compiler itself reads:
`#[test]` and `#[cfg(test)]` in Rust, `unittest` in D, `test "name" {` in Zig. mezura finds the
marker and counts from it to the end of the item under it, the matching `}` or the `;`, whichever
the item ends with. A marker inside a string or a comment is text. The markers are declared in the
language file, under `Tests`, and the [language files guide](LANGUAGE_FILES_GUIDE.md) says how.

This is certain because the compiler reads the same marker: what `#[cfg(test)]` covers is what
`cargo build` leaves out.

### 2. Names the toolchain defines

`*_test.go` and Perl's `*.t`. The go tool compiles a `_test.go` file for `go test` and for nothing
else, and a `.t` file is a test script and nothing else: `prove` runs `t/*.t`, and a distribution
installs only its `.pm` and `.pl`. Both are whole files, every line of them is test code.

A name a framework or a team favours, `test_*.py`, `*Test.java`, `*.spec.ts`, `*_spec.rb`, is a
convention, and a production file can wear it: `NodeTest.java` in the JDK is the XPath node test,
`ZTest.cs` in Unity is the depth test, `hit_test.rs` in a geometry crate is hit testing, and the
polyfill of `RegExp.test` is called `es.regexp.test.js`. Those names are declared with `--tests`,
where they hold for the project that declares them.

### 3. The test directory of a build tool, beside its build file

A directory is test code whole when the directory holding it holds the build file of a tool that
compiles that directory for tests alone:

| build file | test directory beside it |
|---|---|
| `Cargo.toml` | `tests/` |
| `pom.xml`, `build.gradle`, `build.gradle.kts`, `settings.gradle`, `settings.gradle.kts`, `build.sbt` | `src/test/` |
| `Package.swift` | `Tests/` |
| `mix.exs` | `test/` |
| `pubspec.yaml` | `test/`, `integration_test/` |
| `Project.toml`, `JuliaProject.toml` | `test/` |
| `Makefile.PL`, `Build.PL`, `dist.ini` | `t/` |
| `project.clj` | `test/` |
| `rebar.config` | `test/` |
| `elm.json` | `tests/` |
| `DESCRIPTION` together with `NAMESPACE` | `tests/` |

Only the build file's presence counts, its contents are never read. Only the tool's default
directory is in the table: a variant such as Gradle's `src/integrationTest`, Android's
`src/androidTest` or a `testSourceDirectory` that Maven was pointed elsewhere goes through
`--tests`.

Under a `settings.gradle`, a `settings.gradle.kts` or a `build.sbt`, every `src/test` below it is
test code, whether or not the subproject beside it carries a build file of its own, since that is
how sbt and Gradle multi-project builds lay their subprojects out: `core/src/test`, and sbt's
cross-built `core/jvm/src/test`. A `pom.xml` says nothing about the folders around it, since a
Maven module always carries its own.

The names of a test directory and of a build file are read the way the filesystem reads them. On
Windows and macOS a `Tests/` beside a `Cargo.toml` is cargo's integration tests, so it counts; on
Linux it is a folder cargo ignores, so it is production.

`package.json`, `pyproject.toml`, `setup.py`, `composer.json`, a `Gemfile`, a `.csproj`,
`CMakeLists.txt` and `build.zig` define no test directory, so JavaScript, TypeScript, Python, PHP,
Ruby, C#, C, C++ and Zig detect nothing by path until it is declared. And a folder named `test`,
`tests` or `__tests__` means nothing on its own, anywhere: without the build file beside it, it is a
folder like any other.

### Where the count starts from changes nothing

The build file travels with the tree, so the answer is the same however the project is reached:
counting it from its root, from inside its test directory (`mezura ./proj/tests`) or from its
sources (`mezura ./core/src`), naming one file by hand, `--explain` on that file, and the checkout
of a `--diff` revision all agree. A folder above the project never makes it tests: `D:/tests/app`
is tests only if `D:/Cargo.toml` exists.


## Declaring the rest with `--tests`

`--tests` takes glob patterns, and what a pattern names is test code whole, files and folders
alike. A `!` in front takes back what an earlier pattern or a build file gave, and the last pattern
that matches decides:

```bash
mezura ./ --tests "spec/,!spec/fixtures/"
mezura ./ --tests "test_*.py,*_test.py,conftest.py"
mezura ./ --tests "!vendor/"
```

The rules in short, with the whole of them under `--tests` in [COMMANDS.md](COMMANDS.md):

- A pattern with no `./`, no `../` and no drive or root is a **name**, matched at any depth under
  the folder that holds the target, so it sees the target's own name and no folder above it.
  Names are matched with their case.
- A pattern starting with `./` or `../`, or written as a full path, is a **place**, read from the
  directory the command is typed in, and it names that place and everything under it.
- A trailing `/` means a folder only, so `tests/` leaves a script named `tests` alone. `x/**`
  means the folder `x`, and `**` alone a whole target.
- A `!` takes back the folder or file it names, from a build file or from an earlier pattern.
  What a marker inside a file says stays: a `#[cfg(test)]` under `!vendor/` is still test code.
- A pattern that names nothing on disk, and a name with a slash that matched nothing, are reported.
- In PowerShell, quote the whole list or escape each comma with a backtick.

Patterns saved in a project's own configuration, with `--save-local` or under `===> tests` in
`.mezura/config.txt`, are read from the project directory, so they say the same thing from
wherever inside the project the command is typed, and they travel with the repository.

### Ready patterns

Each line is the default of the tool named beside it, so it holds wherever the project kept that
default.

| for | `--tests` |
|---|---|
| pytest | `test_*.py,*_test.py,conftest.py` |
| unittest | `test*.py` |
| jest and vitest | `__tests__/,*.test.js,*.test.jsx,*.test.ts,*.test.tsx,*.spec.js,*.spec.jsx,*.spec.ts,*.spec.tsx` |
| mocha | `test/` |
| RSpec | `spec/` |
| minitest | `test/` |
| PHPUnit | `tests/,*Test.php` |
| .NET test projects | `*Tests/,*.Test/` |
| Go's test inputs | `testdata/` |
| Gradle variants and Kotlin Multiplatform | `src/*Test/,src/testFixtures/` |
| A JVM project with none of the build files above | `src/test/` |
| Perl's author tests | `xt/` |
| Odin | `*_test.odin` |
| C, C++ and Zig, by the project's taste | `test/` or `tests/` |

Every `test/` and `tests/` above is a folder anywhere under the target, a test folder of a nested
project included. Where a project has a production folder of that name, write the place,
`./tests/`, which names that one folder.


## Language by language

### Rust

**Found.** Attributes, read as the compiler reads them:

- `#[test]`, `#[bench]`, and any attribute whose name holds `test`: `#[tokio::test]`, `#[rstest]`,
  `#[wasm_bindgen_test]`, `#[test_case]`, `#[std::prelude::v1::test]`. Measured over the 11,921
  Rust files of a cargo registry: of 30,083 attributes with `test` in the name, one was no test
  attribute, `#[proptest_config]`, and it sat over a proptest anyway.
- `#[cfg(...)]` whose predicate holds `test` outside a `not(...)`: `#[cfg(test)]`,
  `#[cfg(any(test, feature = "x"))]`. `#[cfg(not(test))]` is production, and `#[cfg(testlib)]` is
  a cfg of its own.
- `#[cfg_attr(predicate, attribute)]` only when the predicate says test and the attribute applied
  holds `test` in its name: `#[cfg_attr(test, test)]` is a marker, while
  `#[cfg_attr(test, derive(Debug))]` and `#![cfg_attr(test, allow(deref_nullptr))]` are settings.
- `#![cfg(test)]`: the rest of its scope, which is the whole file at the top of one and the module
  it sits in.

From the marker, further `#[...]` on the item are skipped, and the test code runs to the `}`
matching the first `{`, or to the first `;`, whichever comes first, with brackets counted so that
`[u8; 3]` ends nothing. `let`, `use`, `const`, `static`, `type` and `extern` end at their `;` and
their braces are ignored, so `#[cfg(test)] let x = if a { 1 } else { 2 };` is one statement. An
`if` carries on through its `else` arms. Whatever sits inside test code opens nothing of its own,
so the `#[test]` functions of a `#[cfg(test)] mod tests { }` are counted once.

`tests/` beside a `Cargo.toml` is test code whole, `use` lines, helpers, `tests/common/` and
fixtures included. A `tests/` at the root of a virtual workspace, which cargo never builds, counts
too, since what sits there in practice is test crates and test data; `--tests "!tests/"` says
otherwise. Everywhere else, `examples/`, `benches/` and `src/bin/` included, the markers alone
decide, and a `tests` folder under `src/` is a folder.

**Missed**, each with what mezura answers today:

- `#[cfg(test)] mod tests;`, whose body is another file. That file carries no marker, so only what
  the markers inside it catch is test code, its `#[test]` functions, while its `use` lines, its
  helpers and any `mod` it declares in turn are production. Declare the file meanwhile,
  `--tests "src/tests.rs"` or `--tests "*_tests.rs"`; following the declaration to the file is
  planned.
- A marker on a struct field, an enum variant, a match arm or a parameter counts to the `}` of the
  block around it, so the siblings after it come along:
  ```rust
  pub enum Shape {
      Circle,
      #[cfg(test)]
      Probe,
      Square,
  }
  ```
  Lines 3 to 6 are test code, where the variant alone is lines 3 to 4.
- An inner `#![cfg(test)]` inside a module counts from its own line, so the `mod checks {` above
  it stays out.
- A const generic in a signature, `fn sized() -> Wrapper<{ SIZE }> {`, ends the item at the `}` of
  `{ SIZE }`, so the body under it is production.

**Refused.** Doc tests, since a `///` fenced block is a comment. Tests a macro writes,
`make_test!(a, b, c);` being one line of code that only a compiler can expand. Tests written at
build time and pulled in with `include!`. `#[path = "..."]` on a module declaration.

### D

**Found.** The word `unittest` in code, as `unittest { }`, `@safe unittest { }` and
`version(unittest) { }`, to the matching `}`. The `else` of a `version(unittest)` is production.
`version(unittest):` with a colon covers the rest of its scope: the struct or block it sits in, up
to its `}`, or the rest of the file at module level. `unittest` inside a comment, inside a string,
or as part of a longer word like `myunittest` is text.

**Missed.** An attribute on the line above the marker stays out: `@safe` on a line of its own and
`unittest` on the next counts from `unittest`.

### Zig

**Found.** `test "name" { }` and `test { }`, to the matching `}`. `test` in a comment or a string
is text, and `test_helper` is another word. `build.zig` defines no test directory, so a `test/`
folder is declared.

### Go

**Found.** `*_test.go` anywhere, whole: the tests, the examples, the benchmarks and the helpers
beside them. `testdata/` holds the inputs of tests and stays out on its own; `--tests testdata/`
counts it.

### Perl

**Found.** `*.t` anywhere, whole, and `t/` beside a `Makefile.PL`, a `Build.PL` or a `dist.ini`.
`xt/` is declared.

### Java, Kotlin, Scala and Groovy

**Found.** `src/test/` beside a `pom.xml`, `build.gradle`, `build.gradle.kts`, `settings.gradle`,
`settings.gradle.kts` or `build.sbt`, and every `src/test/` below a `settings.gradle`,
`settings.gradle.kts` or `build.sbt`. JUnit, TestNG, Spock, ScalaTest and specs2 all live there, so
no framework is named. `mezura ./core/src` finds the `core/pom.xml` above it and counts `src/test`
the same.

**Missed on purpose.** A test class under `src/main`, or a `*Test.java` anywhere outside
`src/test`. A `test/` beside a `pom.xml` with no `src` above it, since Maven compiles nothing from
it. A Gradle source set other than `test`, and a `testSourceDirectory` Maven was pointed elsewhere,
are declared, with `!src/test/` where the default was moved away from. A file holding `@Test`
outside the test directory is planned, as a marker that makes the whole file tests.

### Swift

**Found.** `Tests/` beside a `Package.swift`. A `*Tests.swift` under `Sources/`, or an
`XCTestCase` outside `Tests/`, is production. An Xcode project without a `Package.swift` defines
no test directory here, so its test target's folder is declared.

### Elixir

**Found.** `test/` beside a `mix.exs`. A `_test.exs` outside it is production; `--tests "*_test.exs"`
if wanted.

### Dart and Flutter

**Found.** `test/` and `integration_test/` beside a `pubspec.yaml`. A `_test.dart` under `lib/` is
production.

### Julia

**Found.** `test/` beside a `Project.toml` or a `JuliaProject.toml`.

### Clojure

**Found.** `test/` beside a `project.clj`. A `deps.edn` names its test paths in an alias, so under
one the folder is declared.

### Erlang

**Found.** `test/` beside a `rebar.config`, which is where Common Test suites and EUnit's `_tests`
modules live. `-ifdef(TEST).` inside a module is ordinary code: a marker whose test code ends at a
literal `-endif.` is a kind no shipped language has yet.

### Elm

**Found.** `tests/` beside an `elm.json`.

### R

**Found.** `tests/` beside a `DESCRIPTION` and a `NAMESPACE`, both, since an Octave package carries
a `DESCRIPTION` too. testthat's `tests/testthat/` is under it.

### JavaScript, TypeScript, Python, PHP, Ruby, C#, C and C++

Nothing by path: their build files define no test directory. Everything is declared, with the
ready patterns above. vitest's in-source tests, `if (import.meta.vitest) { }`, are code. Python's
doctests in a docstring are a string. C++'s `TEST_CASE` and `TEST_F` are a framework's macros, and
no framework is named.

### OCaml

`let%test` and `let%expect_test` have no closer: the binding ends where the next one begins, and
that is a layout rule the parser has no business guessing. Refused.


## What is refused for every language

- **Doc tests.** A `///` fenced block in Rust, a `>>>` in a Python docstring, an `iex>` in Elixir:
  every one of those lines is a comment, and a line is one thing.
- **Tests a macro or a build script generates.** What is counted is what is written.
- **A framework's names compiled into the program.** `describe`, `it`, `pytest`, `TEST_CASE`,
  `@Test`: every one is a claim about somebody's project, and the failure is silent miscounting.
  What is declared lives in the language file or in `--tests`, visible and editable.
- **Test code inside a nested section.** vitest in the `<script>` of a `.vue` is JavaScript inside
  Vue, and stays so.
- **A test directory by its name alone.** `test`, `tests` and `__tests__` are folders; the build
  file beside one is what makes it tests. A project checked out under `D:/tests/`, or a home
  folder called `test`, changes nothing.


## Where the count shows

- **The report.** A `tests` row under each language that holds any, its `Files %` and `Lines %`
  being its share of the language, and one under the total. `--tests-breakdown split` draws the
  rest of the language as a `production` row above it; a language with sections of other
  languages already draws `X itself` beside them, and `tests` joins those. `--hide tests` hides the
  rows and turns the detection off, patterns included, which also makes the run faster.
- **`--by-file`** draws nothing under a file row; the figure per file is in the document.
- **The JSON document.** `scope.tests_detected`, `languages[].tests` with the figures a nested
  language carries plus `whole_files`, the files that are test code from their first line to their
  last, then `total.tests`, `modules[].total.tests` and `by_file[].tests`. A comparison carries the
  compared pair under each of those.
- **`--diff`** against a document written without detection, or by an earlier version, runs
  without it, so no row appears on either side, and says so.
- **`--explain`.** Every line of test code is marked `test code`, the totals say how many, and a
  file that is test code whole names the rule that decided it: the test directory with its build
  file, the name, or the pattern. The document carries `in_test` on those lines and the rule under
  `test_file`.
