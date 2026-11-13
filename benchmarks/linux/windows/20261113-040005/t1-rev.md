| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/linebench-counters/tokei.exe D:/dev/bench-corpora/linux -t "C,C Header,GNU Style Assembly,Assembly,Python,Perl,Perl,Rust,Shell" --no-ignore --hidden` | 617.0 ± 21.2 | 593.1 | 649.2 | 3.51 ± 0.38 |
| `D:/dev/tools/linebench-counters/scc.exe D:/dev/bench-corpora/linux -i c,h,s,asm,py,pl,pm,rs,sh --no-gitignore --no-ignore --no-scc-ignore -c --no-cocomo --no-config` | 509.0 ± 37.4 | 475.2 | 589.0 | 2.90 ± 0.36 |
| `D:/dev/tools/linebench-counters/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,asm,py,pl,pm,rs,sh --no-gitignore --no-ignore-files --search-in-dotted --no-local --no-default-config --hide keywords,tests --count-minified --count-generated --count-not-code --counting region --no-shebang` | 175.6 ± 17.8 | 162.2 | 218.5 | 1.00 |
| `D:/dev/tools/linebench-counters/cs.exe D:/dev/bench-corpora/linux --ext c,h,s,asm,py,pl,pm,rs,sh --no-gitignore --hidden --include-generated --no-gitattributes --no-config` | 214.6 ± 17.5 | 199.5 | 256.3 | 1.22 ± 0.16 |
