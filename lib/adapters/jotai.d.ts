import { type KV } from '../kv';
/**
 * Atom persisted in a KV instance. Reads synchronously on init and
 * reacts to writes made outside jotai (other KV handles included).
 */
export declare function atomWithKV<T>(key: string, initialValue: T, kv?: KV): import("jotai/vanilla").WritableAtom<T, [T | typeof import("jotai/utils").RESET | ((prev: T) => T | typeof import("jotai/utils").RESET)], void>;
