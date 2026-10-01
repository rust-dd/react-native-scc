# react-native-scc

[![npm](https://img.shields.io/npm/v/react-native-scc-storage)](https://www.npmjs.com/package/react-native-scc-storage)
[![license](https://img.shields.io/npm/l/react-native-scc-storage)](https://github.com/rust-dd/react-native-scc/blob/main/LICENSE)
[![platforms](https://img.shields.io/badge/platforms-iOS%20%7C%20Android-lightgrey)](https://github.com/rust-dd/react-native-scc)

Ultra-low-latency, persistent key-value storage for React Native and Expo. The core is written in Rust on top of [`scc`](https://crates.io/crates/scc) (a scalable concurrent hash map), and it reaches JavaScript through [Nitro Modules](https://nitro.margelo.com), so a call from JS to Rust costs nanoseconds, not microseconds.

This project is also a statement of intent: **bringing Rust closer to React Native.** A Rust core behind Nitro Modules ships as an ordinary npm package — no toolchain for consumers, no compromise on performance — and this library is the proof that the pattern can go head-to-head with established C++ storage libraries.

The design goal is simple: **every read is a RAM lookup, every write is durable, and the disk never sits on your hot path.** Writes update the in-memory map synchronously and stream to a write-ahead log on a background thread; a hard kill can cost you the last few milliseconds of writes, but committed data is never corrupted.

- **All reads from RAM** — the persistent store reads exactly as fast as a pure in-memory one
- **Durable by default** — write-ahead log with group commits, atomic snapshot compaction, CRC-protected recovery
- **Sync and async APIs** — sync for the hot path, `*Async` variants on Nitro's thread pool for anything that must not block the JS thread
- **Batch operations** — `setMany`/`getMany` cross the bridge once for a whole record set
- **Transactions + namespaces** — crash-atomic sync commits, prefix helpers, and scoped KV views
- **Encryption at rest** — opt-in ChaCha20-Poly1305 per instance, snapshot and WAL both encrypted
- **TTL + eviction** — per-key expiry with a background sweeper, optional `maxEntries` cap
- **Native change events** — batched delivery and key-scoped listeners; selectors and hooks react to writes made through any handle of the same store
- **React hooks** — `useKVString`, `useKVNumber`, `useKVBoolean`, `useKVBuffer`, `useKVJSON`
- **State-manager adapters** — zustand persist, jotai `atomWithKV`, redux-persist engine as subpath exports
- **Zero-config persistence** — storage lands in the platform app-data directory (iOS: Application Support, Android: `filesDir`)
- **Multiple independent stores** — each `id` gets its own file pair, WAL thread, and durability settings
- iOS + Android, Expo dev builds + bare React Native

## Benchmarks

JS-visible synchronous API cost vs [react-native-mmkv](https://github.com/mrousavy/react-native-mmkv) 4.3.2 on an **iPhone 15 Pro Max (iOS 27.0.1), Release build**, measured with the example app's [methodology v6](example/BENCHMARK.md) benchmark: random keys over 482- and 20k-key stores, values that differ from the stored one, randomized interleaved trials, results cross-checked between both libraries, and 95% bootstrap confidence intervals. Values are the median of three fresh app launches with ten trials each; every row favors SCC with its interval clear of 1× on all three launches.

| Case (ns per op, lower is better) | SCC | MMKV | SCC faster |
| --- | ---: | ---: | ---: |
| `getString`, 16 B | 155 | 327 | 2.11× |
| `getString`, 256 B | 180 | 380 | 2.14× |
| `getString`, 4 KiB | 442 | 696 | 1.55× |
| `getNumber` | 132 | 296 | 2.23× |
| `getBoolean` | 136 | 289 | 2.13× |
| `getString`, missing key | 127 | 281 | 2.22× |
| `contains` | 123 | 272 | 2.20× |
| `getString`, 16 B, 20k-key store | 157 | 358 | 2.26× |
| `set` string, 16 B | 248 | 427 | 1.76× |
| `set` string, 256 B | 263 | 545 | 2.02× |
| `set` string, 4 KiB | 704 | 3990 | 5.66× |
| `set` number | 198 | 388 | 1.96× |
| `set` boolean | 206 | 365 | 1.77× |
| `setJSON`, ~1 KiB object | 6550 | 7830 | 1.20× |
| `getJSON`, ~1 KiB object | 5260 | 5410 | 1.03× |
| `getMany`, 100 × 16 B, per key ¹ | 150 | 283 | 1.88× |
| `setMany`, 100 × 16 B, per key ¹ | 170 | 354 | 2.08× |
| `set` string, 16 B, one change listener | 269 | 536 | 1.97× |
| `set` string, 16 B, 50 key listeners ² | 282 | 3940 | 13.90× |

A single call after an idle frame (p50): `set` 9.7 µs vs 16.1 µs, `getString` 6.0 µs vs 12.3 µs.

¹ One SCC batch call vs 100 MMKV scalar calls, since MMKV has no batch API; this compares throughput, not atomicity.

² 50 hook-style subscriptions: SCC `addOnKeyChangedListener`, MMKV one filtered `addOnValueChangedListener` per key, the way its hooks subscribe. Listener rows time the write call; delivery to JS follows asynchronously and is not timed.

Write rows measure API return, not durability: MMKV writes into an mmap, so a value survives a process kill once `set` returns, while SCC appends to its WAL on a background thread within ~8 ms; `flush()` is the barrier (1000 × 256 B writes plus `flush()` took 4.0 ms on this device). Reproduce with `npm run bench:ios -- --device <udid> --team <apple-team-id>` from `example/`; every run saves its raw samples.

## Install

```sh
npm install react-native-scc-storage react-native-nitro-modules
```

### Expo

The config plugin declares Expo SDK 57 or newer as an optional peer.

```json
{ "plugins": ["react-native-scc-storage"] }
```

```sh
npx expo prebuild
npx expo run:ios
npx expo run:android
```

Expo Go is not supported (native code) — use a dev build.

### Bare React Native

```sh
cd ios && pod install
```

Prebuilt Rust static libraries ship with the package. If they are missing (e.g. a source checkout), the build scripts compile them automatically — that path requires a Rust toolchain with the iOS/Android targets installed.

## Quick start

```ts
import { createKV } from 'react-native-scc-storage'

const kv = createKV() // persistent, id 'default'

// sync — the hot path
kv.set('user.name', 'Ada')
kv.set('user.score', 42.5)
kv.set('user.premium', true)
kv.setJSON('user.prefs', { theme: 'dark' })

kv.getString('user.name')                    // 'Ada'
kv.getNumber('user.score')                   // 42.5
kv.getJSON<{ theme: string }>('user.prefs')  // { theme: 'dark' }

// async — same store, Nitro thread pool
await kv.setAsync('big.blob', someArrayBuffer)
const blob = await kv.getBufferAsync('big.blob')
await kv.flushAsync() // durability barrier

// batch — one bridge crossing
kv.setMany({ a: '1', b: '2', c: '3' })
kv.getMany(['a', 'b', 'missing']) // ['1', '2', undefined]
```

### Instances

Every `id` is an independent store with its own files (`<id>.snap` + `<id>.wal`), its own background writer, and its own settings. Opening the same `id` twice returns the same underlying store.

```ts
const settings = createKV({ id: 'settings' })
const vault    = createKV({ id: 'vault', durability: 'strict' })   // fsync every commit
const cache    = createKV({ id: 'cache' })                          // relaxed (default): ~1s fsync
const ui       = createKV({ id: 'ui', persistence: 'none' })        // pure in-memory, no files
```

Options: `id`, `path` (override the storage directory), `persistence: 'wal' | 'none'`, `durability: 'relaxed' | 'strict'`, `recreate` (wipe on open), `encryptionKey` (see below), `maxEntries`, and `ttlSweepIntervalMs`.

### Encryption at rest

```ts
const vault = createKV({ id: 'vault', encryptionKey: 'my-secret-passphrase' })
```

Everything the instance writes to disk — snapshot and write-ahead log alike — is encrypted with ChaCha20-Poly1305; the 256-bit cipher key is derived from the passphrase with SHA-256. Opening an encrypted store with a wrong key (or without one) fails without touching the files, and opening a plaintext store with a key fails too, so a configuration mistake can never silently corrupt or rewrite data. Store the passphrase in the platform keystore (Keychain / Android Keystore) — the library deliberately does not manage key storage for you.

### TTL

```ts
kv.set('session.token', token, { ttlMs: 15 * 60 * 1000 })
kv.setJSON('cache.profile', profile, { ttlMs: 60_000 })
```

Expired keys read as missing immediately (`get`, `contains`, `getAllKeys` all agree), and a background sweeper physically reclaims them — also from disk — on the instance's WAL thread (sweep interval: 30 s). TTL persists across restarts: a key set with a 1-hour TTL is still gone after a kill + relaunch past its deadline.

### Eviction

```ts
const cache = createKV({
  id: 'cache',
  maxEntries: 10_000,
  ttlSweepIntervalMs: 5_000,
})
```

When the store outgrows `maxEntries`, the sweeper evicts expired keys first, then arbitrary live keys until it fits. Eviction order is unspecified (not LRU) — use it as a safety cap, not a cache policy.

### Transactions

```ts
const next = kv.transaction((tx) => {
  const current = tx.getNumber('counter') ?? 0
  tx.set('counter', current + 1)
  tx.setJSON('counter.meta', { updatedAt: Date.now() })
  return current + 1
})
```

Transactions are synchronous and crash-atomic for recovery: the callback stages writes in JS and sees its own staged values, then commits them as one WAL record, so replay after a crash applies either all staged writes or none. Concurrent readers do not get multi-key isolation while the in-memory commit is being applied. Async callbacks are rejected so the library never holds transactional state across an `await`.

### Prefixes and namespaces

```ts
const user = kv.namespace('user:123')

user.set('name', 'Ada')              // stores user:123:name
user.setJSON('prefs', { theme: 'dark' })
user.getAllKeys()                    // ['name', 'prefs']
user.clearAll()                      // deletes only user:123:* keys

kv.getKeysByPrefix('user:123:')      // full keys
kv.deleteByPrefix('cache:')
```

Namespaces are lightweight JS views over the same underlying store. They do not create extra files or WAL threads.

### Hooks

```tsx
import { useKVNumber } from 'react-native-scc-storage'

function Counter() {
  const [count, setCount] = useKVNumber('counter')
  return <Button title={`${count ?? 0}`} onPress={() => setCount((count ?? 0) + 1)} />
}
```

Each hook returns `[value, setValue]`; calling `setValue(undefined)` deletes the key. Hooks re-render on any write to the key, including writes made through other KV objects opened with the same id.

```tsx
const theme = useKVSelector<{ theme?: string }, string | undefined>(
  'settings',
  (settings) => settings?.theme
)
```

### Change listener

The listener fires for every mutation of the underlying store, from any handle. `key` is `null` after `clearAll` ("everything changed"). Delivery is asynchronous on the JS thread: changes queue natively and cross into JS as one batch per burst, in write order.

```ts
const sub = kv.addOnValueChangedListener((key) => {
  console.log(key === null ? 'store cleared' : `changed: ${key}`)
})
sub.remove()
```

`addOnKeyChangedListener` fires only for one key (and for `clearAll`). Key listeners are indexed, so a write only reaches the listeners of its own key; hooks, `observeJSON`, and the jotai adapter subscribe this way:

```ts
const sub = kv.addOnKeyChangedListener('settings', () => console.log('settings changed'))
```

Selectors sit on top of a key listener and only fire when the selected value changes:

```ts
const sub = kv.observeJSON(
  'settings',
  (settings: { theme?: string } | undefined) => settings?.theme,
  (theme) => console.log('theme changed', theme)
)
```

## Adapters

One package, three subpath exports. `zustand` and `jotai` are optional peer dependencies — install only what you use.

### zustand

```ts
import { create } from 'zustand'
import { persist, createJSONStorage } from 'zustand/middleware'
import { sccStateStorage } from 'react-native-scc-storage/zustand'

const useStore = create(
  persist((set) => ({ bears: 0 }), {
    name: 'bears',
    storage: createJSONStorage(() => sccStateStorage()),
  })
)
```

The storage is synchronous, so zustand hydrates without an async gap — no loading flicker, no `onRehydrateStorage` dance.

### jotai

```ts
import { atomWithKV } from 'react-native-scc-storage/jotai'

const counterAtom = atomWithKV('counter', 0)
```

Reads synchronously on init (`getOnInit`) and reacts to writes made outside jotai — including other KV handles — via the native change listener.

### redux-persist

```ts
import { createSccStorage } from 'react-native-scc-storage/redux'

const persistedReducer = persistReducer(
  { key: 'root', storage: createSccStorage() },
  rootReducer
)
```

## API

| sync | async | returns |
|---|---|---|
| `set(key, value)` | `setAsync` | `void` — value: `string \| number \| boolean \| ArrayBuffer` |
| `setJSON(key, value)` | `setJSONAsync` | `void` |
| `setMany(entries)` | `setManyAsync` | `void` — `Record<string, string>` |
| `transaction(callback)` | — | callback return value |
| `namespace(prefix)` | — | scoped `KV` view |
| `getKeysByPrefix(prefix)` | — | `string[]` |
| `deleteByPrefix(prefix)` | — | `number` |
| `addOnValueChangedListener(listener)` | — | `KVSubscription` |
| `addOnKeyChangedListener(key, listener)` | — | `KVSubscription` |
| `observeJSON(key, selector, listener)` | — | `KVSubscription` |
| `getString(key)` | `getStringAsync` | `string \| undefined` |
| `getNumber(key)` | `getNumberAsync` | `number \| undefined` |
| `getBoolean(key)` | `getBooleanAsync` | `boolean \| undefined` |
| `getBuffer(key)` | `getBufferAsync` | `ArrayBuffer \| undefined` |
| `getJSON<T>(key)` | `getJSONAsync` | `T \| undefined` |
| `getMany(keys)` | `getManyAsync` | `(string \| undefined)[]` |
| `contains(key)` | `containsAsync` | `boolean` |
| `delete(key)` | `deleteAsync` | `boolean` |
| `getAllKeys()` | `getAllKeysAsync` | `string[]` |
| `clearAll()` | `clearAllAsync` | `number` sync, `void` async |
| `flush()` | `flushAsync` | `void` — blocks until fsynced |
| `size` | — | `number` |
| `close()` | — | `void` |

Reading a key that holds a different type returns `undefined` (matching react-native-mmkv). Numbers are IEEE-754 doubles, i.e. exactly JS number semantics.

## Durability model

Writes update the in-memory map and append their record to an in-process log under a single lock; a dedicated background thread checksums the records and writes them to the write-ahead log in group commits (at most every 8 ms, sooner past 128 KiB). The writer keeps polling for 250 ms after its last input, so writes inside a burst never pay a thread wake-up. With `durability: 'relaxed'` (default) the log is fsynced about once per second; with `'strict'` every group commit is fsynced. `flush()` / `flushAsync()` is the explicit barrier: it returns only after everything written so far is on disk.

On restart the store recovers from snapshot + WAL replay. Every record carries a CRC32; a torn tail from a hard kill is truncated and recovery continues — committed data is never lost or corrupted. When the WAL outgrows `max(4 MiB, 2 × snapshot size)`, the background writer compacts it into an atomically replaced snapshot, keeping recovery files bounded without moving disk I/O onto the JS thread. Transactions wait for a compaction only while the map is captured in memory, never across its fsyncs.

## Architecture

```
TypeScript (KV class, hooks, adapters)
  └─ Nitro Modules (JSI, sync calls, zero-copy where possible)
      └─ C++ HybridObjects (hot get/set/contains/delete: raw JSI host functions)
          └─ C FFI (cbindgen, panic-safe boundary)
              └─ Rust core: scc::HashMap (per-bucket locks) + WAL writer thread
```

The Rust core is an independent crate (`crates/kv-core`) with its own test suite: crash-recovery tests that truncate the WAL at every byte offset, multi-threaded stress tests racing writers against compaction, and criterion benchmarks. The C ABI layer (`crates/kv-ffi`) wraps every entry point in `catch_unwind`, so a Rust panic can never unwind across the language boundary.

## Development

```sh
npm install
npm run specs            # tsc + nitrogen codegen
npm test                 # jest (KV + hooks + adapters against a mock native layer)
cargo test --workspace   # Rust core + FFI suites
cargo bench -p kv-core   # criterion benchmarks
npm run rust:build       # cross-compile static libs for iOS + Android
```

The example app under `example/` runs a full on-device self-test (sync/async round-trips, persistence across launches, cross-handle change events) and the MMKV comparison benchmark with live charts.

## License

MIT
