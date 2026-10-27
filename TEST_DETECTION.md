# Test detection

mezura tells the test code of a language from the rest of it. Every language that holds any gets a
`tests` row under it in the report, the total gets one that adds them up, and the same figures are
in the JSON document, in a `--diff` and in `--explain`. This page says what is counted as test code
without being asked and why each of those is certain, what a test tool's own file turns on, how the
rest is declared with `--tests`, and for each language what is found, what is refused and what is
missed.

The rule behind all of it: **a production line counted as a test hurts more than a test line left
uncounted.** So on its own mezura takes what the code itself states, what the build tool defines,
and the default names of a test tool whose own file sits in the folder. Everything else, a
framework's habit or a team's taste, is declared by whoever runs the count, and stays declared in
the project's own configuration.

- [What is found without asking](#what-is-found-without-asking)
- [What a test tool's file turns on](#what-a-test-tools-file-turns-on)
- [Declaring the rest with --tests](#declaring-the-rest-with---tests)
- [Language by language](#language-by-language)
- [What is refused for every language](#what-is-refused-for-every-language)
- [Where the count shows](#where-the-count-shows)


## What is found without asking

Three kinds of thing. A fourth, the test names a tool's own file turns on, has the
[section after this one](#what-a-test-tools-file-turns-on), since it rests on the tool being present
where these three rest on the compiler or the build tool.

### 1. Markers in the code

Rust, D and Zig write tests inside the source file, behind a marker the compiler itself reads:
`#[test]` and `#[cfg(test)]` in Rust, `unittest` in D, `test "name" {` in Zig. mezura finds the
marker and counts from it to the end of the item under it, the matching `}` or the `;`, whichever
the item ends with. A marker inside a string or a comment is text. The markers are declared in the
language file, under `Tests`, and the [language files guide](LANGUAGE_FILES_GUIDE.md) says how.

This is certain because the compiler reads the same marker: what `#[cfg(test)]` covers is what
`cargo build` leaves out.

Java, Kotlin and C# write tests as ordinary classes, and the test runner collects them by an
annotation or an attribute: `@Test`, `@BeforeEach`, `[Fact]`, `[Test]`, `[TestFixture]`. One of
those in the code of a file makes the whole file test code, since the class around a test method,
its fields, its setup and its helpers, is the test class. The list is explicit and lives in the
language file under `FILE MARKERS`: an annotation earns its place when a test framework defines it
and no production library writes the same name, so `@VisibleForTesting`, `@TestOnly` and `[Category]`
are not on it. The marker has to stand as a word of its own, so `@TestOnly` is not `@Test` and
`[TestFixture]` is not `[Test]`, and one inside a string, a comment or a text block is text.

This is certain because the runner collects tests by the same name and nothing else uses it: a
`@Test` method can only sit in a test class.

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
compiles that directory for tests and for nothing else:

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
| `go.mod` | every `testdata/` below it, at any depth |

Only the build file's presence counts, its contents are never read. Only the tool's default
directory is in the table. Gradle's other source sets, `src/integrationTest` and Android's
`src/androidTest`, are a preset its build file turns on, in the next section, and a
`testSourceDirectory` that Maven was pointed elsewhere goes through `--tests`.

`testdata` is the one name taken below its build file at any depth: `go help packages` says the go
tool ignores a directory of that name, and what sits in one is the input of tests. With no `go.mod`
above it, a `testdata` is a folder.

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
Ruby, C#, C, C++ and Zig get nothing from a build file. What they get by path is what a test tool's
own file turns on, in the next section, and what `--tests` declares. And a folder named `test`,
`tests` or `__tests__` means nothing on its own, anywhere: without a build file beside it or a test
tool's file above it, it is a folder like any other.

### Where the count starts from changes nothing

The build file and the test tool's file travel with the tree, so the answer is the same however the
project is reached:
counting it from its root, from inside its test directory (`mezura ./proj/tests`) or from its
sources (`mezura ./core/src`), naming one file by hand, `--explain` on that file, and the checkout
of a `--diff` revision all agree. A folder above the project never makes it tests: `D:/tests/app`
is tests only if `D:/Cargo.toml` exists.

The one exception is a file another file declares as a module under a marker. The declaring file
has to be in the same count, so counting a folder below it, naming the declared file by itself, or
leaving the declarer out with `--exclude`, a `.gitignore` or as generated leaves the declared file
production, while `--explain` reads the declaring files from disk, whatever `--exclude` or a
`.gitignore` says of them, and answers as a count of the whole project does.


## What a test tool's file turns on

A test tool's own file says the tool is in use, and where it is in use its default names for test
files hold. So a `jest.config.ts` turns jest's names on under the folder holding it: every
`__tests__/` and every `*.test.ts` below that folder is test code, and the same name outside it is
a file like any other. The file is never opened. Its presence in the folder's listing is the whole
of what is read, with pytest's configuration the one exception below. These are the presets, and
`--show-presets` prints them.

The file has to belong to one tool. `jest.config.ts`, `.rspec`, `phpunit.xml` and `conftest.py`
each exist for one test tool and nothing else, where `package.json`, `pyproject.toml`, a `Gemfile`,
`composer.json` and a `.csproj` name a language and no tool, so those turn nothing on. Two rows are
narrower than the tool's documentation would make them:

- **pytest is turned on by `conftest.py`**, the file that sits where the tests are, and by a
  configuration file declaring `testpaths`, under the folders it names and nowhere else. A
  `pytest.ini` that declares no `testpaths` turns nothing on: aiohttp keeps one at its root, beside
  `aiohttp/test_utils.py`, the library's public testing utilities, which pytest never collects and
  a preset over the whole root would count as tests.
- **RSpec is turned on by `.rspec` with a `Gemfile` beside it.** RSpec also reads a `~/.rspec` in
  the home directory, and that file by itself would make every `spec/` under the home folder test
  code, Ruby or not. Nobody keeps a `Gemfile` in their home folder.

The names apply under the folder holding the file and nowhere else, so a monorepo counted from its
root gets jest's names under `site/frontend/jest.config.ts` and PHPUnit's under
`site/backend/phpunit.xml`, each in its own folder. A file deeper down adds its names under its own
folder and cancels nothing above it. A file above the target counts too: counting `./frontend/src`
finds the `frontend/jest.config.ts` above it, the way a `Cargo.toml` above the target is found.

| preset | turned on by | patterns | the default it is |
|---|---|---|---|
| `@jest` | `jest.config.*` | `__tests__/`, `*.test.js`, `*.test.jsx`, `*.test.ts`, `*.test.tsx`, `*.test.mjs`, `*.test.cjs`, `*.test.mts`, `*.test.cts`, `*.spec.js`, `*.spec.jsx`, `*.spec.ts`, `*.spec.tsx`, `*.spec.mjs`, `*.spec.cjs`, `*.spec.mts`, `*.spec.cts` | jest's `testMatch`, without its bare `test.js` and `spec.js` |
| `@vitest` | `vitest.config.*`, `vitest.workspace.*`, and `vite.config.*`, which vitest reads when it has no file of its own | `*.test.js`, `*.test.jsx`, `*.test.ts`, `*.test.tsx`, `*.test.mjs`, `*.test.cjs`, `*.test.mts`, `*.test.cts`, `*.spec.js`, `*.spec.jsx`, `*.spec.ts`, `*.spec.tsx`, `*.spec.mjs`, `*.spec.cjs`, `*.spec.mts`, `*.spec.cts` | vitest's `include`, which has no `__tests__/` |
| `@mocha` | `.mocharc.*` | `test/` | mocha's `spec`, which takes the files at the top of `test/` and nothing below them; the preset takes the folder whole |
| `@pytest` | `conftest.py`, or `pytest.toml`, `.pytest.toml`, `pytest.ini`, `.pytest.ini`, `pyproject.toml`, `tox.ini` or `setup.cfg` declaring `testpaths` | `test_*.py`, `*_test.py`, `conftest.py` | pytest's `python_files` |
| `@unittest` | no file, typed only | `test*.py` | `unittest discover`, which takes `testutil.py` in CPython |
| `@rspec` | `.rspec` with a `Gemfile` beside it | `spec/` | RSpec's `default_path` |
| `@minitest` | no file, typed only | `test/` | the Rails layout |
| `@phpunit` | `phpunit.xml`, `phpunit.xml.dist`, `phpunit.dist.xml` | `tests/`, `Tests/`, `test/` | the folders PHP projects name in that file, since PHPUnit has no default |
| `@dotnet` | no file, typed only | `*Tests/`, `*.Test/` | the .NET naming, which takes `PerformanceTests/` in Unity |
| `@gradle` | `build.gradle`, `build.gradle.kts`, `settings.gradle`, `settings.gradle.kts` | `src/*Test/`, `src/testFixtures/` | Gradle's other test source sets and java-test-fixtures |
| `@jvm` | no file, typed only | `src/test/` | the Maven layout with no build file beside it |
| `@xt` | `Makefile.PL`, `Build.PL`, `dist.ini` | `xt/` | Dist::Zilla's author, release and smoke tests |
| `@odin` | no file, typed only | `*_test.odin` | a convention the compiler gives no meaning |

A preset with no file is typed, `--tests "@unittest"`, since its names catch production code
somewhere: `unittest discover` takes `testutil.py` in CPython, `*Tests/` takes Unity's
`PerformanceTests/`, and `test/` and `src/test/` are anybody's folders. The bare names jest's
`testMatch` also includes, `test.js` and `spec.ts`, are left out of its row: `lib/test.js` and
`lib/reporters/spec.js` are production in mocha, and `runners/test.ts` in vitest. Typed, a preset's
`test/` is a folder anywhere under the target, a test folder of a nested project included; where a
project has a production folder of that name, write the place, `./test/`, which names that one
folder.

**Pytest's configuration is the one file read.** The first of `pytest.toml`, `.pytest.toml`,
`pytest.ini`, `.pytest.ini`, `pyproject.toml`, `tox.ini` and `setup.cfg` present in a folder, in
pytest's own order of precedence, is read for `testpaths` and `python_files`. A declared `testpaths`
turns the preset on under those folders and nowhere else, and a declared `python_files` replaces
the names where it is declared: a declared name takes `.py` files and never a folder, as pytest
takes it, and a declared name without `test` in it applies under `testpaths` and nowhere else, so
a `python_files = __init__.py` beside a root `conftest.py` takes no package of the project. A
`conftest.py` inside the `testpaths` applies the shipped names whatever the configuration above it
says. A `*` in a `python_files` name stops at a `/`, where pytest's own matching crosses it, so
mezura takes less than pytest there and never more. Every other tool's configuration is unread, and
what that misses is a suite the configuration selects: babel's `test/` under its jest
configuration, svelte's `tests/` under its vitest one, a jest `rootDir` pointing elsewhere.

**What applied is said under the report:**

```
Test presets applied:
  @jest      under ./frontend (jest.config.ts)
  @pytest    under ./tools, ./scripts and 3 more (conftest.py)
To disable a test preset: '--tests "!@<name>"'. To disable test detection: '--hide tests'.
```

One row per preset, and only a preset that decided a file: a `jest.config.js` with no JavaScript
test under it prints nothing. The list goes quiet once the project's own configuration has a
`tests` block, saved with `--save-local` or written under `===> tests` in `.mezura/config.txt`,
since the project has then said what its tests are; the presets keep applying, and
`scope.test_presets` in the JSON document and `--explain` on a file still name them. The other way
round, a `package.json`, `pyproject.toml`, `Gemfile` or `composer.json` in the target's own folder
that names a test tool, with no preset matched and no `tests` row for that language, gets one line
naming the preset to type: `jest tests are declared with '--tests "@jest"'`.

**A preset by name.** `@name` inside `--tests` stands for the preset's patterns, everywhere under
the target, whether or not the tool's file is there: `--tests "@pytest"` is
`test_*.py,*_test.py,conftest.py`, and `--tests "@pytest,!tests/data/"` refines it, the last match
winning as always. `!@jest` is every pattern of the preset with a `!` in front, so it takes the
preset back whole, typed or turned on by a file. A list that takes back every pattern of a preset
turns that preset off whole, which is what `!@rspec` writes, so a hand-typed `!spec/` turns RSpec
off as well, and `!test/` mocha. A typed pattern always comes after what a file turned on, so
`--tests "!tests/data/"` corrects the pytest preset without losing it. A folder whose own name
starts with `@` is written as a place, `./@types/`, or after `**/`. A name no preset has refuses the
run and lists the known ones. In PowerShell a preset goes in quotation marks, since the shell reads
a bare word starting with `@` as a variable and passes nothing.

`--save` and `--save-local` write the patterns out with every `@` expanded, so a configuration
committed with the project depends on nothing of the machine that wrote it, and `--explain` names
the pattern that caught a file.

**Your own.** `--save-preset <name>` writes the test patterns of a run into
`data/test-presets/<name>.txt` in the data directory, and `--tests "@<name>"` uses it from then on,
in any project. The file holds patterns under a `===> tests` line, comma-separated or one per line,
with whatever stands above the line printed by `--show-presets` as its description, and it may name
a shipped preset, `@xt`, while one of yours naming another of yours is refused. A shipped preset is
never edited: to change one, take it back and add your own, `--tests "!@jest,@mine"`. A file of
yours named like a shipped one is never read, and every run with detection on says so until the
file is renamed, as the warning `preset-file-shadowed`.

C and C++ keep their tests in `test/` or `tests/` by the project's taste, and a taste is no preset:
write the folder, `./tests/`.


## Declaring the rest with `--tests`

`--tests` takes glob patterns, and what a pattern names is test code whole, files and folders
alike. A `!` in front takes back what an earlier pattern, a build file or a toolchain's file name
gave, and the last pattern that matches decides:

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
- A trailing `/` means a folder only, so `tests/` does not match a script named `tests`. `x/**`
  means the folder `x`, and `**` by itself a whole target.
- Under a `!`, what a marker that opens lines says stays: a `#[cfg(test)]` under `!vendor/` is
  still test code, and so is a file another file declares as a module under a marker. A marker
  that makes a whole file test code, `@Test`, is off there, since the `!` answers that very
  question: a corpus of JUnit files kept as test data stays data under `--tests "!cases/"`.
- A place missing from disk or outside every target is reported. A name that matched nothing is
  reported when it was typed for the run, and stays quiet from a configuration, which names
  folders some of its projects lack. A name a preset wrote is never reported, since the preset says
  what the tool looks for and the project may hold less.
- In PowerShell, quote the whole list or escape each comma with a backtick.

Patterns saved in a project's own configuration, with `--save-local` or under `===> tests` in
`.mezura/config.txt`, are read from the project directory, so they say the same thing from
wherever inside the project the command is typed, and they travel with the repository.


## Language by language

### Rust

**Found.** Attributes, read as the compiler reads them:

- `#[test]`, `#[bench]`, and any attribute whose name holds `test`: `#[tokio::test]`, `#[rstest]`,
  `#[wasm_bindgen_test]`, `#[test_case]`, `#[std::prelude::v1::test]`. Measured over the 11,921
  Rust files of a cargo registry: of 30,083 attributes with `test` in the name, one was no test
  attribute, `#[proptest_config]`, and it sat over a proptest anyway.
- `#[cfg(...)]` whose predicate holds `test` outside every `not(...)` and every `any(...)`:
  `#[cfg(test)]`, `#[cfg(all(test, unix))]`. `#[cfg(not(test))]` is production, and so is
  `#[cfg(any(test, feature = "x"))]`, since every build with the feature on compiles it; of the 66
  such attributes in the same registry, all sit over code a default build ships. `#[cfg(testlib)]`
  is a cfg of its own.
- `#[cfg_attr(predicate, attribute)]` only when the predicate says test and the attribute applied
  holds `test` in its name: `#[cfg_attr(test, test)]` is a marker, while
  `#[cfg_attr(test, derive(Debug))]` and `#![cfg_attr(test, allow(deref_nullptr))]` are settings.
- `#![cfg(test)]`: the rest of its scope, which is the whole file at the top of one, the comments
  and the other `#![...]` lines above it included since nothing else can stand there, and the module
  it sits in.
- `#[cfg(test)] mod tests;`, whose body is another file: that file is test code whole, and so is
  every file it declares in turn, whatever attribute those declarations carry, since everything
  under a `cfg(test)` module inherits it. The file is found where rustc finds it: `tests.rs` or
  `tests/mod.rs` beside a `lib.rs`, `main.rs` or `mod.rs`, under a folder named after any other
  file, and beside a crate root in `src/bin/`; a `mod x;` inside `mod outer { }` is under
  `outer/`. A file named this way that the count left out, under `--exclude` or a `.gitignore`, or
  that its path already makes test code whole, is still read for the modules it declares, so those
  are found too. Any `mod x;` on a line of test code counts, so one inside a
  `#[cfg(test)] mod tests { }` or under a `#![cfg(test)]` does too. The declaring file has to be in
  the same count, as the section above on where the count starts from says. `--explain` on such a
  file names the file that declares it and the one holding the marker.

From the marker, further `#[...]` on the item are skipped, and the test code runs to the `}`
matching the first `{`, or to the first `;`, whichever comes first, with brackets counted so that
`[u8; 3]` ends nothing. `let`, `use`, `const`, `static`, `type` and `extern crate` end at their
`;` and their braces are ignored, so `#[cfg(test)] let x = if a { 1 } else { 2 };` is one
statement, while a `const fn` and an `extern "C" { }` block end at their brace. An `if` carries on
through its `else` arms. Whatever sits inside test code opens nothing of its own, so the
`#[test]` functions of a `#[cfg(test)] mod tests { }` are counted once.

`tests/` beside a `Cargo.toml` is test code whole, `use` lines, helpers, `tests/common/` and
fixtures included. A `tests/` at the root of a virtual workspace, which cargo never builds, counts
too, since what sits there in practice is test crates and test data; `--tests "!./tests/"`, typed
at the root, says otherwise for that one folder. Everywhere else, `examples/`, `benches/` and
`src/bin/` included, only the markers decide, and a `tests` folder under `src/` is a folder.

**Missed**, each with what mezura answers today:

- A `#[cfg(any(test, X))]` where X is a cfg only test builds set, the kernel's `testlib` and tokio's
  `fuzzing`: production, since what X is lives in the build script.
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
  Lines 3 to 6 are test code, where the variant by itself is lines 3 to 4.
- An inner `#![cfg(test)]` inside a module counts from its own line, so the `mod checks {` above
  it stays out.
- A const generic in a signature, `fn sized() -> Wrapper<{ SIZE }> {`, ends the item at the `}` of
  `{ SIZE }`, so the body under it is production.

**Refused.** Doc tests, since a `///` fenced block is a comment. Tests a macro writes,
`make_test!(a, b, c);` being one line of code that only a compiler can expand. Tests written at
build time and pulled in with `include!`. A module declared under `#[path = "..."]`, or under a
`cfg_attr` that applies one, is not followed at all, since the file it names can sit anywhere, and
neither is anything declared inside an inline module that stands under one or opens its body with
`#![path = "..."]`; declare the files they can lead to with `--tests`.

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
beside them. Every `testdata/` below a `go.mod`, at any depth, since the go tool ignores a folder of
that name and what sits in one is the input of tests. A `testdata/` with no `go.mod` above it is a
folder.

### Perl

**Found.** `*.t` anywhere, whole, and `t/` beside a `Makefile.PL`, a `Build.PL` or a `dist.ini`.
The same three files turn the `@xt` preset on under their folder, so `xt/`, the author, release and
smoke tests of Dist::Zilla, is test code there too.

### Java and Kotlin

**Found.** `src/test/` beside a `pom.xml`, `build.gradle`, `build.gradle.kts`, `settings.gradle`,
`settings.gradle.kts` or `build.sbt`, and every `src/test/` below a `settings.gradle`,
`settings.gradle.kts` or `build.sbt`. `mezura ./core/src` finds the `core/pom.xml` above it and
counts `src/test` the same.

Anywhere else, a file is test code whole when its code holds one of the annotations the runners
collect tests by: `@Test` (JUnit 4 and 5, TestNG, kotlin.test and the annotations the JDK's own
test libraries name so), `@ParameterizedTest`, `@RepeatedTest`, `@TestFactory`, `@BeforeEach`,
`@AfterEach`, `@BeforeAll`, `@AfterAll`, `@BeforeClass`, `@AfterClass`, `@BeforeMethod`,
`@AfterMethod`, `@DataProvider` and `@RunWith`, and for Kotlin `@BeforeTest` and `@AfterTest` as
well. So a test class under `src/main`, a base class holding only a `@BeforeEach`, a TestNG
provider class and a JUnit 4 suite are found without a test directory and without a name. The
annotation may carry arguments, `@Test(expected = IllegalStateException.class)`. Measured over the
53,144 Java files of the JDK: 4,545 files hold one of these, and one of them sits outside `test/`,
a javadoc snippet file holding a test method as an example.

**Missed**, each with what mezura answers today:

- JUnit 3, a class extending `TestCase` with no annotation, and Kotest's spec classes, `FunSpec`,
  `StringSpec` and the rest, which register tests inside a lambda: production unless the test
  directory or a pattern says otherwise.
- A test class holding only a Spring slice annotation, `@SpringBootTest`, `@WebMvcTest`, and no
  `@Test` of its own: production, since those start with another word.
- A fully qualified `@org.junit.Test`, a space or a comment between the `@` and the name,
  `@ Test`, the two on separate lines, and the unicode escape `\u0040Test`: production.
- A `*Test.java` outside `src/test` holding no annotation: production, since the name is a
  convention; `--tests "*Test.java"` declares it.

**Refused**, and why: `@Before` and `@After` are AspectJ's advice annotations and sit on Spring
aspects; `@Rule` is Easy Rules'; `@Ignore` is Android Room's and Realm's, on entity fields;
`@Nested` is Gradle's, on task properties; `@Category` is the JDK's own, in `jdk.jfr`; `@ExtendWith`
and `@TestTemplate` appear in the main source of test-support libraries as meta-annotations. A test
class carrying only one of those is missed on purpose.

A `build.gradle`, `build.gradle.kts`, `settings.gradle` or `settings.gradle.kts` turns the `@gradle`
preset on under its folder, so Gradle's other test source sets, `src/integrationTest/`, Android's
`src/androidTest/` and `src/testFixtures/`, are test code there. A `test/` beside a `pom.xml` with
no `src` above it, since Maven compiles nothing from it, and a `testSourceDirectory` Maven was
pointed elsewhere, are declared, with `!src/test/` where the default was moved away from.

### Scala and Groovy

**Found.** The same `src/test/` directories as Java, where ScalaTest, specs2 and Spock live.

**Missed on purpose.** A test class outside `src/test`, since ScalaTest and Spock collect tests by
a base class, `AnyFunSuite`, `Specification`, and a base class is a name any library can define.
`@Test` of JUnit in a Scala or Groovy file outside `src/test` is production too.

### C#

**Found.** A file is test code whole when its code holds one of the attributes the runners collect
tests by: NUnit's `[Test]`, `[TestFixture]`, `[TestCase]`, `[TestCaseSource]`, `[SetUp]`,
`[TearDown]` and `[OneTimeSetUp]`, Unity's `[UnityTest]`, xUnit's `[Fact]` and `[Theory]`, and
MSTest's `[TestClass]`, `[TestMethod]` and `[TestInitialize]`. The attribute may carry arguments or
sit first in a list, `[Fact(Skip = "slow")]`, `[Test, Order(1)]`, and other attribute lists may
stand in front of it on its line, `[Category("Slow")] [Test]`. `[TestFixture]` and
`[TestClass]` catch a fixture whose tests use a custom attribute, and the setup attributes catch a
base fixture that holds no test. Measured over the 14,427 C# files of four Unity projects: every
file holding one of these outside a test directory is a test, samples of the test framework and
base fixtures shipped in `Editor/` folders among them, and `[UnityTest]` by itself finds 17 files
nothing else does.

No build file defines a test directory, since a `.csproj` names its test projects by convention
and nothing else; `--tests "@dotnet"` declares them, which is `*Tests/` and `*.Test/`.

**Missed**, each with what mezura answers today:

- An attribute written after another in one list, `[Trait("Category", "Slow"), Fact]`: the
  file is production unless another attribute of the list sits first somewhere in it.
- An attribute written with its suffix, `[TestAttribute]`, fully qualified, `[NUnit.Framework.Test]`,
  with a space, `[ Test ]`, or with a target, `[method: Test]`: production.
- An attribute with code in front of it on its line, the one-line
  `class T { [Test] public void A() { } }`: production. Over the 14,450 C# files of this machine
  all 8,415 test attributes stand at the start of their line or after another attribute list.

**Refused**, and why: `[Category]` is `System.ComponentModel.Category`, on Unity runtime code;
`[Ignore]` is sqlite-net's, on model classes; `[InlineData]`, `[DataRow]` and `[ClassInitialize]`
only ever sit beside an attribute already on the list. An indexer over a variable named `Test`,
`map[Test]` or `grid[0][Test]`, a list pattern `args is [Test]`, a dictionary initializer
`{ [Test] = 1 }` and a collection expression over a type named `Fact`, `[Fact.Create(a)]`, are code
and mark nothing, since a marker written with its bracket counts only where nothing but other
attribute lists stands in front of it on its line.

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

### JavaScript, TypeScript, Python, PHP, Ruby, C and C++

Nothing from a build file, since theirs define no test directory. What is found by path is what a
test tool's own file turns on, as [the section above](#what-a-test-tools-file-turns-on) says:
jest's, vitest's and mocha's files, `conftest.py` and a pytest configuration declaring `testpaths`,
`.rspec` beside a `Gemfile`, and `phpunit.xml`. The rest is declared, `@unittest` and `@minitest`
among it, and for C and C++ the folder itself. vitest's in-source tests,
`if (import.meta.vitest) { }`, are code. Python's
doctests in a docstring are a string. C++'s `TEST_CASE` and `TEST_F` are a framework's macros, and
no framework is named.

### OCaml

`let%test` and `let%expect_test` have no closer: the binding ends where the next one begins, and
that is a layout rule the parser has no business guessing. Refused.


## What is refused for every language

- **Doc tests.** A `///` fenced block in Rust, a `>>>` in a Python docstring, an `iex>` in Elixir:
  every one of those lines is a comment, and a line is one thing.
- **Tests a macro or a build script generates.** What is counted is what is written.
- **A framework's names compiled into the program.** `describe`, `it`, `pytest`, `TEST_CASE`:
  every one is a claim about somebody's project, and the failure is silent miscounting. What is
  taken lives in the language file, the annotations of Java, Kotlin and C# under `FILE MARKERS`,
  or in `--tests`, visible and editable.
- **Test code inside a nested section.** vitest in the `<script>` of a `.vue` is JavaScript inside
  Vue, and stays so. The other way round, a `<script>` inside a file that is tests whole goes to
  the tests of the container with the rest of the file and is no section, and the keywords inside
  it are not counted at all.
- **A test directory by its name.** `test`, `tests` and `__tests__` are folders; the build
  file beside one, or a test tool's file above one, is what makes it tests. A project checked out
  under `D:/tests/`, or a home folder called `test`, changes nothing.


## Where the count shows

- **The report.** A `tests` row under each language that holds any, its `Files %` and `Lines %`
  being its share of the language, and one under the total. A language with sections of other
  languages draws `X itself` beside them, and `tests` joins those. `--tests-breakdown split` draws
  the rest of the language as a `production` row above `tests`, and the sections and `X itself`
  hang under that row, their percentages being shares of it. `--tests-breakdown
  removed` leaves the tests out of every row and shows them nowhere, no `tests` row under a
  language or under the total, the sections of other languages staying under it; the overview,
  `--sort`, `--top`, the keywords and the file rows follow, a file or a language that is tests
  whole getting no row, and a run that is tests whole gets one sentence saying so in place of the
  report.
  The document carries every figure whole whichever is chosen. `--hide tests` hides the rows and
  turns the detection off, patterns included, which also makes the run faster.
- **`--by-file`** draws nothing under a file row; the figure per file is in the document.
- **The JSON document.** `scope.tests_detected`, `languages[].tests` with the figures a nested
  language carries, `keywords` when they are counted, and `whole_files`, the files that are test
  code from their first line to their last, then `total.tests`, `modules[].total.tests` and
  `languages[].by_file[].tests`, the last without keywords. `scope.test_presets` lists the presets
  a tool's file turned on, one entry per preset and folder with the file and whether it decided a
  file, since the announcement under the report is not in the document. A comparison carries the
  compared pair under each of the figures, the file rows aside, which carry their own figures and
  no pair.
- **`--diff`** against a document written without detection, or by an earlier version, runs
  without it, so no row appears on either side, and says so. A preset that applies in one reading
  and is off in the other gets a note, `preset-on-one-side`, since the files matching its names
  count as tests on one side only.
- **`--explain`.** Every line of test code is marked `test code`, the totals say how many, and a
  file that is test code whole names the rule that decided it: the test directory with its build
  file, the name, the pattern, the tool's file that turned a preset on with the name of it that
  matched, the file that declares it as a module under a marker, the annotation in its code with
  its line, or the `#![cfg(test)]` at its top. The document carries `in_test` on those lines and
  the rule under `test_file`, a preset as `preset` with each tool's name, folder, file and pattern,
  and `configured_by` where pytest's configuration declared the names.
- **Keywords** are counted over the whole language, its test code included, and shown once under
  it. The ones found inside the test code are counted apart as well.
