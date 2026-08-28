# Why the same corpus counts at different speeds

An open question carried over to the Windows side. Nothing here is settled; the Linux half is
measured, the Windows half is observed and still needs confirming.

## What it looked like at first

Reading mezura's own printed exec time, the Linux kernel tree counted in roughly:

```
Linux,   corpus on the Lexar (nvme1n1)      0.25 s
Windows, corpus on D: (the same Lexar)      0.55 s
Windows, corpus on C: (Samsung 9100 PRO)    0.38 s
```

The first reading of that was that Windows has a slow filesystem, and then, once the corpus was
cloned to C: as well, that the two drives explain it: the Samsung is the faster device, so the
faster number came from the faster disk.

Two things sit against that reading.

The Linux and the Windows-on-D figures are **the same physical drive**, so 0.25 against 0.55 is
already a clean comparison with storage held constant. That gap is 2.2x and owes nothing to
hardware.

And the profiling says the cost is not where a slow disk would put it. Opening files is **23% of
program time on Windows on D, against 3% on Linux**. Opening a file reads no data; it resolves a
path, checks access and hands back a handle. Slow storage shows up in reads, not in opens. A
filter driver sitting above the volume — an anti-virus scanning on `CreateFile`, an indexer —
shows up exactly there.

Arithmetic on how much that explains:

```
Windows 0.55 x 23% = 0.126 s in open
Linux   0.25 x  3% = 0.008 s in open
                     -------
difference in open alone            0.118 s
total gap                           0.300 s
```

So opens account for around 40% of it. The rest is somewhere else: directory enumeration, reads,
general syscall cost.

## What was tested on Linux

Whether the disk matters at all here, by emptying the page cache and counting again immediately:

```
sudo sh -c 'sync; echo 3 > /proc/sys/vm/drop_caches'
~/Documents/dev/tools/mezura ~/Documents/dev/bench/linux     ->  0.92 s
~/Documents/dev/tools/mezura ~/Documents/dev/bench/linux     ->  0.26 s
```

Cold is **3.5x slower than warm**. So storage does matter on Linux, very much, when nothing is
cached. It stops mattering once the tree is resident: the corpus is 1.37 GB against 60 GB of
RAM, Linux keeps it, and every run after the first reads from memory. At 222 ms for 63,864 files
and 1.37 GB the implied rate is 6.15 GB/s across small files, which no drive delivers.

This is worth stating precisely, because an earlier guess here was wrong in both directions at
once: it is not true that the disk is irrelevant on Linux, and it is not true that a faster
drive would improve the Linux figure. Cold, the drive is most of the time. Warm, it is none of
it. Repeated runs are warm.

Note also that `benchmark.py` runs three warmup iterations before it measures, so every number
it reports is the warm case by construction.

## The behaviour to confirm on Windows

If Windows does not hold the tree resident the way Linux does — whether because its cache evicts
sooner, or because a filter driver re-reads on every open regardless of cache — then every run
is partly cold, and once you are partly cold the drive is back in the measurement. That would
explain the C:/D: difference without the drives differing in speed at all.

Reported from the Windows side so far, and to be confirmed with the commands below: on C: the
share of time spent in open falls, and the run is barely IO bound. That points at the filter
stack rather than at the hardware, since a driver or device difference would move reads, not
opens.

## Commands to run on Windows

**Is the tree held warm?** Run the same count twice in a row on the same volume. A second run
much faster than the first means the cache is doing its job. Two equally slow runs mean it never
warms, and the drive is in every measurement:

```
mezura.exe D:\dev\bench\linux
mezura.exe D:\dev\bench\linux
```

Then the same on C:.

**Is a filter driver charging per open?** The share of time in open, on each volume. If it falls
on C: while reads stay where they are, it is the filter stack and not the disk:

```
mezura.exe D:\dev\bench\linux
mezura.exe C:\...\linux
```

**What is actually configured.** This is the reading that would settle it, since the two volumes
are on one machine with one Defender:

```powershell
Get-MpComputerStatus | Select-Object RealTimeProtectionEnabled
Get-MpPreference | Select-Object -ExpandProperty ExclusionPath
Get-MpPreference | Select-Object -ExpandProperty ExclusionProcess
```

If an exclusion covers C: and not D:, the whole difference is accounted for and no hardware
explanation is needed.

**Whether the volumes differ in any other way** — indexing, compression, cluster size — since
any of those would also charge per file rather than per byte:

```powershell
Get-Volume -DriveLetter C, D | Select DriveLetter, FileSystem, AllocationUnitSize
Get-WmiObject Win32_Volume -Filter "DriveLetter='D:'" | Select IndexingEnabled, Compressed
Get-WmiObject Win32_Volume -Filter "DriveLetter='C:'" | Select IndexingEnabled, Compressed
Get-PhysicalDisk | Select FriendlyName, MediaType, BusType
```

**A cold measurement, for the equivalent of the Linux test.** Windows has no `drop_caches`; the
usual stand-in is a tool that empties the standby list, such as RAMMap's *Empty Standby List*, or
simply a reboot before the first run. Without one of those, a Windows first-run figure is not
comparable to the Linux cold figure.

## What would settle it

Three outcomes, and each points somewhere different:

- **Open share falls on C:, reads unchanged** — a filter driver charging per open, scoped
  differently on the two volumes. Fixed with an exclusion, not with hardware.
- **Everything falls proportionally on C:** — the drives really do differ under this load, and
  the first reading was right.
- **Second run no faster than the first, on either volume** — Windows is not holding the tree
  resident, every run pays storage, and the Linux comparison is partly a comparison of cache
  behaviour rather than of counting code.

The third would be the most interesting for the benchmark itself, because it would mean the
Linux and Windows figures are not measuring the same thing, and the record would need to say so.

## What this means for the benchmark record

Two runs on one machine, same program, same data, 45% apart, and `run.json` holds nothing that
explains it. Knowing only that protection is enabled is not enough, since that is true on both
volumes and the volumes behave differently.

## To be implemented from the Windows side

Decided but not written, because none of it can be tested from Linux.

### The fields

```
defender_realtime:        True | False | not applicable
defender_corpus_excluded: True | False | unknown (needs admin) | not applicable
defender_per_tool:
  mezura:  process_excluded  binary_excluded
  scc:     process_excluded  binary_excluded
  tokei:   process_excluded  binary_excluded
```

Per tool rather than one flag for the tools directory, because `ExclusionProcess` is per
executable and Microsoft's own wording is that it "excludes files **opened by** specified
processes". A tool listed there never has any of the 88,764 files it opens scanned in real time,
which is exactly the cost measured at 23% of run time. One tool excluded and another not, and t1
stops measuring which counts faster and starts measuring which escaped the scanner.

`binary_excluded` is whether the executable's own path sits under an `ExclusionPath`. It is close
to irrelevant, since the binary is opened once, and is recorded only for completeness.
`defender_corpus_excluded` is shared by all three tools, so it moves the absolute numbers without
skewing the comparison between them.

On Linux and macOS every one of these is written as `not applicable`, spelled out rather than
left absent. An absent key cannot be told apart from one an older version of the script never
recorded, and these records are going to be compared across both platforms and time.

### What needs elevation, and what must not be guessed

`Get-MpComputerStatus` needs no elevation; Microsoft's reference lists no permissions at all, and
`RealTimeProtectionEnabled` is among the properties it returns.

`Get-MpPreference` does need it, and this is the trap: run without elevation, `ExclusionPath`
comes back as the literal string `N/A: Must be admin to view exclusions`. Not an error, not an
empty list — a placeholder shaped like data. Asking "is the corpus in this list" of that answer
returns no, and the record would then claim `False` when the truth is that nothing was known. The
check has to test for that string, and write `unknown (needs admin)`.

The measuring run is elevated anyway, for the power scheme, so it gets the real answer. `--check`
is not, and there is nothing stopping it being run elevated on Windows: unlike `--setup` it
builds nothing and leaves nothing behind, writing only into a temporary directory it deletes.

### The gate

If the three `process_excluded` values are not all alike, refuse. Not a y/N.

The governor is a y/N because a run on an unprepared machine still produces real numbers, only
noisier. An asymmetric process exclusion is different in kind: there is no useful run to be had,
because the comparison has stopped being about counting speed. Same shape as the corpus commit
gate — say what is wrong, give the command that fixes it, and if it is ever overridden, mark the
record so such a run is never quietly compared against a clean one.

```
scc.exe is excluded from Defender scanning and mezura.exe is not.

Files opened by an excluded process are never scanned in real time, so this
run would measure which tool escaped the antivirus, not which counts faster.

  Get-MpPreference | Select -ExpandProperty ExclusionProcess
  Remove-MpPreference -ExclusionProcess scc.exe

Either exclude all three or none.
```
