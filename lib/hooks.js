import { useCallback, useEffect, useMemo, useRef, useSyncExternalStore, } from 'react';
import { getDefaultKV, INTERNAL_GET_JSON_TEXT, KV } from './kv';
class KVSnapshotSource {
    store;
    key;
    read;
    equals;
    snapshot;
    listeners = new Set();
    subscription;
    constructor(store, key, read, equals) {
        this.store = store;
        this.key = key;
        this.read = read;
        this.equals = equals;
    }
    getSnapshot = () => {
        const next = this.read(this.store, this.key, this.snapshot);
        if (this.snapshot === undefined || !this.equals(this.snapshot, next)) {
            this.snapshot = next;
        }
        return this.snapshot;
    };
    subscribe = (listener) => {
        const before = this.getSnapshot();
        this.listeners.add(listener);
        try {
            this.subscription ??= this.store.addOnValueChangedListener((changedKey) => {
                if (changedKey === null || changedKey === this.key)
                    this.invalidate();
            });
        }
        catch (error) {
            this.listeners.delete(listener);
            throw error;
        }
        if (this.getSnapshot() !== before)
            listener();
        return () => {
            this.listeners.delete(listener);
            if (this.listeners.size === 0) {
                this.subscription?.remove();
                this.subscription = undefined;
            }
        };
    };
    invalidate() {
        for (const listener of this.listeners)
            listener();
    }
}
function valueSnapshot(value, previous) {
    if (previous !== undefined && Object.is(previous.value, value)) {
        return previous;
    }
    return { value };
}
const readString = (kv, key, previous) => valueSnapshot(kv.getString(key), previous);
const readNumber = (kv, key, previous) => valueSnapshot(kv.getNumber(key), previous);
const readBoolean = (kv, key, previous) => valueSnapshot(kv.getBoolean(key), previous);
const readBuffer = (kv, key) => valueSnapshot(kv.getBuffer(key));
const readJSON = (kv, key, previous) => {
    const json = kv[INTERNAL_GET_JSON_TEXT](key);
    const previousJSON = previous;
    if (previousJSON !== undefined && previousJSON.json === json) {
        return previousJSON;
    }
    return {
        json,
        value: json === undefined ? undefined : JSON.parse(json),
    };
};
const sameValue = (previous, next) => Object.is(previous.value, next.value);
const sameJSON = (previous, next) => previous.json === next.json;
function sameBuffer(previous, next) {
    const a = previous.value;
    const b = next.value;
    if (a === b)
        return true;
    if (a === undefined || b === undefined || a.byteLength !== b.byteLength) {
        return false;
    }
    const left = new Uint8Array(a);
    const right = new Uint8Array(b);
    for (let i = 0; i < left.length; i++) {
        if (left[i] !== right[i])
            return false;
    }
    return true;
}
function useSnapshotSource(key, kv, read, equals) {
    const store = kv ?? getDefaultKV();
    const source = useMemo(() => new KVSnapshotSource(store, key, read, equals), [store, key, read, equals]);
    const snapshot = useSyncExternalStore(source.subscribe, source.getSnapshot, source.getSnapshot);
    return { source, snapshot };
}
function useKVValue(key, kv, read, equals, write) {
    const store = kv ?? getDefaultKV();
    const { source, snapshot } = useSnapshotSource(key, store, read, equals);
    const set = useCallback((next) => {
        if (next === undefined)
            store.delete(key);
        else
            write(store, key, next);
        source.invalidate();
    }, [key, source, store, write]);
    return [snapshot.value, set];
}
const writeValue = (store, key, value) => store.set(key, value);
const writeJSON = (store, key, value) => store.setJSON(key, value);
export function useKVString(key, kv) {
    return useKVValue(key, kv, readString, sameValue, writeValue);
}
export function useKVNumber(key, kv) {
    return useKVValue(key, kv, readNumber, sameValue, writeValue);
}
export function useKVBoolean(key, kv) {
    return useKVValue(key, kv, readBoolean, sameValue, writeValue);
}
export function useKVBuffer(key, kv) {
    return useKVValue(key, kv, readBuffer, sameBuffer, writeValue);
}
export function useKVJSON(key, kv) {
    return useKVValue(key, kv, readJSON, sameJSON, writeJSON);
}
function createSelectionGetter(source, selector, equals, instance) {
    let hasMemo = false;
    let previousSnapshot;
    let previousSelection;
    return () => {
        const snapshot = source.getSnapshot();
        if (!hasMemo) {
            hasMemo = true;
            previousSnapshot = snapshot;
            const nextSelection = selector(snapshot.value);
            if (instance.hasValue && equals(instance.value, nextSelection)) {
                previousSelection = instance.value;
            }
            else {
                previousSelection = nextSelection;
            }
            return previousSelection;
        }
        if (Object.is(previousSnapshot, snapshot))
            return previousSelection;
        const nextSelection = selector(snapshot.value);
        previousSnapshot = snapshot;
        if (equals(previousSelection, nextSelection))
            return previousSelection;
        previousSelection = nextSelection;
        return nextSelection;
    };
}
function useSnapshotSelector(source, selector, equals) {
    const instanceRef = useRef(undefined);
    if (instanceRef.current === undefined) {
        instanceRef.current = { hasValue: false, value: undefined };
    }
    const instance = instanceRef.current;
    const getSelection = useMemo(() => createSelectionGetter(source, selector, equals, instance), [equals, instance, selector, source]);
    const selected = useSyncExternalStore(source.subscribe, getSelection, getSelection);
    useEffect(() => {
        instance.hasValue = true;
        instance.value = selected;
    }, [instance, selected]);
    return selected;
}
export function useKVSelector(key, selector, kv, equals = Object.is) {
    const store = kv ?? getDefaultKV();
    const source = useMemo(() => new KVSnapshotSource(store, key, readJSON, sameJSON), [store, key]);
    return useSnapshotSelector(source, selector, equals);
}
/** @internal Test seam for snapshot identity and selector memoization. */
export const __hookInternals = {
    KVSnapshotSource,
    createSelectionGetter,
    readBuffer,
    readJSON,
    sameBuffer,
    sameJSON,
};
