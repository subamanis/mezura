| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/linebench-counters/tokei.exe D:/dev/bench-corpora/cpython -t "Python,C,C Header,HTML,JavaScript,Batch,Shell,Objective-C"` | 69.4 ± 1.1 | 67.4 | 71.1 | 2.94 ± 0.14 |
| `D:/dev/tools/linebench-counters/scc.exe D:/dev/bench-corpora/cpython -i py,c,h,html,js,bat,sh,m` | 49.0 ± 1.5 | 46.9 | 51.5 | 2.08 ± 0.11 |
| `D:/dev/tools/linebench-counters/mezura.exe D:/dev/bench-corpora/cpython --languages py,c,h,html,js,bat,sh,m` | 27.2 ± 0.7 | 26.0 | 28.2 | 1.15 ± 0.06 |
| `D:/dev/tools/linebench-counters/cs.exe D:/dev/bench-corpora/cpython --ext py,c,h,html,js,bat,sh,m` | 23.6 ± 1.1 | 22.1 | 25.8 | 1.00 |
