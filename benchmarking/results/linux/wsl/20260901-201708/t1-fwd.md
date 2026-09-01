| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/dev/tools/mezura /home/petros/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --no-heuristics` | 241.2 ± 9.8 | 225.9 | 265.7 | 1.00 |
| `/home/petros/dev/tools/scc /home/petros/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 602.6 ± 9.6 | 592.1 | 627.3 | 2.50 ± 0.11 |
| `/home/petros/dev/tools/tokei /home/petros/dev/bench/linux -t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"` | 476.2 ± 5.3 | 468.7 | 491.3 | 1.97 ± 0.08 |
