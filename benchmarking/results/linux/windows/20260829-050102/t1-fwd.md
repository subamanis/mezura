| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/counters-benchmark/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region` | 345.2 ± 25.0 | 323.1 | 398.8 | 1.00 |
| `D:/dev/tools/counters-benchmark/scc.exe D:/dev/bench-corpora/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 793.0 ± 14.9 | 770.5 | 812.3 | 2.30 ± 0.17 |
| `D:/dev/tools/counters-benchmark/tokei.exe D:/dev/bench-corpora/linux -t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"` | 667.7 ± 43.8 | 632.7 | 781.3 | 1.93 ± 0.19 |
