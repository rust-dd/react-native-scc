# Benchmark methodology (v6)

The example app compares `react-native-scc-storage` with `react-native-mmkv` through each
library's public JS API, inside a Release build. Every claim in a results table should be
traceable to the raw samples saved with the report.

## What is measured

| Section | Question it answers | Unit |
| --- | --- | --- |
| Throughput | What does one synchronous call cost when calls run back to back? | ns per operation |
| Latency after idle | What does a single call cost in a real UI handler, after the event loop sat idle? | ns, p50 and p99 |
| SCC-only diagnostics | Where does SCC time go (JS wrapper vs Nitro), how long do reopen and fsync take? | ns/op or ms |

Throughput runs on two stores: a small one (482 keys over strings of 16 B / 256 B / 4 KiB,
numbers, booleans, ~1 KiB JSON documents) and a 20k-key one (10k strings + 10k numbers).

## Controls

| Threat to validity | Control |
| --- | --- |
| Simulator or Debug timings | `buildMode`, platform and versions are recorded; the UI warns in Debug. Publish only Release numbers from physical devices. |
| One hot key for every call | Keys come from a precomputed random sequence over the whole family, so hashing and caches see a realistic spread. |
| Rewriting an identical value | Writes rotate through 8 payload variants that differ from the stored value at every byte. |
| The two libraries doing different work | Both run the same loop body; read checksums must match exactly, and after every write case the two stores are compared key by key. |
| Background work leaking into the other library's sample | SCC's WAL writer is drained with two untimed `flush()` calls after each SCC sample, so a compaction never overlaps the next sample. |
| Order effects and thermal drift | Per trial, case order is shuffled and each case flips a coin for which library goes first. The seed is stored in the report. |
| Tight loops hiding per-call fixed costs | The latency section runs one call per JS task after an idle event loop, alternating libraries call by call. |
| Means hiding stalls | Throughput keeps every 12-chunk timing (p95 chunk shown); latency reports p99. |
| No uncertainty estimate | 10 throughput trials and 4 latency trials per launch; speedups carry a 95% bootstrap CI and are only called when the CI excludes 1×. |
| Launch-to-launch variance | `scripts/bench-ios.mjs` runs several fresh launches and reports the min–max of the per-launch medians. |
| Asymmetric listener semantics | Both libraries deliver change events asynchronously through Nitro; each sample waits until its own deliveries arrive. |

## Known asymmetries and gaps

- **Durability differs.** MMKV writes into an mmap, so a value survives a process kill once
  `set` returns. SCC's relaxed WAL appends on a background thread within ~8 ms; `flush()` is the
  barrier. Write rows compare API return latency, not the moment data is safe.
- **Batch rows** compare one SCC `getMany`/`setMany` call with 100 MMKV scalar calls, because MMKV
  has no batch API. They measure throughput, not atomicity.
- **Cold start is not compared.** MMKV caches instances natively and cannot be reopened in
  process; SCC's reopen time is reported on its own.
- **Not covered yet:** memory footprint, Android automation (the in-app benchmark itself runs on
  Android), encrypted stores.

## Running

```sh
# in-app: Release build, then tap "Run benchmark" with the device idle and cool
npx expo run:ios --configuration Release

# automated: builds with autorun, 3 fresh launches, summary + raw JSON in bench-results/
npm run bench:ios -- --device "iPhone 17 Pro" --launches 3
npm run bench:ios -- --device <physical-device-udid> --cooldown 120
npm run bench:ios -- --profile quick --launches 1   # pipeline check, numbers not meaningful

# native core only, no bridge
cargo bench -p kv-core --bench scenario_bench
```

The runner starts the app with `-sccBenchAutorun 1 -sccBenchProfile <profile>` (iOS launch
arguments, read through NSUserDefaults), so one Release build serves every profile. It reads each
report from the app's own SCC store files (`last_bench` in the `example` store), because Release
builds drop `console.log` output.

To see where a call spends its time, launch one case in a loop and sample the JS thread
(simulator apps are host processes; `xctrace` attach hung on Xcode 26.6, `/usr/bin/sample` works):

```sh
xcrun simctl launch booted com.rustdd.scckv.example \
  -sccProfileCase get_str16 -sccProfileLib scc -sccProfileSeconds 25
/usr/bin/sample <pid> 5 -mayDie -file get_str16.txt
```

Android has no launch-argument path yet: build with `EXPO_PUBLIC_SCC_AUTORUN_BENCHMARK=1` (and
optionally `EXPO_PUBLIC_SCC_BENCH_PROFILE`). These values are inlined and cached by Metro, so
clear the cache when changing them.
