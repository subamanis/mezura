| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/linebench-counters/cs.exe D:/dev/bench-corpora/linux --ext c,h,s,asm,py,pl,pm,rs,sh` | 233.0 ± 13.9 | 218.2 | 266.1 | 1.14 ± 0.10 |
| `D:/dev/tools/linebench-counters/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,asm,py,pl,pm,rs,sh` | 204.0 ± 13.5 | 189.9 | 239.3 | 1.00 |
| `D:/dev/tools/linebench-counters/scc.exe D:/dev/bench-corpora/linux -i c,h,s,asm,py,pl,pm,rs,sh` | 640.1 ± 27.5 | 595.0 | 689.2 | 3.14 ± 0.25 |
| `D:/dev/tools/linebench-counters/tokei.exe D:/dev/bench-corpora/linux -t "C,C Header,GNU Style Assembly,Assembly,Python,Perl,Perl,Rust,Shell"` | 661.9 ± 31.6 | 625.7 | 726.3 | 3.24 ± 0.26 |
