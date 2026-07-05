import { getDefaultKV } from '../kv';
/**
 * redux-persist storage engine backed by react-native-scc:
 *
 * persistReducer({ key: 'root', storage: createSccStorage() }, rootReducer)
 */
export function createSccStorage(kv) {
    const store = kv ?? getDefaultKV();
    return {
        getItem: (key) => Promise.resolve(store.getString(key) ?? null),
        setItem: (key, value) => {
            store.set(key, value);
            return Promise.resolve();
        },
        removeItem: (key) => {
            store.delete(key);
            return Promise.resolve();
        },
    };
}
