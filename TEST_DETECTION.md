# Test detection

mezura separates test code from production code. By default, every language with tests gets a
`tests` row under it in the report, the total gets one that sums them up, and the same figures
appear in the JSON output, `--diff`, and `--explain`.

The rule behind default detection is simple: **counting production code as a test is worse than
missing a test.** Out of the box, mezura only counts code as tests when the source file, the
toolchain, or a test runner's config file makes it unambiguous. Everything else can be declared
with `--tests`.

- [Detected by default](#detected-by-default)
- [Test tool presets](#test-tool-presets)
- [Declaring custom test paths with `--tests`](#declaring-custom-test-paths-with---tests)
- [Language by language](#language-by-language)
- [What is not detected](#what-is-not-detected)
- [Where the count appears](#where-the-count-appears)


## Detected by default

mezura detects test code automatically in three ways, plus through [test tool presets](#test-tool-presets).

### 1. Markers in the source code

- **Inline test blocks (Rust, D, Zig):** These languages mark test blocks directly in source files
  (`#[test]` and `#[cfg(test)]` in Rust, `unittest` in D, `test "name" {` in Zig). mezura counts
  from the marker to the closing `}` or `;` of the item under it. Markers inside strings or comments
  are ignored.
- **File-level test annotations (Java, Kotlin, C#):** When a file contains a test runner annotation
  or attribute (such as `@Test` or `@BeforeEach` in Java/Kotlin, or `[Fact]`, `[Test]`,
  `[TestFixture]` in C#), the entire file is counted as test code. Markers are matched as whole
  words (`@TestOnly` does not match `@Test`) and are listed in each language file under
  `FILE MARKERS`.

### 2. File names required by the language toolchain

Some toolchains reserve specific file suffixes exclusively for tests:
- Go: `*_test.go`
- Perl: `*.t`

Any file matching these names is counted as test code. Naming conventions that are not enforced by a
toolchain (like `test_*.py`, `*Test.java`, or `*.spec.ts`) are not enabled globally by default,
because production files sometimes share those names. They are enabled either automatically by a
[test tool preset](#test-tool-presets) or manually with `--tests`.

### 3. Standard test directories next to a build file

When a build tool defines a fixed directory for tests, mezura counts that directory as test code
whenever its build file is in the same project root:

| Build file | Test directory |
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
| `DESCRIPTION` + `NAMESPACE` | `tests/` |
| `go.mod` | any `testdata/` directory below it |

Notes:
- mezura checks for the build file's presence on disk rather than parsing custom directory
  overrides inside it. If your project moves its test directory to a custom path, pass it with
  `--tests`.
- For multi-project Gradle (`settings.gradle`, `settings.gradle.kts`) and sbt (`build.sbt`) builds,
  every `src/test/` in subprojects below the root is also counted (e.g. `core/src/test/`).
- Build files that do not enforce a standard test directory (such as `package.json`,
  `pyproject.toml`, `CMakeLists.txt`, or `.csproj`) do not mark a folder on their own; for those
  ecosystems, mezura uses [test tool presets](#test-tool-presets) or `--tests`.

### Counting a subdirectory or single file

You get the same result whether you count from the project root, a subdirectory (`mezura ./src`),
or a single file (`mezura ./tests/api.rs --explain`). mezura walks upward from the target to find
any enclosing build file or test runner config.

The one exception is Rust module files (`#[cfg(test)] mod helper;`): a separate module file is known
to be test code only when the file that declares `mod helper;` is part of the same run (or when
inspecting the file with `--explain`, which checks neighboring files on disk).


## Test tool presets

In ecosystems where the build file does not fix a test location (JavaScript/TypeScript, Python, Ruby,
PHP), mezura looks for the test runner's own config file. When found, it enables that runner's
default test file patterns under the folder containing the config file.

For example, if `frontend/jest.config.ts` exists, `__tests__/` and `*.test.ts` inside `frontend/`
are counted as tests, without affecting other folders in a monorepo.

Run `--show-presets` to see all available presets:

| Preset | Activated automatically by | Patterns matched |
|---|---|---|
| `@jest` | `jest.config.*` | `__tests__/`, `*.test.{js,jsx,ts,tsx,mjs,cjs,mts,cts}`, `*.spec.{js,jsx,ts,tsx,mjs,cjs,mts,cts}` |
| `@vitest` | `vitest.config.*`, `vitest.workspace.*`, `vite.config.*` | `*.test.{js,jsx,ts,tsx,mjs,cjs,mts,cts}`, `*.spec.{js,jsx,ts,tsx,mjs,cjs,mts,cts}` |
| `@mocha` | `.mocharc.*` | `test/` |
| `@pytest` | `conftest.py`, or `testpaths` in a pytest config (`pytest.ini`, `pyproject.toml`, `tox.ini`, `setup.cfg`) | `test_*.py`, `*_test.py`, `conftest.py` |
| `@rspec` | `.rspec` (when a `Gemfile` is in the same folder) | `spec/` |
| `@phpunit` | `phpunit.xml`, `phpunit.xml.dist`, `phpunit.dist.xml` | `tests/`, `Tests/`, `test/` |
| `@gradle` | `build.gradle`, `build.gradle.kts`, `settings.gradle`, `settings.gradle.kts` | `src/*Test/`, `src/testFixtures/` |
| `@xt` | `Makefile.PL`, `Build.PL`, `dist.ini` | `xt/` |
| `@unittest` | Manual only (`--tests "@unittest"`) | `test*.py` |
| `@minitest` | Manual only (`--tests "@minitest"`) | `test/` |
| `@dotnet` | Manual only (`--tests "@dotnet"`) | `*Tests/`, `*.Test/` |
| `@jvm` | Manual only (`--tests "@jvm"`) | `src/test/` |
| `@odin` | Manual only (`--tests "@odin"`) | `*_test.odin` |

### Pytest configuration reading

For `@pytest`, mezura also reads `testpaths` and `python_files` from the project's pytest
configuration (`pytest.toml`, `.pytest.toml`, `pytest.ini`, `.pytest.ini`, `pyproject.toml`,
`tox.ini`, or `setup.cfg`, checked in pytest's precedence order):
- `testpaths` scopes the preset to the listed directories. (A config file without `testpaths` and
  without a `conftest.py` does not activate the preset across the whole repo.)
- `python_files` overrides the default file patterns (`test_*.py`, `*_test.py`).

### Inspecting and overriding presets

When presets are auto-activated and match at least one file, they are listed at the bottom of the
report:

```text
Test presets applied:
  @jest      under ./frontend (jest.config.ts)
  @pytest    under ./tools, ./scripts and 3 more (conftest.py)
To disable a test preset: '--tests "!@<name>"'. To disable test detection: '--hide tests'.
```

You can also control presets explicitly through `--tests`:
- **Enable a preset manually:** `--tests "@pytest"` or `--tests "@dotnet"` applies the preset's
  patterns across the entire target, even when no config file is present.
- **Exclude specific paths from a preset:** `--tests "@pytest,!tests/data/"` keeps the preset but
  marks `tests/data/` back as production code.
- **Disable an auto-detected preset:** `--tests "!@jest"` turns off `@jest` while leaving all other
  rules untouched.
- **Save your own preset:** `--save-preset <name>` saves the `--tests` patterns of the current run
  to `data/test-presets/<name>.txt` so you can reuse them across projects with `--tests "@<name>"`.


## Declaring custom test paths with `--tests`

Use `--tests` with comma-separated glob patterns to mark files or directories as test code, or
prefix a pattern with `!` to mark it back as production code. When multiple patterns match a path,
the last matching pattern wins:

```bash
mezura ./ --tests "spec/,!spec/fixtures/"
mezura ./ --tests "test_*.py,*_test.py,conftest.py"
mezura ./ --tests "!vendor/"
```

Key pattern rules (see `--tests` in [COMMANDS.md](COMMANDS.md) for the full reference):
- **Name patterns** (no `./`, `../`, or root prefix, e.g. `test_*.py` or `tests/`) match at any
  depth under the target.
- **Place patterns** (starting with `./`, `../`, or an absolute path, e.g. `./tests/`) are resolved
  from the current working directory (or from the project root when saved in `.mezura/config.txt`)
  and match that path and everything inside it.
- **Directory-only patterns** end with `/` (e.g. `tests/`), matching directories and everything
  inside them, but not a regular file named `tests`.
- **Negation (`!`):** A `!` pattern overrides path-based test rules and file-level annotations
  (like `@Test` in Java), turning matched files back into production code. It does not strip inline
  test blocks (`#[cfg(test)]`, `unittest`, `test "..." {`) inside a file.
- **PowerShell note:** Wrap the pattern list in quotes (`--tests "@pytest,!tests/data/"`), since
  PowerShell treats unquoted `,` and `@` specially.


## Language by language

### Rust

**Detected:**
- Any item with `#[test]`, `#[bench]`, or an attribute containing `test` in its name
  (`#[tokio::test]`, `#[rstest]`, `#[wasm_bindgen_test]`, `#[test_case]`).
- `#[cfg(...)]` where `test` is required (`#[cfg(test)]`, `#[cfg(all(test, unix))]`). Conditions
  like `#[cfg(not(test))]` or `#[cfg(any(test, feature = "x"))]` are kept as production code because
  non-test builds can compile them.
- `#[cfg_attr(predicate, attr)]` when the predicate requires `test` and `attr` contains `test`.
- `#![cfg(test)]` for the remainder of its enclosing scope (or the entire file when placed at the
  top).
- Separate module files declared under a test marker (`#[cfg(test)] mod tests;`), along with any
  submodules they declare.
- Everything inside `tests/` next to a `Cargo.toml`.

From an inline marker, the test block extends to the matching `}` or `;` of the item (`if`/`else`
chains stay together, and `#[test]` functions inside a `#[cfg(test)] mod` are counted once).

**Limitations:**
- A marker on a single struct field, enum variant, or match arm counts down to the closing `}` of
  the enclosing block.
- A const generic block in a function signature (`fn f() -> W<{ N }> {`) closes at `{ N }` rather
  than the function body.
- Modules using `#[path = "..."]`, tests generated by macros (`make_test!`), `include!` files, and
  doc tests inside `///` comments are not followed or counted as test code.

### D

**Detected:** `unittest { }`, `@safe unittest { }`, and `version(unittest) { }` up to the matching
`}` (any `else` branch is production). `version(unittest):` applies to the rest of its enclosing
block or file.

**Limitation:** If an attribute like `@safe` is on the line above `unittest`, counting starts on the
`unittest` line.

### Zig

**Detected:** `test "name" { }` and `test { }` blocks up to the matching `}`. Because `build.zig`
has no fixed test directory, pass any standalone test folder with `--tests`.

### Go

**Detected:** Every `*_test.go` file, and every `testdata/` directory at any depth under a `go.mod`.

### Perl

**Detected:** Every `*.t` file, plus `t/` and `xt/` (via `@xt`) next to `Makefile.PL`, `Build.PL`,
or `dist.ini`.

### Java and Kotlin

**Detected:**
- `src/test/` next to `pom.xml`, `build.gradle`, `build.gradle.kts`, `settings.gradle`,
  `settings.gradle.kts`, or `build.sbt` (including subprojects under a root settings/sbt file).
- `src/*Test/` and `src/testFixtures/` under Gradle builds via the `@gradle` preset.
- Any file containing `@Test`, `@ParameterizedTest`, `@RepeatedTest`, `@TestFactory`,
  `@BeforeEach`, `@AfterEach`, `@BeforeAll`, `@AfterAll`, `@BeforeClass`, `@AfterClass`,
  `@BeforeMethod`, `@AfterMethod`, `@DataProvider`, `@RunWith`, or Kotlin's `@BeforeTest` /
  `@AfterTest`.

**Limitations:**
- JUnit 3 classes (`extends TestCase` without annotations), Kotest spec classes (`FunSpec`, etc.),
  and files with only Spring slice annotations (`@SpringBootTest`) and no `@Test` method are not
  detected by content outside `src/test/`.
- Fully qualified annotations (`@org.junit.Test`) or annotations whose name is shared with
  production libraries (`@Before`, `@After`, `@Rule`, `@Ignore`, `@Category`) are not used as file
  markers.

### Scala and Groovy

**Detected:** `src/test/` directories next to Maven, Gradle, or sbt build files. Test classes
outside `src/test/` are not detected by content (ScalaTest and Spock rely on base class names rather
than annotations).

### C#

**Detected:** Any file containing NUnit (`[Test]`, `[TestFixture]`, `[TestCase]`,
`[TestCaseSource]`, `[SetUp]`, `[TearDown]`, `[OneTimeSetUp]`), xUnit (`[Fact]`, `[Theory]`),
MSTest (`[TestClass]`, `[TestMethod]`, `[TestInitialize]`), or Unity (`[UnityTest]`) attributes.
To also include helper files in naming-convention folders (`*Tests/`, `*.Test/`), pass
`--tests "@dotnet"`.

**Limitations:**
- An attribute placed second inside a combined bracket list (`[Trait("a", "b"), Fact]`) or after
  code on the same line (`class T { [Test] public void A() {} }`) is not matched unless another test
  attribute appears at the start of a line in the same file.
- Fully qualified attributes (`[NUnit.Framework.Test]`) and `[TestAttribute]` are not matched.

### Swift, Elixir, Dart, Julia, Clojure, Erlang, Elm, and R

**Detected:** The standard test directory next to the project's build file, as listed in the
[build tool table above](#3-standard-test-directories-next-to-a-build-file) (`Tests/` next to
`Package.swift`, `test/` next to `mix.exs`, `test/` and `integration_test/` next to `pubspec.yaml`,
etc.). Test files placed outside those directories require `--tests`.

### JavaScript, TypeScript, Python, PHP, Ruby, C, and C++

**Detected:** Via [test tool presets](#test-tool-presets) when a runner config file (`jest.config.*`,
`vitest.config.*`, `vite.config.*`, `.mocharc.*`, `conftest.py`, `pytest.ini` with `testpaths`,
`.rspec` next to `Gemfile`, `phpunit.xml`) is present in the project, or manually via `--tests`.
In-source framework calls (`describe(...)`, `it(...)`, C++ `TEST_CASE(...)`) are not parsed from
file contents.


## What is not detected

- **Doc tests** inside comments or docstrings (`///` in Rust, `>>>` in Python, `iex>` in Elixir)
  remain counted as comments or strings.
- **Macro-generated or build-generated tests** that do not appear as test declarations in the source
  file.
- **Framework function calls** such as `describe`, `it`, or `TEST_CASE` in languages without dedicated
  syntax or annotations.
- **Inline test blocks inside embedded language sections** (e.g. a `<script>` block inside a `.vue`
  file, unless the entire `.vue` file is marked as a test file).
- **Bare `test/` or `tests/` directories** when neither a matching build file nor a test runner
  config file is present.


## Where the count appears

- **Terminal & Markdown reports:** A `tests` row appears under each language that has tests and
  under the total, showing its share of `Files %` and `Lines %`.
  - `--tests-breakdown share` (default): shows `tests` as a subset under the language row.
  - `--tests-breakdown split`: shows `production` and `tests` rows side by side under each
    language and under the total.
  - `--tests-breakdown removed`: subtracts test code from every row and hides `tests` rows
    completely.
  - `--hide tests`: disables test detection and hides all test rows.
- **JSON output (`--output json`):** Adds `scope.tests_detected`, `scope.test_presets`,
  `languages[].tests`, `total.tests`, `modules[].total.tests`, and `languages[].by_file[].tests`.
- **Comparisons (`--diff`):** Compares test counts alongside total counts. If a test preset applies
  on only one side of a diff, the report emits a `preset-on-one-side` note.
- **Line-by-line explanation (`--explain`):** Labels every test line as `test code`, prints the test
  total in the summary, and states the exact rule (build file, preset, `--tests` pattern, module
  declaration, or annotation) that marked a whole file as test code.
