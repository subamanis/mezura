| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/jdk -t "Java,C++,C++,C++ Header,C++ Header,C,C Header,Shell"` | 307.8 ± 2.9 | 304.0 | 313.1 | 5.89 ± 0.08 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/jdk -i java,cpp,cc,hpp,hh,c,h,sh` | 165.2 ± 9.5 | 160.3 | 198.1 | 3.16 ± 0.18 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/jdk --languages java,cpp,cc,hpp,hh,c,h,sh` | 52.3 ± 0.6 | 51.5 | 53.4 | 1.00 |
| `/home/petros/Documents/dev/tools/cs /home/petros/Documents/dev/bench/jdk --ext java,cpp,cc,hpp,hh,c,h,sh` | 54.4 ± 0.3 | 53.7 | 55.0 | 1.04 ± 0.01 |
