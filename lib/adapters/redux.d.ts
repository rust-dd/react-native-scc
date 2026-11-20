import { type KV } from '../kv';
/** redux-persist storage: `persistReducer({ key: 'root', storage: createSccStorage() }, reducer)`. */
export declare function createSccStorage(kv?: KV): {
    getItem: (key: string) => Promise<string | null>;
    setItem: (key: string, value: string) => Promise<void>;
    removeItem: (key: string) => Promise<void>;
};
