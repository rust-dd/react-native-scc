export type KVValue = string | number | boolean | ArrayBuffer;
export interface KVOptions {
    /** Instance name; storage files derive from it. Default: 'default'. */
    id?: string;
    /** Storage directory. Default: platform app-data dir + '/react-native-scc'. */
    path?: string;
    /** 'wal' (durable, default) or 'none' (pure in-memory). */
    persistence?: 'wal' | 'none';
    /** 'relaxed' (default, ~1s fsync) or 'strict' (fsync per commit). */
    durability?: 'relaxed' | 'strict';
    /** Wipe existing files on open. Default false. */
    recreate?: boolean;
    /** Enables encryption at rest (ChaCha20-Poly1305; the cipher key is derived from this passphrase). */
    encryptionKey?: string;
    /** Maximum live entries before the background sweeper evicts keys. */
    maxEntries?: number;
    /** Background TTL/eviction sweep interval in milliseconds. */
    ttlSweepIntervalMs?: number;
}
export interface SetOptions {
    /** The key expires this many milliseconds from now. */
    ttlMs?: number;
}
