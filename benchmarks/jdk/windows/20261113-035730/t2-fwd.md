| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/linebench-counters/cs.exe D:/dev/bench-corpora/jdk --ext java,cpp,cc,hpp,hh,c,h,sh` | 177.1 ± 12.9 | 169.1 | 209.2 | 1.17 ± 0.13 |
| `D:/dev/tools/linebench-counters/mezura.exe D:/dev/bench-corpora/jdk --languages java,cpp,cc,hpp,hh,c,h,sh` | 151.4 ± 13.5 | 143.6 | 188.2 | 1.00 |
| `D:/dev/tools/linebench-counters/scc.exe D:/dev/bench-corpora/jdk -i java,cpp,cc,hpp,hh,c,h,sh` | 346.4 ± 20.1 | 330.9 | 380.7 | 2.29 ± 0.24 |
| `D:/dev/tools/linebench-counters/tokei.exe D:/dev/bench-corpora/jdk -t "Java,C++,C++,C++ Header,C++ Header,C,C Header,Shell"` | 488.2 ± 27.4 | 450.3 | 527.0 | 3.22 ± 0.34 |
