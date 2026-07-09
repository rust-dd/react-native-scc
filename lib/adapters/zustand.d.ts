import type { StateStorage } from 'zustand/middleware';
import { type KV } from '../kv';
/**
 * zustand persist storage over a KV instance. Synchronous, so
 * hydration completes without an async gap:
 *
 * persist(config, {
 *   name: 'my-store',
 *   storage: createJSONStorage(() => sccStateStorage()),
 * })
 */
export declare function sccStateStorage(kv?: KV): StateStorage;
