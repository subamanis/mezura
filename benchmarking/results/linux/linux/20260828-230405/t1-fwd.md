| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region` | 222.7 ± 9.8 | 212.6 | 241.4 | 1.00 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 471.2 ± 4.4 | 468.0 | 485.9 | 2.12 ± 0.09 |
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/linux -t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"` | 474.2 ± 3.0 | 469.7 | 478.8 | 2.13 ± 0.09 |
