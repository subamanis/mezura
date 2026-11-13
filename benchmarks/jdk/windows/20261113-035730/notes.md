# Benchmark session notes 20261113-035730

corpus:       jdk @ b96680ca9, pinned and verified
              D:/dev/bench-corpora/jdk
              NTFS, Lexar SSD NQ790 2TB, SSD, NVMe
machine:      power scheme: Balanced -> high performance
              control drift start to end: 1.0139
              background before the run: 0.2% busy
MS Defender:  realtime True, every process excluded
codestats:    codestats 1.1.0 fetched, cs-x86_64-pc-windows-gnu.exe
mezura:       v3.5.0 (2026-11-12) fetched, mezura-v3.5.0-windows-x86_64.zip
scc:          scc version 4.1.0 fetched, scc_Windows_x86_64.zip
tokei:        tokei 15.0.0 compiled with serialization support: json built with rustc 1.97.1 (8bab26f4f 2026-07-14)
parity:       within 1.0% of the corpus

- [ ] machine quiet during the run

observations:
-

since 20260919-200059 (same machine, same corpus commit)
  mezura    t1   the languages, the same-work flags or the instance's own arguments or environment changed, so the times do not compare   version v3.2.0 (2026-09-19) -> v3.5.0 (2026-11-12)
  codestats t1   149 ms   never measured before on this machine
  scc       t1   280 ms -> 280 ms   +0.2% ± 1.5%   within the noise
  tokei     t1   448 ms -> 445 ms   -0.6% ± 1.8%   within the noise
  control   mezura   timed under another build in 20260919-200059, so the machine's own shift is not known
  differs   background 0.4% busy -> 0.2% busy
            linebench  0.2.0 -> 0.4.1
            hyperfine  hyperfine 1.20.0 -> hyperfine 2.0.0
            drift      0.6% -> 1.4%
