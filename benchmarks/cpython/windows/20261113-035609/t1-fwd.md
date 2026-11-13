| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/linebench-counters/cs.exe D:/dev/bench-corpora/cpython --ext py,c,h,html,js,bat,sh,m --no-gitignore --hidden --include-generated --no-gitattributes --no-config` | 22.0 ± 3.7 | 19.8 | 34.6 | 1.02 ± 0.17 |
| `D:/dev/tools/linebench-counters/mezura.exe D:/dev/bench-corpora/cpython --languages py,c,h,html,js,bat,sh,m --no-gitignore --no-ignore-files --search-in-dotted --no-local --no-default-config --hide keywords,tests --count-minified --count-generated --count-not-code --counting region --no-shebang` | 21.5 ± 0.3 | 21.1 | 22.4 | 1.00 |
| `D:/dev/tools/linebench-counters/scc.exe D:/dev/bench-corpora/cpython -i py,c,h,html,js,bat,sh,m --no-gitignore --no-ignore --no-scc-ignore -c --no-cocomo --no-config` | 40.5 ± 0.9 | 39.3 | 42.0 | 1.88 ± 0.05 |
| `D:/dev/tools/linebench-counters/tokei.exe D:/dev/bench-corpora/cpython -t "Python,C,C Header,HTML,JavaScript,Batch,Shell,Objective-C" --no-ignore --hidden` | 66.1 ± 1.2 | 63.7 | 67.6 | 3.08 ± 0.07 |
