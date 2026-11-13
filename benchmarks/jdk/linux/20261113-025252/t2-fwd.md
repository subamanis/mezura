| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/cs /home/petros/Documents/dev/bench/jdk --ext java,cpp,cc,hpp,hh,c,h,sh` | 54.5 ± 0.5 | 53.8 | 55.5 | 1.05 ± 0.02 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/jdk --languages java,cpp,cc,hpp,hh,c,h,sh` | 52.1 ± 0.7 | 51.3 | 53.2 | 1.00 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/jdk -i java,cpp,cc,hpp,hh,c,h,sh` | 161.2 ± 1.2 | 159.0 | 162.8 | 3.09 ± 0.04 |
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/jdk -t "Java,C++,C++,C++ Header,C++ Header,C,C Header,Shell"` | 309.3 ± 5.0 | 303.2 | 320.9 | 5.94 ± 0.12 |
