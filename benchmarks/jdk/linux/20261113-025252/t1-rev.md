| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/jdk -t "Java,C++,C++,C++ Header,C++ Header,C,C Header,Shell" --no-ignore --hidden` | 299.8 ± 3.7 | 295.1 | 307.6 | 7.01 ± 0.17 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/jdk -i java,cpp,cc,hpp,hh,c,h,sh --no-gitignore --no-ignore --no-scc-ignore -c --no-cocomo --no-config` | 112.8 ± 6.1 | 110.5 | 134.8 | 2.64 ± 0.15 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/jdk --languages java,cpp,cc,hpp,hh,c,h,sh --no-gitignore --no-ignore-files --search-in-dotted --no-local --no-default-config --hide keywords,tests --count-minified --count-generated --count-not-code --counting region --no-shebang` | 42.8 ± 0.9 | 41.4 | 44.9 | 1.00 |
| `/home/petros/Documents/dev/tools/cs /home/petros/Documents/dev/bench/jdk --ext java,cpp,cc,hpp,hh,c,h,sh --no-gitignore --hidden --include-generated --no-gitattributes --no-config` | 45.8 ± 0.7 | 45.0 | 47.2 | 1.07 ± 0.03 |
