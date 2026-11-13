| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/linebench-counters/tokei.exe D:/dev/bench-corpora/jdk -t "Java,C++,C++,C++ Header,C++ Header,C,C Header,Shell"` | 468.9 ± 16.5 | 451.5 | 493.1 | 3.10 ± 0.25 |
| `D:/dev/tools/linebench-counters/scc.exe D:/dev/bench-corpora/jdk -i java,cpp,cc,hpp,hh,c,h,sh` | 345.2 ± 18.4 | 331.8 | 383.2 | 2.28 ± 0.20 |
| `D:/dev/tools/linebench-counters/mezura.exe D:/dev/bench-corpora/jdk --languages java,cpp,cc,hpp,hh,c,h,sh` | 151.4 ± 10.8 | 144.2 | 177.2 | 1.00 |
| `D:/dev/tools/linebench-counters/cs.exe D:/dev/bench-corpora/jdk --ext java,cpp,cc,hpp,hh,c,h,sh` | 176.6 ± 12.8 | 169.2 | 210.7 | 1.17 ± 0.12 |
