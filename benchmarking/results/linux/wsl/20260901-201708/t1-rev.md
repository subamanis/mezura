| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/dev/tools/tokei /home/petros/dev/bench/linux -t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"` | 476.3 ± 3.4 | 472.5 | 484.1 | 1.98 ± 0.06 |
| `/home/petros/dev/tools/scc /home/petros/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 602.7 ± 18.0 | 586.1 | 656.0 | 2.50 ± 0.10 |
| `/home/petros/dev/tools/mezura /home/petros/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --no-heuristics` | 241.1 ± 7.0 | 230.9 | 252.6 | 1.00 |
