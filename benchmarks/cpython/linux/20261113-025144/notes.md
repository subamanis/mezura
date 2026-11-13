# Benchmark session notes 20261113-025144

corpus:       cpython @ 34439b8a2, pinned and verified
              /home/petros/Documents/dev/bench/cpython
              ext4 /dev/nvme1n1p3, Lexar SSD NQ790 2TB, 16.0 GT/s PCIe x4
machine:      cpu governor on 16 cpus: powersave -> performance
              control drift start to end: 1.0176
              background before the run: 0% busy
MS Defender:  realtime not applicable, process exclusions not applicable
codestats:    codestats 1.1.0 fetched, cs-x86_64-unknown-linux-gnu
mezura:       v3.5.0 (2026-11-12) fetched, mezura-v3.5.0-linux-x86_64.tar.gz
scc:          scc version 4.1.0 fetched, scc_Linux_x86_64.tar.gz
tokei:        tokei 15.0.0 compiled with serialization support: json built with rustc 1.97.1 (8bab26f4f 2026-07-14)
parity:       within 1.0% of the corpus

- [ ] machine quiet during the run

observations:
-

since: no earlier run on this machine over this corpus shares an instance with this one
