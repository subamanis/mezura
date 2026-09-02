| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --no-heuristics` | 226.8 ± 11.3 | 210.7 | 252.7 | 1.00 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 471.9 ± 2.9 | 468.7 | 478.8 | 2.08 ± 0.10 |
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/linux -t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"` | 473.5 ± 3.8 | 469.1 | 484.6 | 2.09 ± 0.11 |
