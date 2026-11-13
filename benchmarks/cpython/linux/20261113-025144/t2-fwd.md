| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/cs /home/petros/Documents/dev/bench/cpython --ext py,c,h,html,js,bat,sh,m` | 8.1 ± 0.3 | 7.9 | 9.0 | 1.00 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/cpython --languages py,c,h,html,js,bat,sh,m` | 9.6 ± 0.2 | 9.1 | 9.9 | 1.18 ± 0.05 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/cpython -i py,c,h,html,js,bat,sh,m` | 26.3 ± 0.4 | 25.6 | 26.8 | 3.25 ± 0.13 |
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/cpython -t "Python,C,C Header,HTML,JavaScript,Batch,Shell,Objective-C"` | 46.6 ± 0.7 | 45.4 | 48.4 | 5.74 ± 0.23 |
