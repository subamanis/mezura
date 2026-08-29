# Benchmarking

`benchmark.py` measures mezura against scc and tokei on the Linux kernel tree. One file, and it
detects Linux, WSL, macOS or Windows on its own, so there is no platform flag to pass.

## Prerequisites

Python 3.9+, [hyperfine](https://github.com/sharkdp/hyperfine) on PATH (Debian 12 ships 1.15,
the minimum), git, and cargo for `--setup`.

```
sudo apt update && sudo apt install -y build-essential git hyperfine python3 curl
```

## Running it

First time on a machine. Not with sudo, or the binaries, `~/.cargo` and `target/` end up owned
by root:

```
python3 benchmark.py --setup
```

`--setup` fetches the scc binary for this platform, builds tokei, clones the kernel at the
pinned commit and builds mezura from this repo. It skips whatever is already there, and
finishes by checking its own work.

The real run, about six minutes on a quiet machine:

```
sudo python3 benchmark.py
```

As root it sets the cpu governor to `performance` (Linux) or the power scheme to High
performance (Windows), and puts it back when the run ends, whether it finishes, fails or is
interrupted. Without root, the first thing it prints is what it would have changed and the
command to rerun with, and it asks before doing any work. `--yes` answers, `--no-prep` skips it
even as root, and with no terminal attached it carries on rather than hanging. What was applied
is recorded in `run.json` under `settings.machine_prepared`.

`sudo` resets `HOME`, so paths are resolved from `SUDO_USER` rather than from root.

## Paths

Where you keep things differs per machine, so it is machine-local settings, and there is no
default on purpose: a default is just somebody else's machine. Copy `benchmark.conf.example`
to `benchmark.conf` (that exact name, beside `benchmark.py`, gitignored) and edit:

```
tools  = C:/bench/tools
corpus = C:/bench/linux
```

| setting | flag | environment | default |
|---|---|---|---|
| `tools` | `--tools` | `MEZURA_BENCH_TOOLS` | none, must be set |
| `corpus` | `--corpus` | `MEZURA_BENCH_CORPUS` | none, must be set |
| `out` | `--out` | | `benchmarking/results` |

A flag beats an environment variable, which beats the file. With neither set, every mode
refuses with the recipe, `--check` included: the check answers "am I ready", and without the
locations there is nothing to be ready about.

Keep the corpus and the tools on a local disk. Measuring across `/mnt` from WSL, or over a
network share, measures the mount.

Other flags: `--setup-only`, `--check`, `--noise`, `--report`, `--keep-raw`, `--no-prep`,
`--yes`, `--corpus-def`, `--warmup`, `--runs`, `--settle`.

## --check

```
python3 benchmark.py --check
```

Three seconds, and it writes nothing at all. It runs each of the six tool invocations once
against the real corpus, reads the counts back out, and proves hyperfine works:

```
== check: linux at /home/petros/Documents/dev/bench/linux
   mezura  t1  ok       63,864 files      36,036,878 lines
   scc     t1  ok       63,724 files      36,013,098 lines
   tokei   t1  ok       63,782 files      36,022,156 lines
   ...

   hyperfine     ok

all good.
```

This is what to run before rebooting, letting the machine settle and committing six minutes to
a real run. It goes through the same gate a real run does (paths, binaries, hyperfine, and the
corpus sitting on the commit its definition pins), and then does what no static check can:
runs the tools, parses what they printed, and looks at the counts.

A count of zero fails the check. It means the definition names a language the tree does not
have, which no static check can see:

```
   tokei   t1  counted nothing at all, so its share of the linux definition names no
                language this tree has
```

`--setup` runs the check automatically when it finishes, and stops rather than measuring if it
fails.

## --noise

```
python3 benchmark.py --noise
```

Fifteen seconds, writes nothing, and answers "is this machine steady enough to benchmark right
now" with facts rather than a score. It samples the system-wide CPU for a few seconds with
nothing of ours running (the absolute signal: how many cores other processes are using), then
runs the real workload five times, the first one taken cold on purpose, and reports the spread
across the warm runs, the parallelism the workload reached ((user+system)/wall against the
core count), and whether the first run shows the tree was cold. The verdict is `steady` or
`not steady` with the number that caused it; the exit code follows, so it can gate a script.

What it deliberately does not do is compare mezura against a copy of itself: that ratio was
measured to stay at 1.00 however loaded the machine is, since load hits both sides equally.
Parallelism and the background sample are the signals that actually move.

Also useful before rebooting into a benchmark session: if `--noise` says several cores are
busy on a machine you believe is idle, find that process before trusting any number.

## What gets counted

A corpus is a file under `corpora/`. `linux.conf` is the one this project is measured against:

```
name      = linux
remote    = https://github.com/torvalds/linux.git
commit    = 0ff41df1cb268fc69e703a08a57ee14ae967d0ca
languages = c,h,s,py,pl,rs,sh
types     = C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell
```

Only what genuinely differs between one tree and another lives here. `languages` is what mezura
and scc are told to count, as extensions; `types` is the same set as tokei spells it, which is
language names. Both are needed because the tools do not agree on how a language is named.

The flags that make the three tools do *equal* work (mezura counting by region with keywords
hidden, scc without complexity or cocomo) are properties of the benchmark, not of the corpus,
so they stay in `benchmark.py` and cannot drift between definitions.

`--corpus-def <name>` picks another file under `corpora/`, or takes a path if what you give it
has a separator in it. A definition with a `commit` is checked before every run and before
every `--check`, not only under `--setup`: a checkout sitting on anything else is refused, with
the command that puts it right. Leave `commit` blank to measure a tree as it stands; the run is
then recorded with `corpus_pinned: false` and can never be quietly compared with a pinned one.

`remote` is only needed to fetch. A tree you already keep locally at the right commit needs no
remote at all: the setup takes it as it stands. It is refused only when the checkout is on the
wrong commit and there is nothing to fetch the right one from, which is the one case nothing
can fix by itself.

Only the *path* to a corpus is machine-local. Its identity is part of the definition.

## Where results go

```
results/
├── README.md
├── linux/linux/20260828-193938/
├── linux/windows/20260828-214501/
└── chromium/wsl/20260829-101122/
```

One directory per corpus, then per platform, then per run, named by timestamp to the second.
Nothing is ever overwritten, and a run that would collide refuses rather than clobber.

Each run's `run.json` is the single source of truth. The results page is generated from
those, nothing else, so it can never disagree with them.

`results/README.md` is the human-facing page, rewritten after every run (or on demand with
`--report`): per corpus and platform the latest run's machine line, the two tables with wall,
cpu, parallelism and lines/s, and the trust checks in plain words, then the catalog of every
run with links. GitHub renders it as the folder's front page.

Inside a run directory:

- `run.json`: machine, settings, every measurement, every count. Self-contained; this is the
  one to read back and to compare across platforms. It opens with `format: 1`: fields may be
  added under the same number, and anything that changes the meaning of an existing field
  bumps it.
- `summary.csv`, `counts.csv`: the same numbers, flat
- `machine.txt`: what the run was measured on
- `<phase>.md` / `<phase>.json`: hyperfine's own output per phase
- `transcript.txt`: everything that was printed, this script and the tools alike
- `notes.md`: the checklist to fill in by hand

`out/` holds the raw tool output and is deleted once the numbers are read; `--keep-raw` keeps
it. It is gitignored either way, because tokei's per-file JSON alone is 40 MB.

## Comparing runs across machines

Two runs are comparable only if `mezura_head` matches and `mezura_clean` is true in both. A
dirty tree means the binary cannot be traced back to a commit, so the two sides may not have
measured the same code. `mezura_clean` ignores untracked files, since they never reach the
build; `corpus_clean` counts them, since a stray file in the corpus does get counted.

Check `drift` before anything else: the same mezura binary is measured at the start and at the
end of the run, and the ratio of those two means says whether the machine moved under the six
minutes in between. Comparing releases is done afterwards, across the recorded runs.
(A byte-identical-copy check used to run beside this; it was removed 2026-08-29 after the
mechanism it guarded against, a per-file antivirus verdict, was measured not to exist: the
penalty follows the process name alone, and two same-named copies cannot differ by it.)

On Windows the run also records the Defender state: real-time protection, whether the corpus
sits under an exclusion path, and per tool whether its process is excluded from scanning.
**Asymmetric process exclusions refuse the run outright**, because files opened by an excluded
process are never scanned in real time, and the comparison would measure who escaped the
antivirus rather than who counts faster. Reading the exclusion lists needs an elevated shell;
unelevated, the record says `unknown (needs admin)` rather than guessing.

`summary.csv` carries two derived columns beside the times: `parallelism`, (user+system)/wall,
how much of the machine the tool actually harvested, and `lines_per_cpu_s`, how cheaply it
counted. The first is a property of the thread architecture on that OS, the second of the
algorithm, and the pair is what to read when a number looks odd.

Then note which table answers which question:

- **t1** pins all three tools to the same languages and the same counting model
  (`--counting region`, which is how scc and tokei count). Same work, so the ratio is a speed
  ratio. This is the number that survives scrutiny.
- **t2** runs all three out of the box. mezura's default is `--counting content`, which asks a
  different question of each line, and the default language sets differ too. The tools count
  different amounts here, so the ratio mixes speed with how much work each one did.

`counts.csv` carries a `model` column and names its third bucket, because `blanks` under the
region model and `extra` under the content model are not the same quantity.

There are no sweep phases. A sweep result is a fact about one machine and one corpus, banked
in the run that measured it, not something to re-measure per run: in the recorded runs of
2026-08-28/29 (this machine, the pinned linux corpus, Windows, WSL2 and native Debian), scc's
swept best (`--file-process-job-workers 64`) gained 4.7-6% over its default. tokei has no
thread knob, its rayon pool already takes every core; mezura runs at its shipped default. On
different hardware the sweep is a different fact and would need measuring there.

## Tests

```
python3 test_benchmark.py
```

Covers the parsing that has already bitten once or can: the git porcelain shapes behind the
provenance fields, the three tool-JSON readers (including tokei's grand-total key, which must
be read once and not summed in again), the Defender `N/A`-placeholder trap, and the drift
arithmetic.
