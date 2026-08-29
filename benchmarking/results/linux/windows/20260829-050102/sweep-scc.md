| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/counters-benchmark/scc.exe D:/dev/bench-corpora/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 800.0 ± 19.0 | 771.7 | 833.4 | 1.01 ± 0.03 |
| `D:/dev/tools/counters-benchmark/scc.exe D:/dev/bench-corpora/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config --file-process-job-workers 32` | 808.4 ± 21.9 | 779.5 | 864.5 | 1.02 ± 0.03 |
| `D:/dev/tools/counters-benchmark/scc.exe D:/dev/bench-corpora/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config --file-process-job-workers 64` | 793.7 ± 13.1 | 774.0 | 808.5 | 1.00 ± 0.02 |
| `D:/dev/tools/counters-benchmark/scc.exe D:/dev/bench-corpora/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config --file-process-job-workers 32 --directory-walker-job-workers 16` | 796.7 ± 15.7 | 776.0 | 828.4 | 1.01 ± 0.03 |
| `D:/dev/tools/counters-benchmark/scc.exe D:/dev/bench-corpora/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config --file-process-job-workers 64 --directory-walker-job-workers 16` | 791.4 ± 14.0 | 770.0 | 809.6 | 1.00 |
