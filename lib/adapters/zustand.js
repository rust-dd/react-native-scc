import { getDefaultKV } from '../kv';
/**
 * zustand persist storage backed by react-native-scc. Synchronous, so
 * hydration completes without an async gap:
 *
 * persist(config, {
 *   name: 'my-store',
 *   storage: createJSONStorage(() => sccStateStorage()),
 * })
 */
export function sccStateStorage(kv) {
    const store = kv ?? getDefaultKV();
    return {
        getItem: (name) => store.getString(name) ?? null,
        setItem: (name, value) => {
            store.set(name, value);
        },
        removeItem: (name) => {
            store.delete(name);
        },
    };
}
