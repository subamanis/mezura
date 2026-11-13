| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/linebench-counters/tokei.exe D:/dev/bench-corpora/linux -t "C,C Header,GNU Style Assembly,Assembly,Python,Perl,Perl,Rust,Shell"` | 664.5 ± 28.4 | 618.9 | 709.7 | 3.19 ± 0.24 |
| `D:/dev/tools/linebench-counters/scc.exe D:/dev/bench-corpora/linux -i c,h,s,asm,py,pl,pm,rs,sh` | 624.3 ± 20.6 | 591.3 | 668.5 | 3.00 ± 0.21 |
| `D:/dev/tools/linebench-counters/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,asm,py,pl,pm,rs,sh` | 208.0 ± 12.9 | 193.4 | 234.6 | 1.00 |
| `D:/dev/tools/linebench-counters/cs.exe D:/dev/bench-corpora/linux --ext c,h,s,asm,py,pl,pm,rs,sh` | 232.7 ± 13.5 | 219.8 | 262.9 | 1.12 ± 0.09 |
