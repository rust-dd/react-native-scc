import { type KV } from '../kv';
/** Atom persisted in a KV store: reads synchronously on init, follows writes from any handle, and `undefined` deletes. */
export declare function atomWithKV<T>(key: string, initialValue: T, kv?: KV): import("jotai/vanilla").WritableAtom<T, [T | typeof import("jotai/utils").RESET | ((prev: T) => T | typeof import("jotai/utils").RESET)], void>;
