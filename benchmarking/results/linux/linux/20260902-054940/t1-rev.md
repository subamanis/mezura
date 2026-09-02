| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/linux -t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"` | 473.9 ± 3.0 | 468.8 | 481.0 | 2.07 ± 0.07 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 472.4 ± 2.1 | 468.7 | 477.1 | 2.06 ± 0.07 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --no-heuristics` | 229.3 ± 8.1 | 221.0 | 246.5 | 1.00 |
