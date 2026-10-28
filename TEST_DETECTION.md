# Test detection

mezura tells the test code of a language from the rest of it. By default, every language that holds any gets a
`tests` row under it in the report, the total gets one that adds them up, and the same figures are
in the JSON document, in a `--diff` and in `--explain`. This page says what is counted as test code, what a test tool's own file turns on, how the
rest is declared with `--tests`, and for each language what is found, what is refused and what is
missed.

The rule behind all of it: **a production line counted as a test hurts more than a test line left
uncounted.** So on its own mezura takes what the code itself states, what the build tool defines,
and the default names of a test tool whose own file sits in the folder.

- [Detected by default](#detected-by-default)
- [What a test tool's file turns on](#what-a-test-tools-file-turns-on)
- [Declaring the rest with --tests](#declaring-the-rest-with---tests)
- [Language by language](#language-by-language)
- [What is refused for every language](#what-is-refused-for-every-language)
- [Where the count shows](#where-the-count-shows)


## Detected by default

Three kinds of test code are found with no setup. The names a test tool's own file turns on are in
the [next section](#what-a-test-tools-file-turns-on).

### 1. Markers in the code

Rust, D and Zig write tests inside the source file, behind a marker the compiler reads: `#[test]`
and `#[cfg(test)]` in Rust, `unittest` in D, `test "name" {` in Zig. mezura counts from the marker
to the end of the item under it, its closing `}` or `;`. A marker inside a string or a comment is
text. The markers are declared in the language file, under `Tests`, as the
[language files guide](LANGUAGE_FILES_GUIDE.md) describes.

Java, Kotlin and C# write tests as ordinary classes that the test runner finds by an annotation or
an attribute: `@Test`, `@BeforeEach`, `[Fact]`, `[Test]`, `[TestFixture]`. One of these in a file
makes the whole file test code. The list lives in the language file under `FILE MARKERS` and holds
only names that a test framework defines and no production library uses, so `@VisibleForTesting`,
`@TestOnly` and `[Category]` are not on it. The marker has to be a whole word: `@TestOnly` is not
`@Test`, and `[TestFixture]` is not `[Test]`.

### 2. Names the toolchain defines

`*_test.go` and Perl's `*.t`, as whole files. The go tool compiles a `_test.go` file for `go test`
and nothing else, and a `.t` file is always a test script.

Names that are only a convention, `test_*.py`, `*Test.java`, `*.spec.ts`, `*_spec.rb`, are not
detected, because production files carry them too: `NodeTest.java` in the JDK, `ZTest.cs` in Unity,
`hit_test.rs` in a geometry crate. Declare them with `--tests`.

### 3. The test directory of a build tool, beside its build file

These directories are test code whole when the build file sits beside them:

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

The build file is never opened, only its presence counts, so a test directory moved elsewhere in
its configuration, Maven's `testSourceDirectory`, is declared with `--tests`. Gradle's other source
sets, `src/integrationTest` and Android's `src/androidTest`, come from the `@gradle` preset in the
next section.

`testdata` counts at any depth below a `go.mod`, since the go tool ignores folders of that name and
keeps test inputs in them. Without a `go.mod` above it, it is an ordinary folder.

Under a `settings.gradle`, `settings.gradle.kts` or `build.sbt`, every `src/test` below it is test
code, also in a subproject with no build file of its own: `core/src/test`, and sbt's
`core/jvm/src/test`. A `pom.xml` covers its own folder only.

Folder and build file names follow the case rules of the filesystem: on Windows and macOS a
`Tests/` beside a `Cargo.toml` counts, on Linux it is production.

`package.json`, `pyproject.toml`, `setup.py`, `composer.json`, `Gemfile`, `.csproj`,
`CMakeLists.txt` and `build.zig` define no test directory, so JavaScript, TypeScript, Python, PHP,
Ruby, C#, C, C++ and Zig get nothing from a build file: only what a test tool's file turns on, and
what `--tests` declares. A folder named `test`, `tests` or `__tests__` means nothing by itself.

### Counting part of a project

The answer is the same however the project is reached: from its root, from inside its test
directory (`mezura ./proj/tests`), from its sources (`mezura ./core/src`), by naming one file,
with `--explain` on that file, or in the checkout of a `--diff` revision. A build file above the
target is found wherever the count starts. A folder above the project never makes it tests:
`D:/tests/app` is tests only if `D:/Cargo.toml` exists.

One exception: a Rust file declared as a module under a test marker is test code only when the
declaring file is in the same count. Counting a folder below it, naming only the declared file, or
leaving the declaring file out with `--exclude`, a `.gitignore` or as generated makes it production.
`--explain` reads the declaring files from disk in any case, so it answers like a count of the whole
project.


## What a test tool's file turns on

A test tool's own file means the tool is in use, so its default test names apply under the folder
holding it: a `jest.config.ts` makes every `__tests__/` and every `*.test.ts` below its folder test
code, and the same names outside that folder are ordinary files. The file is only looked for and
never opened, except pytest's configuration, below. These are the presets, and `--show-presets`
lists them.

Only a file that belongs to one tool turns a preset on: `jest.config.ts`, `.rspec`, `phpunit.xml`,
`conftest.py`. `package.json`, `pyproject.toml`, a `Gemfile`, `composer.json` and a `.csproj` turn
nothing on. Two presets are narrower than the tool's own documentation:

- **pytest** is turned on by a `conftest.py`, or by a configuration file that declares `testpaths`,
  under those folders only. A `pytest.ini` without `testpaths` turns nothing on, since at a project
  root it would make library files such as aiohttp's `aiohttp/test_utils.py` count as tests.
- **RSpec** needs a `Gemfile` beside the `.rspec`, since a `.rspec` in the home folder would
  otherwise turn every `spec/` under it into tests.

A preset applies under its file's folder only, so in a monorepo jest's names apply under
`site/frontend/` and PHPUnit's under `site/backend/`. A file deeper down adds its own names and
removes nothing above it. A file above the target counts too: counting `./frontend/src` finds
`frontend/jest.config.ts`.

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

A preset with no file applies only when typed, `--tests "@unittest"`, because its names catch
production code somewhere: `testutil.py` in CPython, `PerformanceTests/` in Unity, and anybody's
`test/` or `src/test/`. jest's bare `test.js` and `spec.ts` are left out of its row, since
`lib/test.js` and `lib/reporters/spec.js` are production in mocha. A typed preset's `test/` matches
at any depth under the target; to mean one folder, write its place, `./test/`.

**Pytest's configuration is the one file that is read.** The first of `pytest.toml`,
`.pytest.toml`, `pytest.ini`, `.pytest.ini`, `pyproject.toml`, `tox.ini` and `setup.cfg` in a
folder, in pytest's own order, is read for `testpaths` and `python_files`. `testpaths` limits the
preset to those folders, and `python_files` replaces its names: files only, and a name without
`test` in it applies only under `testpaths`. A `conftest.py` inside `testpaths` uses the shipped
names whatever the configuration says. A `*` in `python_files` stops at a `/`, so mezura takes at
most what pytest takes. No other tool's configuration is read, so a suite selected there is missed:
babel's `test/` under its jest configuration, svelte's `tests/` under its vitest one, a jest
`rootDir` pointing elsewhere.

**The presets that applied are listed under the report:**

```
Test presets applied:
  @jest      under ./frontend (jest.config.ts)
  @pytest    under ./tools, ./scripts and 3 more (conftest.py)
To disable a test preset: '--tests "!@<name>"'. To disable test detection: '--hide tests'.
```

Only a preset that matched a file is listed. Once the project's own configuration has a `tests`
block, saved with `--save-local` or written under `===> tests` in `.mezura/config.txt`, the list is
no longer printed; the presets still apply, and `scope.test_presets` in the JSON document and
`--explain` still name them. When a `package.json`, `pyproject.toml`, `Gemfile` or `composer.json` in
the target's folder names a test tool, no preset matched and that language has no tests, one line
names the preset to type: `jest tests are declared with '--tests "@jest"'`.

**A preset by name.** `@name` in `--tests` stands for the preset's patterns, applied everywhere
under the target whether or not the tool's file is there: `--tests "@pytest,!tests/data/"`.
`!@jest` turns a preset off, typed or turned on by a file, and leaves every other rule alone. A
typed pattern always comes after what a file turned on, so `--tests "!tests/data/"` corrects the
pytest preset and keeps the rest of it. A folder whose name starts with `@` is written as a place,
`./@types/`. An unknown preset name stops the run and lists the known ones. In PowerShell a preset
goes in quotation marks, since the shell reads a bare word starting with `@` as a variable.

`--save` and `--save-local` write the patterns with every `@name` expanded and `!@name` kept as
written, so a saved configuration does not depend on the presets of the machine that wrote it.

**Your own.** `--save-preset <name>` saves the test patterns of a run as
`data/test-presets/<name>.txt` in the data directory, and `--tests "@<name>"` uses it from then on,
in any project. The file holds patterns under a `===> tests` line, comma-separated or one per line,
and the text above that line is the description `--show-presets` prints. It may name a shipped
preset, `@xt` or `!@jest`, and never another of yours. A shipped preset cannot be edited: take it
back and add yours, `--tests "!@jest,@mine"`. A file of yours named like a shipped preset is
ignored, with the warning `preset-file-shadowed` on every run until it is renamed.

C and C++ have no preset, since where their tests live is up to each project: write the folder,
`./tests/`.


## Declaring the rest with `--tests`

`--tests` takes glob patterns, and what a pattern names is test code whole, files and folders
alike. A `!` in front takes back what an earlier pattern, a build file or a toolchain's file name
gave, and the last pattern that matches decides:

```bash
mezura ./ --tests "spec/,!spec/fixtures/"
mezura ./ --tests "test_*.py,*_test.py,conftest.py"
mezura ./ --tests "!vendor/"
```

The rules in short, with the full text under `--tests` in [COMMANDS.md](COMMANDS.md):

- A pattern with no `./`, no `../` and no drive or root is a **name**, matched at any depth from the
  target down, the target's own name included. Names are matched with their case.
- A pattern starting with `./` or `../`, or written as a full path, is a **place**, read from the
  directory the command runs in, and covers that place and everything under it.
- A trailing `/` matches folders only, so `tests/` leaves a script named `tests` alone. `x/**`
  means the folder `x`, and `**` the whole target.
- A `!` leaves the markers in the code in place: a `#[cfg(test)]` under `!vendor/` is still test
  code, and so is a module declared under one. It does undo a marker that makes a whole file test
  code, such as `@Test`, so JUnit files kept as test data stay data under `--tests "!cases/"`.
- A place missing from disk or outside every target is reported. A name that matched nothing is
  reported when typed for the run, and stays quiet when it comes from a configuration or a preset.
- In PowerShell, quote the whole list or escape each comma with a backtick.

Patterns saved in the project's configuration, with `--save-local` or under `===> tests` in
`.mezura/config.txt`, are read from the project directory, so they mean the same wherever inside
the project the command runs, and they travel with the repository.


## Language by language

### Rust

**Found.** Attributes, read as the compiler reads them:

- `#[test]`, `#[bench]`, and any attribute with `test` in its name: `#[tokio::test]`, `#[rstest]`,
  `#[wasm_bindgen_test]`, `#[test_case]`.
- `#[cfg(...)]` with `test` outside every `not(...)` and every `any(...)`: `#[cfg(test)]`,
  `#[cfg(all(test, unix))]`. `#[cfg(not(test))]` and `#[cfg(any(test, feature = "x"))]` are
  production, since a normal build can compile them: 65 of the 66 `any(test, ...)` in a cargo
  registry sit over code a default build ships. `#[cfg(testlib)]` is a cfg of its own.
- `#[cfg_attr(predicate, attribute)]` when the predicate says test and the attribute has `test` in
  its name: `#[cfg_attr(test, test)]` counts, `#[cfg_attr(test, derive(Debug))]` does not.
- `#![cfg(test)]`: the rest of its scope, which at the top of a file is the whole file.
- `#[cfg(test)] mod tests;`: the file it declares is test code whole, and so is every file that one
  declares in turn. The file is found where rustc finds it. A file left out by `--exclude` or a
  `.gitignore` is still read for the modules it declares. A `mod x;` anywhere inside test code counts
  the same. The declaring file has to be in the same count, as
  [above](#counting-part-of-a-project), and `--explain` on a declared file names the file that
  declares it.

From the marker, the test code runs to the `}` matching the first `{`, or to the first `;`,
whichever comes first. `let`, `use`, `const`, `static`, `type` and `extern crate` end at their `;`,
so `#[cfg(test)] let x = if a { 1 } else { 2 };` is one statement. An `if` carries on through its
`else` arms. The `#[test]` functions inside a `#[cfg(test)] mod tests { }` are counted once.

`tests/` beside a `Cargo.toml` is test code whole, helpers and fixtures included, and so is a
`tests/` at the root of a virtual workspace; `--tests "!./tests/"`, typed at the root, takes it
back. Everywhere else, `examples/`, `benches/` and `src/bin/` included, only the markers decide, and
a `tests` folder under `src/` is a folder.

Measured over the 4,695 Rust files of a cargo registry, 1.9 million lines: 341,282 of its 341,616
test lines are found, and 29 lines are counted as tests wrongly.

**Missed:**

- A `#[cfg(any(test, X))]` where X is a cfg only test builds set, the kernel's `testlib` and tokio's
  `fuzzing`: production.
- A marker on a struct field, an enum variant, a match arm or a parameter runs to the `}` of the
  block around it, taking the siblings after it:
  ```rust
  pub enum Shape {
      Circle,
      #[cfg(test)]
      Probe,
      Square,
  }
  ```
  Lines 3 to 6 are test code, where the variant is lines 3 to 4.
- An inner `#![cfg(test)]` inside a module counts from its own line, so the `mod checks {` above
  it stays out.
- A const generic in a signature, `fn sized() -> Wrapper<{ SIZE }> {`, ends the item at
  `{ SIZE }`, so the body under it is production.

**Refused.** Doc tests, since a `///` block is a comment. Tests a macro writes,
`make_test!(a, b, c);`. Tests written at build time and pulled in with `include!`. A module declared
under `#[path = "..."]` is never followed, since the file it names can sit anywhere; declare those
files with `--tests`.

### D

**Found.** `unittest { }`, `@safe unittest { }` and `version(unittest) { }`, to the matching `}`.
The `else` of a `version(unittest)` is production. `version(unittest):` with a colon covers the
rest of its scope: the struct or block it sits in, or the rest of the file at module level.
`myunittest` is another word.

**Missed.** An attribute on its own line above the marker: with `@safe` on one line and `unittest`
on the next, the count starts at `unittest`.

### Zig

**Found.** `test "name" { }` and `test { }`, to the matching `}`. `test_helper` is another word.
`build.zig` defines no test directory, so a `test/` folder is declared with `--tests`.

### Go

**Found.** `*_test.go` anywhere, whole: the tests, the examples, the benchmarks and the helpers
beside them. Every `testdata/` below a `go.mod`, at any depth.

### Perl

**Found.** `*.t` anywhere, whole, and `t/` beside a `Makefile.PL`, a `Build.PL` or a `dist.ini`.
The same three files turn the `@xt` preset on, so `xt/` is test code there too.

### Java and Kotlin

**Found.** `src/test/` beside a `pom.xml`, `build.gradle`, `build.gradle.kts`, `settings.gradle`,
`settings.gradle.kts` or `build.sbt`, and every `src/test/` below a `settings.gradle`,
`settings.gradle.kts` or `build.sbt`. `mezura ./core/src` finds the `core/pom.xml` above it.

Anywhere else, a file is test code whole when it holds one of these annotations: `@Test` (JUnit 4
and 5, TestNG, kotlin.test), `@ParameterizedTest`, `@RepeatedTest`, `@TestFactory`, `@BeforeEach`,
`@AfterEach`, `@BeforeAll`, `@AfterAll`, `@BeforeClass`, `@AfterClass`, `@BeforeMethod`,
`@AfterMethod`, `@DataProvider` and `@RunWith`, and for Kotlin `@BeforeTest` and `@AfterTest` as
well. So a test class under `src/main`, a base class with only a `@BeforeEach`, a TestNG data
provider and a JUnit 4 suite are all found. Arguments are fine:
`@Test(expected = IllegalStateException.class)`. Over the 53,155 Java files of the JDK, 4,531 files
hold one of these, and only one sits outside `test/`: a javadoc snippet holding a test method as an
example.

**Missed**, counted as production:

- JUnit 3, a class extending `TestCase` with no annotation, and Kotest's specs, `FunSpec`,
  `StringSpec` and the rest.
- A class with only a Spring test annotation, `@SpringBootTest`, `@WebMvcTest`, and no `@Test`.
- `@org.junit.Test`, `@ Test`, the `@` and the name on separate lines, and `\u0040Test`.
- A `*Test.java` outside `src/test` with no annotation; declare it with `--tests "*Test.java"`.

**Refused**, because production code uses these names too: `@Before` and `@After` (AspectJ advice
on Spring aspects), `@Rule` (Easy Rules), `@Ignore` (Room and Realm entity fields), `@Nested`
(Gradle task properties), `@Category` (the JDK's `jdk.jfr`), `@ExtendWith` and `@TestTemplate`
(meta-annotations in test-support libraries). A test class carrying only one of those is missed.

A Gradle build file also turns the `@gradle` preset on under its folder, so `src/integrationTest/`,
Android's `src/androidTest/` and `src/testFixtures/` are test code there. A `test/` beside a
`pom.xml`, and a `testSourceDirectory` pointed elsewhere, are declared with `--tests`, with
`!src/test/` where the default was moved away from.

### Scala and Groovy

**Found.** The same `src/test/` directories as Java, where ScalaTest, specs2 and Spock live.

**Missed on purpose.** A test class outside `src/test`: ScalaTest and Spock find tests by a base
class, `AnyFunSuite`, `Specification`, and any library can define a base class of that name.
JUnit's `@Test` in a Scala or Groovy file outside `src/test` is production too.

### C#

**Found.** A file is test code whole when it holds one of these attributes: NUnit's `[Test]`,
`[TestFixture]`, `[TestCase]`, `[TestCaseSource]`, `[SetUp]`, `[TearDown]` and `[OneTimeSetUp]`,
Unity's `[UnityTest]`, xUnit's `[Fact]` and `[Theory]`, and MSTest's `[TestClass]`, `[TestMethod]`
and `[TestInitialize]`. Arguments and lists are fine, `[Fact(Skip = "slow")]`, `[Test, Order(1)]`,
and other attribute lists may come first on the line, `[Category("Slow")] [Test]`. `[TestFixture]`,
`[TestClass]` and the setup attributes also catch a fixture with a custom test attribute and a base
fixture with no test. Over four Unity projects, 2.75 million lines of C#, these attributes find
173,754 test lines and none wrongly; most of the 17,229 test lines left are helpers in `Tests/`
folders, which `@dotnet` takes.

No build file defines a test directory; `--tests "@dotnet"` declares the usual ones, `*Tests/` and
`*.Test/`.

**Missed**, counted as production:

- An attribute after another in one list, `[Trait("Category", "Slow"), Fact]`, unless the file
  holds another test attribute written first.
- `[TestAttribute]`, `[NUnit.Framework.Test]`, `[ Test ]` and `[method: Test]`.
- An attribute with code before it on its line, as in the one-line
  `class T { [Test] public void A() { } }`.

**Refused.** `[Category]` is `System.ComponentModel.Category`, on Unity runtime code, and
`[Ignore]` is sqlite-net's, on model classes. `[InlineData]`, `[DataRow]` and `[ClassInitialize]`
always sit beside an attribute already on the list. A bracket that is code, `map[Test]`,
`args is [Test]`, `{ [Test] = 1 }`, `[Fact.Create(a)]`, marks nothing: a marker counts only where
nothing but attribute lists stands before it on its line.

### Swift

**Found.** `Tests/` beside a `Package.swift`. A `*Tests.swift` under `Sources/`, or an
`XCTestCase` outside `Tests/`, is production. For an Xcode project with no `Package.swift`, declare
the test target's folder with `--tests`.

### Elixir

**Found.** `test/` beside a `mix.exs`. A `_test.exs` elsewhere is production; declare it with
`--tests "*_test.exs"`.

### Dart and Flutter

**Found.** `test/` and `integration_test/` beside a `pubspec.yaml`. A `_test.dart` under `lib/` is
production.

### Julia

**Found.** `test/` beside a `Project.toml` or a `JuliaProject.toml`.

### Clojure

**Found.** `test/` beside a `project.clj`. A `deps.edn` names its test paths in an alias, so with
one the folders are declared.

### Erlang

**Found.** `test/` beside a `rebar.config`, where Common Test suites and EUnit's `_tests` modules
live. An `-ifdef(TEST).` block inside a module is production.

### Elm

**Found.** `tests/` beside an `elm.json`.

### R

**Found.** `tests/` beside both a `DESCRIPTION` and a `NAMESPACE`, since an Octave package carries a
`DESCRIPTION` too. testthat's `tests/testthat/` is inside it.

### JavaScript, TypeScript, Python, PHP, Ruby, C and C++

Nothing from a build file. What is found by path comes from a test tool's file, as
[above](#what-a-test-tools-file-turns-on): jest's, vitest's and mocha's files, `conftest.py` and a
pytest configuration declaring `testpaths`, `.rspec` beside a `Gemfile`, and `phpunit.xml`.
Everything else is declared, `@unittest` and `@minitest` included, and for C and C++ the test
folder. On Home Assistant, the pytest preset turned on by its `conftest.py` files finds 1,904,376 of
its 1,980,689 test lines and none wrongly; what is left is helpers such as `common.py` and
`__init__.py`. vitest's in-source tests, `if (import.meta.vitest) { }`, are code, Python's doctests are a
string, and C++'s `TEST_CASE` and `TEST_F` macros are not detected.

### OCaml

**Refused.** `let%test` and `let%expect_test` have no closing symbol: the binding ends where the
next one begins, which only the layout tells.


## What is refused for every language

- **Doc tests.** Rust's `///` blocks, a `>>>` in a Python docstring, Elixir's `iex>`: those lines
  are comments.
- **Tests a macro or a build script generates.** What is counted is what is written.
- **A test framework's names**, `describe`, `it`, `pytest`, `TEST_CASE`: guessing them would
  silently miscount somebody's project. What is detected lives in the language files, the
  annotations of Java, Kotlin and C# under `FILE MARKERS`, or in `--tests`, where it can be seen and
  edited.
- **Test code inside a nested section.** vitest code in the `<script>` of a `.vue` file is
  JavaScript inside Vue. A `<script>` inside a file that is test code whole goes to the tests of
  that file, and its keywords are not counted.
- **A test directory by its name.** `test`, `tests` and `__tests__` are ordinary folders: a build
  file beside one, or a test tool's file above it, is what makes it tests. A project under
  `D:/tests/` changes nothing.


## Where the count shows

- **The report.** A `tests` row under each language that has tests, and one under the total. Its
  `Files %` and `Lines %` are its share of the language. In a language holding sections of other
  languages, `tests` sits beside them and `X itself`. `--tests-breakdown split` adds a `production`
  row above `tests`, under each language and under the total, and the sections hang under
  `production`. `--tests-breakdown removed` leaves the tests out everywhere: no `tests` rows, and the
  overview, `--sort`, `--top`, the keywords and the file rows use the figures without them; a run
  that is all tests prints one sentence. The JSON document always carries the whole figures.
  `--hide tests` turns the detection off, patterns included, which also makes the run faster.
- **`--by-file`** draws nothing under a file row; the figures per file are in the JSON document.
- **The JSON document.** `scope.tests_detected`; `languages[].tests` with the figures a language
  carries, `keywords` when they are counted, and `whole_files`, the files that are test code from
  their first line to their last; `total.tests`, `modules[].total.tests` and
  `languages[].by_file[].tests`, the last without keywords. `scope.test_presets` lists the presets a
  tool's file turned on, one entry per preset and folder, with the file and whether it matched
  anything. A comparison carries the before and after pair under each figure, except in the file
  rows.
- **`--diff`** against a document written without detection, or by an older version, runs without
  it and says so. A preset on in one reading and off in the other gets the note
  `preset-on-one-side`.
- **`--explain`** marks every line of test code `test code`, gives the total, and for a file that is
  test code whole names the rule that decided it: the test directory with its build file, the name,
  the pattern, the preset with the tool's file and the name that matched, the file that declares it
  as a module, the annotation and its line, or the `#![cfg(test)]` at its top. The JSON document
  carries `in_test` on those lines and the rule under `test_file`, a preset as `preset` with each
  tool's name, folder, file and pattern, and `configured_by` where pytest's configuration declared
  the names.
- **Keywords** are counted over the whole language, its test code included, and shown once under
  it. Those found in the test code are also counted apart.
