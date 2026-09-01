# Benchmark results

Written by `benchmark.py` after every run, not edited by hand. What every term means and how this was measured: the two sections at the bottom.

## linux corpus, WSL2, 20260901-201708

AMD Ryzen 7 9700X 8-Core Processor, 16 threads, 30 GB usable RAM, Ubuntu 24.04.3 LTS  
corpus at `0ff41df1c` on ext4 /dev/sdd, unknown  
mezura v3.0.0 (unreleased), scc 4.0.0, tokei 14.0.0  
3 warmups, 30 timed runs per command (15 in the first pass + 15 in the reverse pass), 3 s of pause before each command

#### Same work (all three pinned to the same languages and settings)

| tool | wall | vs fastest | user cpu | system cpu | parallelism | lines/s | lines per cpu second | files | lines |
|---|---|---|---|---|---|---|---|---|---|
| mezura | 241 ms ± 9 | 1.00x | 2.22 s | 0.53 s | 11.4 | 149.4M | 13.1M | 63,864 | 36,036,878 |
| tokei | 476 ms ± 4 | 1.97x | 5.58 s | 0.83 s | 13.45 | 75.6M | 5.6M | 63,782 | 36,022,156 |
| scc | 603 ms ± 14 | 2.50x | 5.92 s | 0.94 s | 11.38 | 59.8M | 5.3M | 63,724 | 36,013,098 |

#### Out of the box (each tool at its own defaults)

| tool | wall | vs fastest | user cpu | system cpu | parallelism | lines/s | lines per cpu second | files | lines |
|---|---|---|---|---|---|---|---|---|---|
| mezura | 277 ms ± 9 | 1.00x | 2.61 s | 0.55 s | 11.4 | 129.3M | 11.3M | 66,539 | 35,816,820 |
| tokei | 533 ms ± 9 | 1.93x | 6.24 s | 0.91 s | 13.4 | 75.0M | 5.6M | 83,843 | 39,992,832 |
| scc | 847 ms ± 20 | 3.06x | 9.10 s | 1.12 s | 12.08 | 47.2M | 3.9M | 83,784 | 39,993,947 |

Trust checks for this run:
- **Machine steadiness**: the same binary, timed at the start of the run and again at the end, differed by 0.9%.
- **Command order**: every table ran in both command orders and the numbers above pool the two. Swapping the order moved no tool by more than 3.0%.
- **Power**: the machine was measured as it was, with no settings changed.
- **Quiet machine**: everything other than the benchmark was using 0.0% of the cpu when the run started, about 0.0 of 16 cores.
- **hyperfine warning**: t1-rev: Statistical outliers were detected.

## linux corpus, Windows, 20260901-195326

AMD Ryzen 7 9700X 8-Core Processor, 16 threads, 62 GB usable RAM, Windows-11-10.0.26200-SP0  
corpus at `0ff41df1c` on NTFS, Lexar SSD NQ790 2TB, SSD, NVMe  
mezura v3.0.0 (unreleased), scc 4.0.0, tokei 14.0.0  
3 warmups, 30 timed runs per command (15 in the first pass + 15 in the reverse pass), 3 s of pause before each command

#### Same work (all three pinned to the same languages and settings)

| tool | wall | vs fastest | user cpu | system cpu | parallelism | lines/s | lines per cpu second | files | lines |
|---|---|---|---|---|---|---|---|---|---|
| mezura | 343 ms ± 20 | 1.00x | 2.30 s | 2.24 s | 13.23 | 104.9M | 7.9M | 63,893 | 36,035,852 |
| tokei | 674 ms ± 66 | 1.96x | 6.17 s | 3.07 s | 13.71 | 53.4M | 3.9M | 63,811 | 36,021,160 |
| scc | 796 ms ± 18 | 2.32x | 5.88 s | 2.89 s | 11.01 | 45.2M | 4.1M | 63,753 | 36,012,072 |

#### Out of the box (each tool at its own defaults)

| tool | wall | vs fastest | user cpu | system cpu | parallelism | lines/s | lines per cpu second | files | lines |
|---|---|---|---|---|---|---|---|---|---|
| mezura | 381 ms ± 21 | 1.00x | 2.72 s | 2.31 s | 13.21 | 94.1M | 7.1M | 66,568 | 35,815,794 |
| tokei | 752 ms ± 28 | 1.98x | 6.83 s | 3.54 s | 13.79 | 53.2M | 3.9M | 83,891 | 39,991,863 |
| scc | 1,007 ms ± 23 | 2.65x | 10.30 s | 3.05 s | 13.25 | 39.7M | 3.0M | 83,832 | 39,992,940 |

Trust checks for this run:
- **Machine steadiness**: the same binary, timed at the start of the run and again at the end, differed by 0.3%.
- **Command order**: every table ran in both command orders and the numbers above pool the two. Swapping the order moved no tool by more than 2.3%.
- **Power**: the cpu was set to its high-performance mode for the run and restored after.
- **Quiet machine**: everything other than the benchmark was using 0.4% of the cpu when the run started, about 0.1 of 16 cores.
- **Antivirus**: real-time protection on, all three tools equally excluded from real-time scanning.

## linux corpus, Native Linux, 20260828-230405

AMD Ryzen 7 9700X 8-Core Processor, 16 threads, 60 GB usable RAM, Debian GNU/Linux 13 (trixie)  
corpus at `0ff41df1c` on ext4 /dev/nvme1n1p3, Lexar SSD NQ790 2TB, 16.0 GT/s PCIe x4  
mezura v3.0.0 (unreleased), scc 4.0.0, tokei 14.0.0  
3 warmups, 30 timed runs per command (15 in the first pass + 15 in the reverse pass), 3 s of pause before each command

#### Same work (all three pinned to the same languages and settings)

| tool | wall | vs fastest | user cpu | system cpu | parallelism | lines/s | lines per cpu second | files | lines |
|---|---|---|---|---|---|---|---|---|---|
| mezura | 223 ms ± 9 | 1.00x | 2.08 s | 0.35 s | 10.91 | 161.3M | 14.8M | 63,864 | 36,036,878 |
| scc | 471 ms ± 3 | 2.11x | 5.74 s | 0.39 s | 13.01 | 76.4M | 5.9M | 63,724 | 36,013,098 |
| tokei | 474 ms ± 3 | 2.12x | 5.64 s | 0.59 s | 13.15 | 76.0M | 5.8M | 63,782 | 36,022,156 |

#### Out of the box (each tool at its own defaults)

| tool | wall | vs fastest | user cpu | system cpu | parallelism | lines/s | lines per cpu second | files | lines |
|---|---|---|---|---|---|---|---|---|---|
| mezura | 258 ms ± 10 | 1.00x | 2.42 s | 0.37 s | 10.81 | 139.1M | 12.9M | 66,536 | 35,816,653 |
| tokei | 521 ms ± 3 | 2.02x | 6.34 s | 0.74 s | 13.59 | 76.8M | 5.6M | 83,843 | 39,992,832 |
| scc | 688 ms ± 3 | 2.67x | 8.92 s | 0.42 s | 13.57 | 58.1M | 4.3M | 83,784 | 39,993,947 |

Trust checks for this run:
- **Machine steadiness**: the same binary, timed at the start of the run and again at the end, differed by 0.3%.
- **Command order**: every table ran in both command orders and the numbers above pool the two. Swapping the order moved no tool by more than 0.6%.
- **Power**: the cpu was set to its high-performance mode for the run and restored after.

## Every run

Same-work times, the sections above show only the latest run per platform. Commits, machine state and everything else: inside each run's directory.

| run | platform | corpus | mezura | scc | tokei | machine steadiness |
|---|---|---|---|---|---|---|
| [20260901-201708](linux/wsl/20260901-201708/) | WSL2 | linux | 241 ms | 603 ms | 476 ms | 0.9% |
| [20260901-195326](linux/windows/20260901-195326/) | Windows | linux | 343 ms | 796 ms | 674 ms | 0.3% |
| [20260829-072844](linux/wsl/20260829-072844/) | WSL2 | linux | 257 ms | 657 ms | 531 ms | 3.4% |
| [20260829-050102](linux/windows/20260829-050102/) | Windows | linux | 346 ms | 794 ms | 661 ms | 1.3% |
| [20260828-230405](linux/linux/20260828-230405/) | Native Linux | linux | 223 ms | 471 ms | 474 ms | 0.3% |

## Methodology

- hyperfine, with no shell in between. Each section above states its own warmups, timed runs and pause.
- The machine is restarted and otherwise idle, and the run samples the system-wide cpu before it measures anything, which is the quiet machine trust check above. The harness's `noise` command answers the same question in fifteen seconds, before committing to a run.
- Every table is measured twice, in one command order and then in the reverse. The numbers shown average the two, and how far they disagreed is printed in each run's trust checks.
- A corpus definition pins a commit, and a checkout on any other commit refuses to run. A run on an unpinned tree says so beside its corpus line.
- Counts come from each tool's own JSON output.
- Same work: one language set for all three, generated and minified files counted by all, gitignore obeyed by all, any extra feature like keyword counting and complexity analysis turned off. The files and lines columns prove it held.
- Out of the box: bare `tool <dir>`, nothing else.
- The exact flags: [the harness README](../README.md).

## Terms

- **wall**: how long a run takes on the clock, in milliseconds: the mean of all the timed runs, both command orders together, ± their σ. That σ holds the run-to-run noise plus half the gap between the two orders.
- **vs fastest**: this tool's wall divided by the fastest tool's wall in the same table.
- **user cpu**: cpu seconds spent running the tool's own code, summed over every thread. 16 threads busy for one second is 16 s.
- **system cpu**: cpu seconds spent inside the operating system on the tool's behalf, opening and reading files, plus whatever sits on that path (antivirus, filter drivers).
- **parallelism**: user plus system cpu, divided by wall: 4.6 s of cpu inside a 0.35 s run means 13 threads were busy on average.
- **lines/s**: the lines this tool itself counted, divided by its wall time.
- **lines per cpu second**: the lines this tool counted, divided by its user plus system cpu. How cheaply it counts, with the number of cores taken out of the picture.
- **files / lines**: what the tool reported counting. Under "Same work" the three must nearly agree. Out of the box they differ by design. The same commit checks out a few files differently per platform, so counts across two sections differ by a fraction of a percent.
- **machine steadiness**: the same binary timed at the start and at the end of the whole run. The percentage is how far apart the two means came out.
