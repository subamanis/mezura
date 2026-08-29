| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/dev/tools/mezura /home/petros/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region` | 250.9 ± 9.7 | 237.9 | 274.5 | 1.00 |
| `/home/petros/dev/tools/scc /home/petros/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 667.6 ± 31.2 | 610.5 | 717.1 | 2.66 ± 0.16 |
| `/home/petros/dev/tools/tokei /home/petros/dev/bench/linux -t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"` | 523.0 ± 30.6 | 507.1 | 629.6 | 2.08 ± 0.15 |
