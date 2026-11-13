| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/cpython -t "Python,C,C Header,HTML,JavaScript,Batch,Shell,Objective-C" --no-ignore --hidden` | 45.2 ± 0.6 | 44.3 | 46.1 | 6.19 ± 0.22 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/cpython -i py,c,h,html,js,bat,sh,m --no-gitignore --no-ignore --no-scc-ignore -c --no-cocomo --no-config` | 19.9 ± 0.3 | 19.5 | 20.7 | 2.72 ± 0.10 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/cpython --languages py,c,h,html,js,bat,sh,m --no-gitignore --no-ignore-files --search-in-dotted --no-local --no-default-config --hide keywords,tests --count-minified --count-generated --count-not-code --counting region --no-shebang` | 7.6 ± 0.6 | 7.1 | 9.3 | 1.05 ± 0.09 |
| `/home/petros/Documents/dev/tools/cs /home/petros/Documents/dev/bench/cpython --ext py,c,h,html,js,bat,sh,m --no-gitignore --hidden --include-generated --no-gitattributes --no-config` | 7.3 ± 0.2 | 7.1 | 8.0 | 1.00 |
