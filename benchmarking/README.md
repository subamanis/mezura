# Benchmarking

`benchmark.py` measures mezura against scc and tokei on the Linux kernel tree, pinned at commit
`0ff41df1cb268fc69e703a08a57ee14ae967d0ca`. One file, and it detects Linux, WSL, macOS or
Windows environment on its own.

## Prerequisites

Python 3.9+, git, cargo, and [hyperfine](https://github.com/sharkdp/hyperfine) on PATH
(Debian 12 ships 1.15, the minimum). Everything but hyperfine is already on a machine that
builds mezura:

| system | command |
|---|---|
| Debian, Ubuntu | `sudo apt install hyperfine` |
| Fedora, RHEL | `sudo dnf install hyperfine` |
| Arch | `sudo pacman -S hyperfine` |
| openSUSE | `sudo zypper install hyperfine` |
| Alpine | `apk add hyperfine` |
| Windows | `winget install sharkdp.hyperfine` |
| macOS | `brew install hyperfine` |
| anywhere else | `cargo install hyperfine` |

## Running it

First time on a machine

```
python3 benchmark.py setup
```

`setup` fetches the scc binary for this platform, builds tokei, clones the kernel at the
pinned commit and builds mezura from this repo. It skips whatever is already there, rebuilds
mezura either way, and finishes by running `check` and stopping. Measuring is a second
invocation, about three minutes on a quiet machine. On Linux and macOS:

```
sudo python3 benchmark.py run
```

On Windows the same command, from a terminal opened with "Run as administrator":

```
python benchmark.py run
```

Elevated, it sets the cpu governor to `performance` (Linux) or the power scheme to High
performance (Windows), and puts it back when the run ends, whether it finishes, fails or is
interrupted. Unelevated it prints what it would have changed and the command to rerun with,
then asks before doing any work. `--yes` answers that question, `--no-prep` skips the whole
thing even when elevated, and with no terminal attached it carries on rather than hanging.
What was applied is recorded in `run.json` under `settings.machine_prepared`.

`sudo` resets `HOME`, so on Linux and macOS the paths are resolved from `SUDO_USER` rather
than from root.

The five commands are `run`, `setup`, `check`, `noise` and `report`. A bare invocation prints
the help, and `benchmark.py <command> --help` lists that command's own flags.

## Paths

Copy `benchmark.conf.example` to `benchmark.conf` (that exact name, beside `benchmark.py`, gitignored) and edit:

```
tools  = C:/bench/tools
corpus = C:/bench/linux
```

| setting | flag | environment | default |
|---|---|---|---|
| `tools` | `--tools` | `MEZURA_BENCH_TOOLS` | none, must be set |
| `corpus` | `--corpus` | `MEZURA_BENCH_CORPUS` | none, must be set |
| `out` | `--out` | | `benchmarking/results` |

A flag beats an environment variable, which beats the file. With neither set, every command
refuses with the recipe, `check` included, since the check answers "am I ready" and without
the locations there is nothing to be ready about. The exception is `report`, which needs only
the recorded runs.

Keep the corpus and the tools on a local disk. Measuring across `/mnt` from WSL, or over a
network share, measures the mount.

## check

```
python3 benchmark.py check
```

Answers "is this machine ready to measure". It runs each of the six tool invocations once
against the real corpus, reads the counts back out, and proves hyperfine works. A few
seconds, and it leaves no files behind:

```
== check: linux at /home/petros/Documents/dev/bench/linux
   mezura  t1  ok       63,864 files      36,036,878 lines
   scc     t1  ok       63,724 files      36,013,098 lines
   tokei   t1  ok       63,782 files      36,022,156 lines
   ...

   hyperfine     ok

all good.
```

 It goes through the same gate a real run does (paths, binaries, hyperfine, and the
corpus sitting on the commit its definition pins), and then
runs the tools, parses what they printed, and looks at the counts. On Windows it also reports
the MS Defender state, and unequal exclusions fail the check exactly as they refuse the run.

A count of zero fails the check. It means the definition names a language the tree does not
have:

```
   tokei   t1  counted nothing at all, so its share of the linux definition names no
                language this tree has
```

## noise

```
python3 benchmark.py noise
```

Answers "is this machine steady enough to benchmark right now", in about fifteen seconds and
leaving no files behind. It samples the system-wide cpu for a few seconds
with nothing of ours running, which is how many cores other processes are using, then runs the
real workload five times, the first one cold on purpose. It reports the spread across the warm
runs, the parallelism the workload reached, and whether that first run shows the tree was cold.
The verdict comes from the worse of the background and the spread, and names the number that
caused it. `steady` and `relatively steady` exit 0, `somewhat unsteady` and `not steady`
exit 1, so a script can gate on it.

| | steady | relatively steady | somewhat unsteady | not steady |
|---|---|---|---|---|
| background | < 0.75 cores | 0.75 to 1.5 | 1.5 to 3 | 3 cores and up |
| spread | < 5% | 5 to 10% | 10 to 15% | 15% and up |

The background is judged in cores' worth of other work, so one set of thresholds means the same
thing on a laptop and on a 32-core server. The percentage of the whole machine is printed beside
it and is what goes into `run.json`.

A real run samples the background the same way before it measures anything, and records it in
`run.json`, so every published table can say how quiet the machine was.

If it says several cores are busy on a machine you believe is idle, find that process before
trusting any number.

## What gets counted

A corpus is a file under `corpora/`. `linux.conf` is the one this project is measured against:

```
name      = linux
remote    = https://github.com/torvalds/linux.git
commit    = 0ff41df1cb268fc69e703a08a57ee14ae967d0ca
languages = c,h,s,py,pl,rs,sh
types     = C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell
```

Only what genuinely differs between one tree and another lives here. `languages` is what
mezura and scc are told to count, as extensions, and `types` is the same set as tokei spells
it, which is language names. Both are needed because the tools do not agree on how a language
is named. The flags that make the three do *equal* work (mezura counting by region with
keywords hidden, scc without complexity or cocomo) are properties of the benchmark rather than
of the corpus, so they live in `benchmark.py` and cannot drift between definitions.

`--corpus-def <name>` picks another file under `corpora/`, or takes a path if what you give it
has a separator in it. A definition with a `commit` is checked before every run and every
`check`, not only under `setup`: a checkout sitting on anything else is refused, with the
command that puts it right. Leave `commit` blank to measure a tree as it stands, and the run
is then recorded with `corpus_pinned: false` and can never be quietly compared with a pinned
one.

`remote` is only needed to fetch. A tree you already keep at the right commit needs none, and
the setup takes it as it stands. With a remote and a blank commit, the setup clones the
default branch when the corpus is not there yet and otherwise leaves the checkout alone. The
one case nothing can fix by itself is a checkout on the wrong commit with no remote to fetch
the right one from, and that is refused.

Only the *path* to a corpus is machine-local. Its identity is part of the definition.

## Where results go

A finished run prints its two tables and any hyperfine warning in the terminal, so the answer
is on screen before anything below is opened.

```
results/
├── README.md
├── linux/linux/20260828-193938/
├── linux/windows/20260828-214501/
└── chromium/wsl/20260829-101122/
```

One directory per corpus, then per platform, then per run, named by timestamp to the second.
Nothing is ever overwritten, and a run that would collide refuses.

`results/README.md` is the human-facing page, rewritten after every run and on demand with
`report`. It shows the latest run per corpus and platform, its machine and settings, the two
tables and the trust checks, and links every run once there is more than one to link to.

Inside a run directory:

- `run.json`: machine, settings, every measurement, every count. Self-contained, and the one
  to read back and compare across platforms. The results page is generated from these.
  To read one by eye: `python3 -m json.tool run.json`
- `summary.csv`, `counts.csv`: the same numbers, flat
- `machine.txt`: what the run was measured on
- `<phase>.md` / `<phase>.json`: hyperfine's own output per phase
- `transcript.txt`: every line this script printed from phase 0 on, hyperfine's warnings and
  the two summary tables included. The machine preparation and Defender state are in
  `run.json` and `notes.md`
- `notes.md`: the checklist to fill in by hand

`out/` holds the raw tool output and is deleted once the numbers are read, unless `--keep-raw`
says otherwise. It is gitignored either way, because tokei's per-file JSON alone is 40 MB.

## Reading the numbers

Each run measures two tables, and they answer different questions:

- **t1** pins all three tools to the same languages and the same counting model
  (`--counting region`, which is how scc and tokei count). Same work, so the ratio is a speed
  ratio. This is the number that survives scrutiny.
- **t2** runs all three out of the box. mezura's default is `--counting content`, which asks a
  different question of each line, and the default language sets differ too. The tools count
  different amounts here, so the ratio mixes speed with how much work each one did.

Every command in a table is measured twice, once in each command order, and the published mean
and spread cover all of both passes. How far the two orders disagreed is its own trust check
on the page.

Beside the times, `summary.csv` carries `parallelism`, (user+system)/wall, and
`lines_per_cpu_s`, the lines counted per second of cpu. The first belongs to the thread
architecture on that OS, the second to the algorithm, and the pair is what to read when a
number looks odd. `counts.csv` carries a `model` column and names its third bucket, because
`blanks` under the region model and `extra` under the content model are not the same quantity.

**Threads.** Each tool runs at its own default thread settings, which was measured to be the
right call: scc's best setting (`--file-process-job-workers 64`) gains 4.6% on native Debian
and about 1% on Windows, changing no ranking, tokei's rayon pool already takes every core, and
mezura's own sweep came out inside its noise. On different hardware these are different
numbers and would need measuring there.

## Comparing runs across machines

Two runs are comparable only if `mezura_head` matches and `mezura_clean` is true in both.
`mezura_clean` asks whether the binary can be traced back to that commit, so it turns false
only for changed tracked files that reach the build, meaning the workspace members and the two
Cargo manifests. Untracked files never count. Tracked edits outside the build, this script and
the documents included, leave it true and are listed under `mezura_changed_beside_the_build`,
while the ones that dirtied it are under `mezura_changed_before_building`. `corpus_clean`
counts every stray file, since a stray file in the corpus does get counted.

Check `drift` before anything else. The same mezura binary is measured at the start of the run
and again at the end, and the ratio of those two means says whether the machine moved under
the minutes in between.

On Windows the run also records the Defender state: real-time protection, whether the corpus
sits under an exclusion path, and per tool whether its process is excluded from scanning and
whether its binary sits under an excluded path. **Unequal exclusions refuse the run outright**,
because files opened by an excluded process are never scanned in real time, and the comparison
would measure who escaped the antivirus rather than who counts faster.
`--allow-unequal-exclusions` measures anyway, and then the run record, the notes and the
results page all carry a bold warning that the results may not represent real performance.
Reading the exclusion lists needs an elevated shell. Unelevated, the record says
`unknown (needs admin)`, and when Defender itself cannot be queried it says that instead.

## Tests

```
python3 test_benchmark.py
```

Covers the parsing that has already bitten once or can: the git porcelain shapes behind the
provenance fields, the three tool-JSON readers (including tokei's grand-total key, which must
be read once and not summed in again), the Defender `N/A` placeholder trap, and the drift
arithmetic.
