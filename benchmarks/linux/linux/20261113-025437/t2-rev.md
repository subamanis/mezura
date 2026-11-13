| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/linux -t "C,C Header,GNU Style Assembly,Assembly,Python,Perl,Perl,Rust,Shell"` | 416.9 ± 3.1 | 412.9 | 423.4 | 4.88 ± 0.12 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/linux -i c,h,s,asm,py,pl,pm,rs,sh` | 290.7 ± 2.6 | 285.8 | 295.2 | 3.41 ± 0.09 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/linux --languages c,h,s,asm,py,pl,pm,rs,sh` | 85.4 ± 2.0 | 81.7 | 89.1 | 1.00 |
| `/home/petros/Documents/dev/tools/cs /home/petros/Documents/dev/bench/linux --ext c,h,s,asm,py,pl,pm,rs,sh` | 86.6 ± 2.4 | 83.2 | 91.8 | 1.01 ± 0.04 |
