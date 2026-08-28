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
