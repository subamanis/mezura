| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/counters-benchmark/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --no-heuristics` | 344.1 ± 23.9 | 318.7 | 396.1 | 1.00 |
| `D:/dev/tools/counters-benchmark/scc.exe D:/dev/bench-corpora/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 794.3 ± 16.2 | 770.3 | 828.7 | 2.31 ± 0.17 |
| `D:/dev/tools/counters-benchmark/tokei.exe D:/dev/bench-corpora/linux -t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"` | 671.8 ± 63.9 | 628.5 | 889.3 | 1.95 ± 0.23 |
