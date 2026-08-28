| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 471.9 ± 2.4 | 467.3 | 476.1 | 1.05 ± 0.01 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config --file-process-job-workers 32` | 452.6 ± 2.5 | 447.4 | 456.2 | 1.01 ± 0.01 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config --file-process-job-workers 64` | 450.1 ± 3.2 | 445.1 | 458.3 | 1.00 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config --file-process-job-workers 32 --directory-walker-job-workers 16` | 454.1 ± 3.3 | 449.8 | 460.3 | 1.01 ± 0.01 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config --file-process-job-workers 64 --directory-walker-job-workers 16` | 450.4 ± 2.8 | 446.0 | 456.1 | 1.00 ± 0.01 |
