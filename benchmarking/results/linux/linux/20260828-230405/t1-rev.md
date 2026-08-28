| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/linux -t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"` | 473.6 ± 2.9 | 468.5 | 479.0 | 2.11 ± 0.09 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 471.7 ± 1.5 | 469.1 | 474.3 | 2.11 ± 0.08 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region` | 224.1 ± 9.0 | 213.0 | 241.5 | 1.00 |
