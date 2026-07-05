import { type KV } from '../kv';
/**
 * redux-persist storage engine backed by react-native-scc:
 *
 * persistReducer({ key: 'root', storage: createSccStorage() }, rootReducer)
 */
export declare function createSccStorage(kv?: KV): {
    getItem: (key: string) => Promise<string | null>;
    setItem: (key: string, value: string) => Promise<void>;
    removeItem: (key: string) => Promise<void>;
};
