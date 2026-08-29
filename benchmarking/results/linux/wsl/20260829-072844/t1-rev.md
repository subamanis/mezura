| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/dev/tools/tokei /home/petros/dev/bench/linux -t "C,C Header,GNU Style Assembly,Python,Perl,Rust,Shell"` | 539.5 ± 45.0 | 502.2 | 673.1 | 2.05 ± 0.19 |
| `/home/petros/dev/tools/scc /home/petros/dev/bench/linux -i c,h,s,py,pl,rs,sh -c --no-cocomo --no-config` | 646.2 ± 12.6 | 629.2 | 673.8 | 2.46 ± 0.11 |
| `/home/petros/dev/tools/mezura /home/petros/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region` | 262.6 ± 10.1 | 246.2 | 282.2 | 1.00 |
