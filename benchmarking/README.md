# Benchmarking

`benchmark.py` measures mezura against scc and tokei on the Linux kernel tree. One file, and it
detects Linux, WSL, macOS or Windows on its own — there is no platform flag to pass.

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
pinned commit, builds mezura from this repo and makes the control copy. It skips whatever is
already there, and finishes by checking its own work.

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

Where you keep things differs per machine, so it is machine-local settings, not a flag you
retype. Copy `benchmark.conf.example` to `benchmark.conf` (gitignored) and edit:

```
tools  = ~/Documents/dev/tools
corpus = ~/Documents/dev/bench/linux
```

| setting | flag | environment | default |
|---|---|---|---|
| `tools` | `--tools` | `MEZURA_BENCH_TOOLS` | `~/Documents/dev/tools` |
| `corpus` | `--corpus` | `MEZURA_BENCH_CORPUS` | `~/Documents/dev/bench/linux` |
| `out` | `--out` | | `benchmarking/results` |

A flag beats an environment variable, which beats the file, which beats the default.

Keep the corpus and the tools on a local disk. Measuring across `/mnt` from WSL, or over a
network share, measures the mount.

Other flags: `--setup-only`, `--check`, `--keep-raw`, `--no-prep`, `--yes`, `--corpus-def`,
`--warmup`, `--runs`, `--settle`.

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
   control       ok
all good. nothing was written.
```

This is what to run before rebooting, letting the machine settle and committing six minutes to
a real run. It goes through the same gate a real run does — paths, binaries, hyperfine, and the
corpus sitting on the commit its definition pins — and then does what no static check can:
runs the tools, parses what they printed, and looks at the counts.

A count of zero fails the check. It means the definition names a language the tree does not
have, which no static check can see:

```
   tokei   t1  counted nothing at all, so its share of the linux definition names no
                language this tree has
```

The control copy is reported rather than measured. It is missing before the first run and goes
stale whenever mezura is rebuilt, and neither is a failure: the next run copies it over.

`--setup` runs the check automatically when it finishes, and stops rather than measuring if it
fails.

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

The flags that make the three tools do *equal* work — mezura counting by region with keywords
hidden, scc without complexity or cocomo — are properties of the benchmark, not of the corpus,
so they stay in `benchmark.py` and cannot drift between definitions.

`--corpus-def <name>` picks another file under `corpora/`, or takes a path if what you give it
has a separator in it. A definition with a `commit` is checked before every run and before
every `--check`, not only under `--setup`: a checkout sitting on anything else is refused, with
the command that puts it right. Leave `commit` blank to measure a tree as it stands; the run is
then recorded with `corpus_pinned: false` and can never be quietly compared with a pinned one.

`remote` is only needed to fetch. A tree you already keep locally at the right commit needs no
remote at all — the setup takes it as it stands. It is refused only when the checkout is on the
wrong commit and there is nothing to fetch the right one from, which is the one case nothing
can fix by itself.

Only the *path* to a corpus is machine-local. Its identity is part of the definition.

## Where results go

```
results/
├── index.csv
├── linux/linux/20260828-193938/
├── linux/windows/20260828-214501/
└── chromium/wsl/20260829-101122/
```

One directory per corpus, then per platform, then per run, named by timestamp to the second.
Nothing is ever overwritten, and a run that would collide refuses rather than clobber.

`index.csv` gains a row per run: corpus, platform, stamp, which mezura and corpus commit was
measured, whether the machine was prepared, the worst control ratio and the three t1 means. It answers
"what have I got, and which of these are comparable" without opening anything.

Inside a run directory:

- `run.json` — machine, settings, every measurement, every count. Self-contained; this is the
  one to read back and to compare across platforms.
- `summary.csv`, `counts.csv` — the same numbers, flat
- `machine.txt` — what the run was measured on
- `<phase>.md` / `<phase>.json` — hyperfine's own output per phase
- `transcript.txt` — everything that was printed, this script and the tools alike
- `notes.md` — the checklist to fill in by hand

`out/` holds the raw tool output and is deleted once the numbers are read; `--keep-raw` keeps
it. It is gitignored either way, because tokei's per-file JSON alone is 40 MB.

## Comparing runs across machines

Two runs are comparable only if `mezura_head` matches and `mezura_clean` is true in both. A
dirty tree means the binary cannot be traced back to a commit, so the two sides may not have
measured the same code. `mezura_clean` ignores untracked files, since they never reach the
build; `corpus_clean` counts them, since a stray file in the corpus does get counted.

Check both control runs before anything else. They put mezura against a byte-identical copy of
itself, refreshed whenever the two differ, so a `worst_control` far from 1.00 means the machine
moved under the measurement and nothing else in that run can be trusted. The control is never
an older mezura: comparing releases is done afterwards, by reading `index.csv` across runs.

Then note which table answers which question:

- **t1** pins all three tools to the same languages and the same counting model
  (`--counting region`, which is how scc and tokei count). Same work, so the ratio is a speed
  ratio. This is the number that survives scrutiny.
- **t2** runs all three out of the box. mezura's default is `--counting content`, which asks a
  different question of each line, and the default language sets differ too. The tools count
  different amounts here, so the ratio mixes speed with how much work each one did.

`counts.csv` carries a `model` column and names its third bucket, because `blanks` under the
region model and `extra` under the content model are not the same quantity.

The two sweeps are there to keep the two apart. `sweep-mezura` shows how much is on the table
from thread settings alone, so a release that only retuned them is not mistaken for one that
got genuinely faster. `sweep-scc` measures scc at its own best rather than its default, so the
headline is not won by benchmarking it badly.
