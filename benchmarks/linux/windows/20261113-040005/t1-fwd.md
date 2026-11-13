| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `D:/dev/tools/linebench-counters/cs.exe D:/dev/bench-corpora/linux --ext c,h,s,asm,py,pl,pm,rs,sh --no-gitignore --hidden --include-generated --no-gitattributes --no-config` | 210.0 ± 12.6 | 200.2 | 245.0 | 1.20 ± 0.10 |
| `D:/dev/tools/linebench-counters/mezura.exe D:/dev/bench-corpora/linux --languages c,h,s,asm,py,pl,pm,rs,sh --no-gitignore --no-ignore-files --search-in-dotted --no-local --no-default-config --hide keywords,tests --count-minified --count-generated --count-not-code --counting region --no-shebang` | 174.9 ± 10.5 | 164.3 | 195.3 | 1.00 |
| `D:/dev/tools/linebench-counters/scc.exe D:/dev/bench-corpora/linux -i c,h,s,asm,py,pl,pm,rs,sh --no-gitignore --no-ignore --no-scc-ignore -c --no-cocomo --no-config` | 533.1 ± 30.6 | 483.0 | 587.4 | 3.05 ± 0.25 |
| `D:/dev/tools/linebench-counters/tokei.exe D:/dev/bench-corpora/linux -t "C,C Header,GNU Style Assembly,Assembly,Python,Perl,Perl,Rust,Shell" --no-ignore --hidden` | 617.6 ± 19.3 | 590.3 | 644.5 | 3.53 ± 0.24 |
