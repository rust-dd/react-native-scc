import { getDefaultKV } from '../kv';
/** Synchronous zustand persist storage, so hydration has no async gap: `createJSONStorage(() => sccStateStorage())`. */
export function sccStateStorage(kv) {
    const store = () => kv ?? getDefaultKV();
    return {
        getItem: (name) => store().getString(name) ?? null,
        setItem: (name, value) => {
            store().set(name, value);
        },
        removeItem: (name) => {
            store().delete(name);
        },
    };
}
