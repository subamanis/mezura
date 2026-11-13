| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/cpython -t "Python,C,C Header,HTML,JavaScript,Batch,Shell,Objective-C"` | 47.0 ± 0.7 | 46.0 | 48.1 | 5.87 ± 0.13 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/cpython -i py,c,h,html,js,bat,sh,m` | 26.2 ± 0.3 | 25.7 | 26.6 | 3.27 ± 0.06 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/cpython --languages py,c,h,html,js,bat,sh,m` | 9.6 ± 0.2 | 9.3 | 10.1 | 1.20 ± 0.03 |
| `/home/petros/Documents/dev/tools/cs /home/petros/Documents/dev/bench/cpython --ext py,c,h,html,js,bat,sh,m` | 8.0 ± 0.1 | 7.8 | 8.2 | 1.00 |
