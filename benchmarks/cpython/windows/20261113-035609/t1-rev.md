| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/linebench-counters/tokei.exe D:/dev/bench-corpora/cpython -t "Python,C,C Header,HTML,JavaScript,Batch,Shell,Objective-C" --no-ignore --hidden` | 65.4 ± 1.7 | 62.7 | 69.5 | 3.03 ± 0.08 |
| `D:/dev/tools/linebench-counters/scc.exe D:/dev/bench-corpora/cpython -i py,c,h,html,js,bat,sh,m --no-gitignore --no-ignore --no-scc-ignore -c --no-cocomo --no-config` | 40.4 ± 1.1 | 39.2 | 43.4 | 1.87 ± 0.06 |
| `D:/dev/tools/linebench-counters/mezura.exe D:/dev/bench-corpora/cpython --languages py,c,h,html,js,bat,sh,m --no-gitignore --no-ignore-files --search-in-dotted --no-local --no-default-config --hide keywords,tests --count-minified --count-generated --count-not-code --counting region --no-shebang` | 21.6 ± 0.2 | 21.3 | 21.8 | 1.00 |
| `D:/dev/tools/linebench-counters/cs.exe D:/dev/bench-corpora/cpython --ext py,c,h,html,js,bat,sh,m --no-gitignore --hidden --include-generated --no-gitattributes --no-config` | 21.9 ± 3.5 | 19.2 | 33.3 | 1.01 ± 0.16 |
