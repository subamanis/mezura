| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/linebench-counters/tokei.exe D:/dev/bench-corpora/jdk -t "Java,C++,C++,C++ Header,C++ Header,C,C Header,Shell" --no-ignore --hidden` | 447.3 ± 19.8 | 428.8 | 480.8 | 3.33 ± 0.33 |
| `D:/dev/tools/linebench-counters/scc.exe D:/dev/bench-corpora/jdk -i java,cpp,cc,hpp,hh,c,h,sh --no-gitignore --no-ignore --no-scc-ignore -c --no-cocomo --no-config` | 280.8 ± 17.7 | 269.3 | 318.5 | 2.09 ± 0.23 |
| `D:/dev/tools/linebench-counters/mezura.exe D:/dev/bench-corpora/jdk --languages java,cpp,cc,hpp,hh,c,h,sh --no-gitignore --no-ignore-files --search-in-dotted --no-local --no-default-config --hide keywords,tests --count-minified --count-generated --count-not-code --counting region --no-shebang` | 134.3 ± 12.1 | 127.9 | 168.9 | 1.00 |
| `D:/dev/tools/linebench-counters/cs.exe D:/dev/bench-corpora/jdk --ext java,cpp,cc,hpp,hh,c,h,sh --no-gitignore --hidden --include-generated --no-gitattributes --no-config` | 150.3 ± 12.3 | 144.2 | 184.3 | 1.12 ± 0.14 |
