# Benchmark session notes 20260828-230405
- [ ] corpus and tools on a local disk, not a network or /mnt mount
      corpus: /home/petros/Documents/dev/bench/linux
      tools:  /home/petros/Documents/dev/tools
- [ ] same corpus as the session this is compared against (linux @ 0ff41df1c)
- [ ] machine quiet during the run
- [ ] mezura built from the working tree being measured

observations:
-
- corpus_device was added by hand after the run. The field did not exist when this session was
  measured; the value is what device_of() reports for the same corpus path on the same machine,
  which has not changed. Every other field is as the run wrote it.
