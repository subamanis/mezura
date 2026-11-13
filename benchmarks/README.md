# Benchmark results

Written by `linebench report` after every run, and rewritten whole each time. One section per machine, and under it the newest run over each corpus, biggest corpus first. A run holding your own build or arguments is kept apart, under its own headings. What every term means and how this was measured: the last two sections.

## Native Linux, AMD Ryzen 7 9700X 8-Core Processor

16 threads, 60 GB usable RAM, Debian GNU/Linux 13 (trixie)

### linux corpus, 20261113-025437

measured 2026-11-13 02:54 UTC by linebench 0.4.0  
corpus at `0ff41df1c` on ext4 /dev/nvme1n1p3, Lexar SSD NQ790 2TB, 16.0 GT/s PCIe x4  
codestats 1.1.0, mezura v3.5.0 (2026-11-12), scc 4.1.0, tokei 15.0.0  
3 warmups, 30 timed runs per command (15 in the first pass + 15 in the reverse pass), 3 s of pause before each command

#### Same work (every counter pinned to the same languages and settings)

| counter | wall | vs fastest | user cpu | system cpu | parallelism | lines/s | lines per cpu second | files | lines |
|---|---|---|---|---|---|---|---|---|---|
| mezura | 67 ms ± 2 | 1.00x | 0.62 s | 0.28 s | 13.54 | 540.7M | 39.9M | 63,743 | 36,019,013 |
| codestats | 79 ms ± 1 | 1.19x ± 0.03 | 0.94 s | 0.28 s | 15.32 | 454.4M | 29.7M | 63,738 | 36,018,801 |
| scc | 208 ms ± 3 | 3.12x ± 0.09 | 2.72 s | 0.37 s | 14.88 | 173.1M | 11.6M | 63,738 | 36,018,801 |
| tokei | 411 ms ± 3 | 6.17x ± 0.15 | 5.57 s | 0.60 s | 15.01 | 87.7M | 5.8M | 63,793 | 36,027,518 |

#### Out of the box (each counter at its own defaults)

| counter | wall | vs fastest | user cpu | system cpu | parallelism | lines/s | lines per cpu second | files | lines |
|---|---|---|---|---|---|---|---|---|---|
| mezura | 85 ms ± 2 | 1.00x | 0.90 s | 0.28 s | 13.82 | 419.0M | 30.3M | 63,442 | 35,732,206 |
| codestats | 86 ms ± 2 | 1.01x ± 0.04 | 1.01 s | 0.30 s | 15.11 | 413.2M | 27.3M | 63,258 | 35,672,682 |
| scc | 290 ms ± 3 | 3.41x ± 0.09 | 4.03 s | 0.37 s | 15.15 | 124.0M | 8.2M | 63,738 | 36,018,801 |
| tokei | 418 ms ± 3 | 4.90x ± 0.13 | 5.64 s | 0.60 s | 14.94 | 86.3M | 5.8M | 63,793 | 36,027,518 |

Trust checks for this run:
- **Machine steadiness**: the same binary, timed at the start of the run and again at the end, differed by 0.3%.
- **Command order**: every table ran in both command orders and the numbers above pool the two. Swapping the order moved no counter by more than 1.0%.
- **Power**: set for the run and restored after: cpu governor on 16 cpus: powersave -> performance.
- **Quiet machine**: everything other than the benchmark was using 0% of the cpu when the run started, about 0.0 of 16 cores.
- **Equal work**: every file count sat within 1.0% of the 63,779 files the corpus declares, and the line counts within 1.0% of each other.
- **Left out**: cloc, because the corpus definition leaves it out on linux.

### jdk corpus, 20261113-025252

measured 2026-11-13 02:52 UTC by linebench 0.4.0  
corpus at `b96680ca9` on ext4 /dev/nvme1n1p3, Lexar SSD NQ790 2TB, 16.0 GT/s PCIe x4  
codestats 1.1.0, mezura v3.5.0 (2026-11-12), scc 4.1.0, tokei 15.0.0  
3 warmups, 30 timed runs per command (15 in the first pass + 15 in the reverse pass), 3 s of pause before each command

#### Same work (every counter pinned to the same languages and settings)

| counter | wall | vs fastest | user cpu | system cpu | parallelism | lines/s | lines per cpu second | files | lines |
|---|---|---|---|---|---|---|---|---|---|
| mezura | 43 ms ± 1 | 1.00x | 0.34 s | 0.26 s | 13.88 | 310.3M | 22.4M | 61,550 | 13,271,450 |
| codestats | 46 ms ± 1 | 1.06x ± 0.03 | 0.43 s | 0.28 s | 15.40 | 290.6M | 18.9M | 61,420 | 13,226,395 |
| scc | 112 ms ± 4 | 2.62x ± 0.12 | 1.35 s | 0.33 s | 14.99 | 118.0M | 7.9M | 61,420 | 13,226,395 |
| tokei | 301 ms ± 4 | 7.03x ± 0.18 | 4.04 s | 0.42 s | 14.85 | 44.0M | 3.0M | 61,421 | 13,226,397 |

#### Out of the box (each counter at its own defaults)

| counter | wall | vs fastest | user cpu | system cpu | parallelism | lines/s | lines per cpu second | files | lines |
|---|---|---|---|---|---|---|---|---|---|
| mezura | 52 ms ± 1 | 1.00x | 0.48 s | 0.25 s | 14.15 | 254.2M | 18.0M | 61,546 | 13,265,825 |
| codestats | 54 ms ± 0 | 1.04x ± 0.01 | 0.51 s | 0.28 s | 14.47 | 241.5M | 16.7M | 61,281 | 13,145,866 |
| scc | 163 ms ± 7 | 3.13x ± 0.14 | 2.12 s | 0.32 s | 14.94 | 81.0M | 5.4M | 61,420 | 13,226,395 |
| tokei | 309 ms ± 4 | 5.91x ± 0.11 | 4.11 s | 0.43 s | 14.72 | 42.9M | 2.9M | 61,417 | 13,226,116 |

Trust checks for this run:
- **Machine steadiness**: the same binary, timed at the start of the run and again at the end, differed by 1.0%.
- **Command order**: every table ran in both command orders and the numbers above pool the two. Swapping the order moved no counter by more than 2.5%.
- **Power**: set for the run and restored after: cpu governor on 16 cpus: powersave -> performance.
- **Quiet machine**: everything other than the benchmark was using 0% of the cpu when the run started, about 0.0 of 16 cores.
- **Equal work**: every file count sat within 1.0% of the 61,420 files the corpus declares, and the line counts within 1.0% of each other.
- **Left out**: cloc, because the corpus definition leaves it out on linux.
- **hyperfine warning**: t1-rev: Statistical outliers were detected.
- **hyperfine warning**: t2-rev: Statistical outliers were detected.

### cpython corpus, 20261113-025144

measured 2026-11-13 02:51 UTC by linebench 0.4.0  
corpus at `34439b8a2` on ext4 /dev/nvme1n1p3, Lexar SSD NQ790 2TB, 16.0 GT/s PCIe x4  
codestats 1.1.0, mezura v3.5.0 (2026-11-12), scc 4.1.0, tokei 15.0.0  
3 warmups, 30 timed runs per command (15 in the first pass + 15 in the reverse pass), 3 s of pause before each command

#### Same work (every counter pinned to the same languages and settings)

| counter | wall | vs fastest | user cpu | system cpu | parallelism | lines/s | lines per cpu second | files | lines |
|---|---|---|---|---|---|---|---|---|---|
| codestats | 7 ms ± 0 | 1.00x | 0.08 s | 0.02 s | 13.15 | 306.5M | 23.3M | 3,554 | 2,229,543 |
| mezura | 8 ms ± 1 | 1.05x ± 0.08 | 0.06 s | 0.02 s | 10.16 | 290.9M | 28.6M | 3,557 | 2,230,162 |
| scc | 20 ms ± 0 | 2.74x ± 0.10 | 0.22 s | 0.03 s | 12.28 | 112.0M | 9.1M | 3,554 | 2,229,543 |
| tokei | 45 ms ± 1 | 6.22x ± 0.24 | 0.61 s | 0.03 s | 14.24 | 49.3M | 3.5M | 3,581 | 2,230,903 |

#### Out of the box (each counter at its own defaults)

| counter | wall | vs fastest | user cpu | system cpu | parallelism | lines/s | lines per cpu second | files | lines |
|---|---|---|---|---|---|---|---|---|---|
| codestats | 8 ms ± 0 | 1.00x | 0.08 s | 0.02 s | 12.02 | 233.3M | 19.4M | 3,335 | 1,879,259 |
| mezura | 10 ms ± 0 | 1.19x ± 0.04 | 0.07 s | 0.02 s | 9.54 | 221.3M | 23.2M | 3,539 | 2,117,231 |
| scc | 26 ms ± 0 | 3.26x ± 0.10 | 0.32 s | 0.03 s | 12.96 | 84.9M | 6.6M | 3,554 | 2,229,543 |
| tokei | 47 ms ± 1 | 5.81x ± 0.19 | 0.62 s | 0.03 s | 13.89 | 47.7M | 3.4M | 3,579 | 2,230,868 |

Trust checks for this run:
- **Machine steadiness**: the same binary, timed at the start of the run and again at the end, differed by 1.8%.
- **Command order**: every table ran in both command orders and the numbers above pool the two. Swapping the order moved no counter by more than 1.4%.
- **Power**: set for the run and restored after: cpu governor on 16 cpus: powersave -> performance.
- **Quiet machine**: everything other than the benchmark was using 0% of the cpu when the run started, about 0.0 of 16 cores.
- **Equal work**: every file count sat within 1.0% of the 3,556 files the corpus declares, and the line counts within 1.0% of each other.
- **Left out**: cloc, because the corpus definition leaves it out on linux.
- **hyperfine warning**: t1-fwd: Statistical outliers were detected.
- **hyperfine warning**: t2-fwd: Statistical outliers were detected.

## Methodology

- hyperfine, with no shell in between. Each section above states its own warmups, timed runs and pause.
- The machine is restarted and otherwise idle, and the run samples the system-wide cpu before it measures anything, which is the quiet machine trust check above. `linebench noise` answers the same question with five runs of the control, before committing to a run.
- Every table is measured twice, in one command order and then in the reverse. The numbers shown average the two, and how far they disagreed is printed in each run's trust checks.
- A corpus definition pins a commit, and a checkout on any other commit refuses to run. A run on an unpinned tree says so beside its corpus line.
- Counts come from each counter's own JSON output, and the file counts are checked against the count the corpus definition declares for its commit, which is the equal work trust check above.
- Same work: one language set for every counter, generated and minified files counted by all, no ignore file read by any of them, and every extra feature turned off. What each one turns off, as its definition declares it:
  - codestats: ignore files off, hidden files counted, generated files counted, linguist attributes off, no configuration file read
  - mezura: ignore files off, dotted directories entered, no default and no project settings read, shebang lines unread, keyword and test counting off, minified, generated and non code files counted, region counting model, content identification left on since scc has no way to turn its own off
  - scc: ignore files off, complexity and cost estimates off, no config file read
  - tokei: ignore files off, hidden files and directories counted, which is everything tokei has beyond the counting
- Out of the box: the same language set for every counter, with each counter at its own defaults and an instance's own arguments where it has them.
- The exact flags: each counter's definition under `counters/` in the linebench repository.

## Terms

- **wall**: how long a run takes on the clock, in milliseconds: the mean of all the timed runs, both command orders together, ± their σ. That σ holds the run-to-run noise plus half the gap between the two orders.
- **vs fastest**: this counter's wall divided by the fastest counter's wall in the same table, ± the σ of that ratio, worked out from the two walls' σ. A ratio whose interval reaches 1.00 is within the noise of the fastest.
- **user cpu**: cpu seconds spent running the counter's own code, summed over every thread. 16 threads busy for one second is 16 s.
- **system cpu**: cpu seconds spent inside the operating system on the counter's behalf, opening and reading files, plus whatever sits on that path (antivirus, filter drivers).
- **parallelism**: user plus system cpu, divided by wall: 4.6 s of cpu inside a 0.35 s run means 13 threads were busy on average.
- **lines/s**: the lines this counter itself counted, divided by its wall time.
- **lines per cpu second**: the lines this counter counted, divided by its user plus system cpu. How cheaply it counts, with the number of cores taken out of the picture.
- **files / lines**: what the counter reported counting. Under "Same work" every counter must nearly agree, and the equal work check says whether they did. Out of the box they differ by design.
- **machine steadiness**: the same binary timed at the start and at the end of the whole run. The percentage is how far apart the two means came out.

Every number here comes out of a record under this folder. `linebench report --verify` says whether this page is the one those records make, and `linebench verify <run directory>` reads one of them back and holds its numbers against each other.
