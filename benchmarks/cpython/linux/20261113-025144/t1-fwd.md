| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/cs /home/petros/Documents/dev/bench/cpython --ext py,c,h,html,js,bat,sh,m --no-gitignore --hidden --include-generated --no-gitattributes --no-config` | 7.2 ± 0.3 | 7.1 | 8.1 | 1.00 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/cpython --languages py,c,h,html,js,bat,sh,m --no-gitignore --no-ignore-files --search-in-dotted --no-local --no-default-config --hide keywords,tests --count-minified --count-generated --count-not-code --counting region --no-shebang` | 7.7 ± 0.4 | 7.1 | 8.5 | 1.06 ± 0.07 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/cpython -i py,c,h,html,js,bat,sh,m --no-gitignore --no-ignore --no-scc-ignore -c --no-cocomo --no-config` | 20.0 ± 0.3 | 19.7 | 20.8 | 2.76 ± 0.11 |
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/cpython -t "Python,C,C Header,HTML,JavaScript,Batch,Shell,Objective-C" --no-ignore --hidden` | 45.2 ± 0.7 | 44.7 | 47.7 | 6.25 ± 0.24 |
