| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `/home/petros/Documents/dev/tools/cs /home/petros/Documents/dev/bench/jdk --ext java,cpp,cc,hpp,hh,c,h,sh --no-gitignore --hidden --include-generated --no-gitattributes --no-config` | 45.2 ± 0.3 | 45.0 | 45.8 | 1.06 ± 0.02 |
| `/home/petros/Documents/dev/tools/mezura /home/petros/Documents/dev/bench/jdk --languages java,cpp,cc,hpp,hh,c,h,sh --no-gitignore --no-ignore-files --search-in-dotted --no-local --no-default-config --hide keywords,tests --count-minified --count-generated --count-not-code --counting region --no-shebang` | 42.7 ± 1.0 | 41.4 | 44.8 | 1.00 |
| `/home/petros/Documents/dev/tools/scc /home/petros/Documents/dev/bench/jdk -i java,cpp,cc,hpp,hh,c,h,sh --no-gitignore --no-ignore --no-scc-ignore -c --no-cocomo --no-config` | 111.4 ± 0.6 | 110.5 | 113.0 | 2.61 ± 0.06 |
| `/home/petros/Documents/dev/tools/tokei /home/petros/Documents/dev/bench/jdk -t "Java,C++,C++,C++ Header,C++ Header,C,C Header,Shell" --no-ignore --hidden` | 301.4 ± 4.2 | 294.7 | 309.7 | 7.05 ± 0.19 |
