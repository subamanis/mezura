| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/linebench-counters/cs.exe D:/dev/bench-corpora/cpython --ext py,c,h,html,js,bat,sh,m` | 26.0 ± 5.0 | 22.4 | 40.2 | 1.00 |
| `D:/dev/tools/linebench-counters/mezura.exe D:/dev/bench-corpora/cpython --languages py,c,h,html,js,bat,sh,m` | 26.5 ± 0.6 | 25.5 | 27.8 | 1.02 ± 0.20 |
| `D:/dev/tools/linebench-counters/scc.exe D:/dev/bench-corpora/cpython -i py,c,h,html,js,bat,sh,m` | 48.2 ± 1.1 | 46.6 | 50.0 | 1.85 ± 0.36 |
| `D:/dev/tools/linebench-counters/tokei.exe D:/dev/bench-corpora/cpython -t "Python,C,C Header,HTML,JavaScript,Batch,Shell,Objective-C"` | 69.9 ± 2.1 | 67.0 | 75.2 | 2.69 ± 0.52 |
