# Notes for moving this into its own repo

Written while finishing the proof of concept. Nothing here is urgent; it is what would have to
change for this to stand on its own in the `loc-conformance` org beside linejudge, where scc and
tokei could pick it up for their own benchmarks.

## Throw out the provenance machinery

`build_inputs`, `reaches_the_build`, `git_state` on the repo, `mezura_head`, `mezura_clean`,
`mezura_changed_before_building`, `mezura_changed_beside_the_build`, and `setup_mezura`. Roughly
five functions and sixty lines.

All of it answers one question — which source produced the binary — and that is the measured
project's business, not the harness's. It also answers it weakly: a git commit does not pin a
binary, since the compiler, the flags and the platform all vary underneath it.

Replace with, per binary measured:

- its sha256, which pins the exact artifact that ran
- its `--version` output
- a `--label` the operator can set for anything the harness cannot know

Hashes do not compare across platforms, since a Linux and a Windows build are necessarily
different files. For cross-platform comparison the version string is the only identity a tool
offers, and the label covers the rest. Claiming more than that would be a lie.

## Tool definitions, parallel to corpora/

Today `EQUAL_WORK_MEZURA` / `_SCC` / `_TOKEI`, `pinned_flags`, `read_totals`, `SCC_VERSION`,
`TOKEI_VERSION` and the download logic are hardcoded knowledge of three specific programs, and
mezura holds a privileged position: it is the control, it is always the first command, and t1
and t2 are built around it.

A `tools/<name>.conf` beside `corpora/<name>.conf` would carry:

- the binary name
- how to ask it for JSON
- the flags that make it do work equal to the others
- where the counts sit in its output
- optionally, where to fetch it from

Then a general run measures every declared tool over the declared corpus, with none of them
special, and adding a fourth tool is a file rather than a patch.

Reading counts back out is the fiddly part: it needs something like a path expression per field
rather than the three hand-written parsers in `read_totals`.

## What carries over unchanged

`corpora/`, the machine preparation and its restore, the control runs, the transcript, the
index, the results layout, and the t1/t2 split — which is already tool-agnostic once the flags
come from a definition: t1 is "equal work", t2 is "each at its defaults".

## Measuring whether the machine is steady

Two findings from testing the current `--noise` flag under artificial load, kept here because
the fix belongs in the real tool rather than in this proof of concept.

### The control ratio is blind to load

`--noise`, and the control phases inside a run, compare mezura against a byte-identical copy of
itself. Load hits both equally, so their ratio stays at 1.00 however busy the machine is.
Measured with twelve busy loops running:

```
quiet    255 ms   spread 4.3%   ratio 1.011
loaded   603 ms   spread 7.5%   ratio 1.016
```

The machine was 2.4 times slower, load average 16, and the ratio did not move. Worse, the
verdict compares the ratio against the spread, so a noisier machine makes the test more
lenient, not less.

What the ratio does detect is drift between two measurements taken apart in time. That is worth
keeping, but it is not a load check and must not be presented as one.

### Achieved parallelism is the signal, and it needs no history

`user + system` divided by wall time says how much of the machine the run actually got. The work
is the same either way, so cpu time stays put while wall time stretches:

```
quiet    wall 263.5 ms   user 2368 ms + sys 371 ms   ->  10.40 of 16 cores
loaded   wall 580.1 ms   user 2738 ms + sys 366 ms   ->   5.35 of 16 cores
```

Twice the separation the spread gave, with no stored baseline and no invented threshold: the
reference is `os.cpu_count()`, and "5.35 of your 16 cores" reads on its own.

### A real defect in the current script

`worst_control` compares the two commands *within* each control phase, which is the ratio shown
above to be blind. It never compares `control-start` against `control-end`, which is the pair
that would show the machine drifting over the six minutes of a run. From a real session:

```
control-start  256.7 / 259.1   within phase 1.009
control-end    260.2 / 254.2   within phase 1.024   <- what is reported
start mean 257.9 vs end mean 257.2  ->  1.003       <- what would mean something
```

Under load the second figure would be around 2.35. The data is already in both exports; only the
comparison is wrong. This is the gate the README tells people to check first.

### The control copy may not be earning its place

A `--noise` flag was tried and removed. It compared mezura against its copy back to back, ten
runs each, and was justified as catching position bias between the two slots. At that scale
nothing drifts: ten runs of a 260 ms workload is under three seconds per command, warmup levels
the page cache, and no cpu throttles in that window. The flag measured neither load nor bias.

That undercuts the control copy itself. If what matters is whether the machine moved between
the start and the end of a six-minute run, the same binary measured twice answers it, and the
byte-identical copy adds nothing. Worth settling before the real tool carries it over.

## notes.md asks the human to verify what the script already knows

The checklist written into every run directory is inherited from the bash script, from when
nothing was recorded automatically. Three of its four boxes are now facts in `run.json`:

| box | what already holds the answer |
|---|---|
| corpus and tools on a local disk, not /mnt | `corpus_fs`, `corpus_device` |
| same corpus as the session compared against | `corpus_pinned`, and the run refuses otherwise |
| mezura built from the tree being measured | `mezura_clean`, `mezura_changed_before_building` |
| machine quiet during the run | nothing. `worst_control` is the closest evidence |

Leaving them as empty boxes is worse than redundant: it presents as unverified what the record
has already established, and the second one cannot fail at all, since a run against the wrong
commit never starts.

What would be worth having instead is a file that states what was found and leaves a box only
for what the script cannot know:

```
# Benchmark session notes 20260828-230405

corpus:  linux @ 0ff41df1c, pinned and verified
         /home/petros/Documents/dev/bench/linux
         ext4 on Lexar SSD NQ790 2TB, 16.0 GT/s PCIe x4
mezura:  9b31de9, clean
machine: governor set to performance for the run
         control drift start to end: 1.0065

- [ ] machine quiet during the run

observations:
-
```

That also makes the directory readable on its own, without opening `run.json`, which is most of
what anyone wants when they come back to an old session.
