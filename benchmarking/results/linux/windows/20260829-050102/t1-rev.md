| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/counters-benchmark/tokei.exe D:/dev/bench-corpora/linux -t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"` | 653.9 ± 21.1 | 628.1 | 688.1 | 1.88 ± 0.12 |
| `D:/dev/tools/counters-benchmark/scc.exe D:/dev/bench-corpora/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 794.5 ± 19.2 | 771.7 | 830.0 | 2.29 ± 0.13 |
| `D:/dev/tools/counters-benchmark/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region` | 347.7 ± 18.7 | 327.2 | 380.0 | 1.00 |
