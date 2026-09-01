| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/counters-benchmark/tokei.exe D:/dev/bench-corpora/linux -t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"` | 676.1 ± 67.6 | 625.4 | 899.5 | 1.97 ± 0.22 |
| `D:/dev/tools/counters-benchmark/scc.exe D:/dev/bench-corpora/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 798.2 ± 19.4 | 775.5 | 843.5 | 2.33 ± 0.12 |
| `D:/dev/tools/counters-benchmark/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --no-heuristics` | 342.8 ± 15.9 | 317.9 | 371.2 | 1.00 |
