import { atomWithStorage } from 'jotai/utils';
import { getDefaultKV } from '../kv';
/**
 * Atom persisted in a KV instance. Reads synchronously on init and
 * reacts to writes made outside jotai (other KV handles included).
 */
export function atomWithKV(key, initialValue, kv) {
    const store = kv ?? getDefaultKV();
    return atomWithStorage(key, initialValue, {
        getItem: (k, fallback) => {
            const value = store.getJSON(k);
            return value === undefined ? fallback : value;
        },
        setItem: (k, value) => {
            store.setJSON(k, value);
        },
        removeItem: (k) => {
            store.delete(k);
        },
        subscribe: (k, callback, fallback) => {
            const subscription = store.addOnValueChangedListener((changed) => {
                if (changed === null || changed === k) {
                    const value = store.getJSON(k);
                    callback(value === undefined ? fallback : value);
                }
            });
            return () => subscription.remove();
        },
    }, { getOnInit: true });
}
