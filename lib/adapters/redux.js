import { getDefaultKV } from '../kv';
/**
 * redux-persist storage engine over a KV instance:
 *
 * persistReducer({ key: 'root', storage: createSccStorage() }, rootReducer)
 */
export function createSccStorage(kv) {
    const store = kv ?? getDefaultKV();
    return {
        getItem: async (key) => store.getString(key) ?? null,
        setItem: async (key, value) => {
            store.set(key, value);
        },
        removeItem: async (key) => {
            store.delete(key);
        },
    };
}
