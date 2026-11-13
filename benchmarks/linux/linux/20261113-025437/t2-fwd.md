| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/cs /home/petros/Documents/dev/bench/linux --ext c,h,s,asm,py,pl,pm,rs,sh` | 86.1 ± 2.0 | 83.1 | 89.6 | 1.01 ± 0.04 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/linux --languages c,h,s,asm,py,pl,pm,rs,sh` | 85.2 ± 2.2 | 80.6 | 88.0 | 1.00 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/linux -i c,h,s,asm,py,pl,pm,rs,sh` | 290.1 ± 3.3 | 286.4 | 298.9 | 3.41 ± 0.10 |
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/linux -t "C,C Header,GNU Style Assembly,Assembly,Python,Perl,Perl,Rust,Shell"` | 418.4 ± 1.7 | 415.6 | 421.2 | 4.91 ± 0.13 |
