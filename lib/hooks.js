import { useEffect, useState } from 'react';
import { getDefaultKV, KV } from './kv';
function useKVValue(key, kv, read, write) {
    const store = kv ?? getDefaultKV();
    const [value, setValue] = useState(() => read(store, key));
    useEffect(() => {
        setValue(read(store, key));
        const subscription = store.addOnValueChangedListener((changedKey) => {
            if (changedKey === null || changedKey === key) {
                setValue(read(store, key));
            }
        });
        return () => subscription.remove();
        // `read`/`write` are module-level constants at every call site.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [store, key]);
    const set = (next) => {
        if (next === undefined)
            store.delete(key);
        else
            write(store, key, next);
    };
    return [value, set];
}
const readString = (kv, key) => kv.getString(key);
const readNumber = (kv, key) => kv.getNumber(key);
const readBoolean = (kv, key) => kv.getBoolean(key);
const readBuffer = (kv, key) => kv.getBuffer(key);
export function useKVString(key, kv) {
    return useKVValue(key, kv, readString, (s, k, v) => s.set(k, v));
}
export function useKVNumber(key, kv) {
    return useKVValue(key, kv, readNumber, (s, k, v) => s.set(k, v));
}
export function useKVBoolean(key, kv) {
    return useKVValue(key, kv, readBoolean, (s, k, v) => s.set(k, v));
}
export function useKVBuffer(key, kv) {
    return useKVValue(key, kv, readBuffer, (s, k, v) => s.set(k, v));
}
export function useKVJSON(key, kv) {
    return useKVValue(key, kv, (s, k) => s.getJSON(k), (s, k, v) => s.setJSON(k, v));
}
