import type { StateStorage } from 'zustand/middleware';
import { type KV } from '../kv';
/** Synchronous zustand persist storage, so hydration has no async gap: `createJSONStorage(() => sccStateStorage())`. */
export declare function sccStateStorage(kv?: KV): StateStorage;
