| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/counters-benchmark/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region` | 347.2 ± 24.6 | 320.7 | 394.7 | 1.00 ± 0.10 |
| `D:/dev/tools/counters-benchmark/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --threads 4 64` | 349.2 ± 20.8 | 328.1 | 398.7 | 1.01 ± 0.10 |
| `D:/dev/tools/counters-benchmark/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --threads 16 64` | 349.1 ± 24.0 | 328.0 | 400.8 | 1.01 ± 0.10 |
| `D:/dev/tools/counters-benchmark/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --threads 8 32` | 347.2 ± 26.3 | 320.9 | 402.9 | 1.00 |
| `D:/dev/tools/counters-benchmark/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --threads 8 128` | 352.7 ± 20.2 | 331.5 | 388.2 | 1.02 ± 0.10 |
| `D:/dev/tools/counters-benchmark/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --threads 16 128` | 348.2 ± 23.5 | 326.6 | 393.8 | 1.00 ± 0.10 |
