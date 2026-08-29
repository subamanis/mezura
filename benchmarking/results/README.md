# Benchmark results

Written by `benchmark.py` after every run, not edited by hand. What every term means and how this was measured: the two sections at the bottom.

## linux corpus, Windows, 20260829-050102

AMD Ryzen 7 9700X 8-Core Processor, 16 threads, 62 GB usable RAM, Windows-11-10.0.26200-SP0  
corpus at `0ff41df1c` on NTFS, Lexar SSD NQ790 2TB, SSD, NVMe

### Same work (all three pinned to the same languages and settings)

| tool | wall | vs fastest | total cpu | parallelism | lines/s | files | lines |
|---|---|---|---|---|---|---|---|
| mezura | 346 ms ± 22 | 1.00x | 4.55 s | 13.13 | 104M | 63,893 | 36,035,852 |
| tokei | 661 ms ± 35 | 1.91x | 9.15 s | 13.85 | 55M | 63,811 | 36,021,160 |
| scc | 794 ms ± 17 | 2.29x | 8.74 s | 11.02 | 45M | 63,753 | 36,012,072 |

### Out of the box (each tool at its own defaults)

| tool | wall | vs fastest | total cpu | parallelism | lines/s | files | lines |
|---|---|---|---|---|---|---|---|
| mezura | 377 ms ± 23 | 1.00x | 4.98 s | 13.19 | 95M | 66,565 | 35,815,627 |
| tokei | 757 ms ± 35 | 2.01x | 10.40 s | 13.74 | 53M | 83,891 | 39,991,863 |
| scc | 1,007 ms ± 21 | 2.67x | 13.33 s | 13.24 | 40M | 83,832 | 39,992,940 |

Trust checks for this run:
- **Machine steadiness**: the same binary, timed at the start of the run and again at the end, differed by 1.3%.
- **Command order**: every table ran in both command orders and the numbers above pool the two. Swapping the order moved no tool by more than 2.1%.
- **Power**: the cpu was set to its high-performance mode for the run and restored after.
- **Antivirus**: real-time protection on, all three tools equally excluded from real-time scanning.

## linux corpus, Native Linux, 20260828-230405

AMD Ryzen 7 9700X 8-Core Processor, 16 threads, 60 GB usable RAM, Debian GNU/Linux 13 (trixie)  
corpus at `0ff41df1c` on ext4 /dev/nvme1n1p3, Lexar SSD NQ790 2TB, 16.0 GT/s PCIe x4

### Same work (all three pinned to the same languages and settings)

| tool | wall | vs fastest | total cpu | parallelism | lines/s | files | lines |
|---|---|---|---|---|---|---|---|
| mezura | 223 ms ± 9 | 1.00x | 2.44 s | 10.91 | 161M | 63,864 | 36,036,878 |
| scc | 471 ms ± 3 | 2.11x | 6.13 s | 13.01 | 76M | 63,724 | 36,013,098 |
| tokei | 474 ms ± 3 | 2.12x | 6.23 s | 13.15 | 76M | 63,782 | 36,022,156 |

### Out of the box (each tool at its own defaults)

| tool | wall | vs fastest | total cpu | parallelism | lines/s | files | lines |
|---|---|---|---|---|---|---|---|
| mezura | 258 ms ± 10 | 1.00x | 2.78 s | 10.81 | 139M | 66,536 | 35,816,653 |
| tokei | 521 ms ± 3 | 2.02x | 7.08 s | 13.59 | 77M | 83,843 | 39,992,832 |
| scc | 688 ms ± 3 | 2.67x | 9.34 s | 13.57 | 58M | 83,784 | 39,993,947 |

Trust checks for this run:
- **Machine steadiness**: the same binary, timed at the start of the run and again at the end, differed by 0.3%.
- **Command order**: every table ran in both command orders and the numbers above pool the two. Swapping the order moved no tool by more than 0.6%.
- **Power**: the cpu was set to its high-performance mode for the run and restored after.

## Methodology

- hyperfine, no shell: 3 warmups, 15 timed runs per command, 3 s pause between command series.
- The machine is restarted and otherwise idle. `--noise` verifies the background and the workload spread before anything is measured.
- Every table is measured twice, in one command order and then in the reverse. The numbers shown average the two, and how far they disagreed is printed in each run's trust checks.
- The corpus is pinned to a commit. A checkout on any other commit refuses to run.
- Counts come from each tool's own JSON output.
- Same work: one language set for all three, generated and minified files counted by all, gitignore obeyed by all, any extra feature like keyword counting and complexity analysis turned off. The files and lines columns prove it held.
- Out of the box: bare `tool <dir>`, nothing else.
- The exact flags: [the harness README](../README.md).

## Terms

- **wall**: how long a run takes on the clock, mean ± σ over the timed runs, in milliseconds.
- **vs fastest**: this tool's wall divided by the fastest tool's wall in the same table.
- **total cpu**: seconds of processor time, summed over every thread, user plus kernel. 16 threads busy for one second is 16 s.
- **parallelism**: cpu seconds divided by wall seconds: 4.6 s of cpu inside a 0.35 s run means 13 threads were busy on average.
- **lines/s**: the lines this tool itself counted, divided by its wall time.
- **files / lines**: what the tool reported counting. Under "Same work" the three must nearly agree. Out of the box they differ by design.
- **machine steadiness**: the same binary timed at the start and at the end of the whole run. The percentage is how far apart the two means came out.
