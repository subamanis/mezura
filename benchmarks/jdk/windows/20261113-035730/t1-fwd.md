| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/linebench-counters/cs.exe D:/dev/bench-corpora/jdk --ext java,cpp,cc,hpp,hh,c,h,sh --no-gitignore --hidden --include-generated --no-gitattributes --no-config` | 147.2 ± 11.4 | 142.4 | 188.1 | 1.12 ± 0.12 |
| `D:/dev/tools/linebench-counters/mezura.exe D:/dev/bench-corpora/jdk --languages java,cpp,cc,hpp,hh,c,h,sh --no-gitignore --no-ignore-files --search-in-dotted --no-local --no-default-config --hide keywords,tests --count-minified --count-generated --count-not-code --counting region --no-shebang` | 131.1 ± 10.1 | 126.8 | 167.5 | 1.00 |
| `D:/dev/tools/linebench-counters/scc.exe D:/dev/bench-corpora/jdk -i java,cpp,cc,hpp,hh,c,h,sh --no-gitignore --no-ignore --no-scc-ignore -c --no-cocomo --no-config` | 279.7 ± 15.0 | 270.9 | 316.7 | 2.13 ± 0.20 |
| `D:/dev/tools/linebench-counters/tokei.exe D:/dev/bench-corpora/jdk -t "Java,C++,C++,C++ Header,C++ Header,C,C Header,Shell" --no-ignore --hidden` | 443.5 ± 19.2 | 426.4 | 474.8 | 3.38 ± 0.30 |
