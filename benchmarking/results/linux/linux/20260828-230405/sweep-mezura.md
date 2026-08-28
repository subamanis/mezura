| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region` | 229.5 ± 12.0 | 210.2 | 257.3 | 1.04 ± 0.07 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --threads 4 64` | 228.2 ± 13.8 | 204.5 | 252.7 | 1.04 ± 0.07 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --threads 16 64` | 228.1 ± 7.9 | 212.9 | 244.6 | 1.04 ± 0.05 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --threads 8 32` | 220.1 ± 8.2 | 204.3 | 232.9 | 1.00 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --threads 8 128` | 235.6 ± 9.2 | 213.9 | 252.2 | 1.07 ± 0.06 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/linux --languages c,h,s,py,pl,rs,sh --hide keywords --count-minified --count-generated --counting region --threads 16 128` | 235.5 ± 10.4 | 216.1 | 256.3 | 1.07 ± 0.06 |
