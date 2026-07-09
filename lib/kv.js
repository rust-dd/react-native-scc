import { NitroModules } from 'react-native-nitro-modules';
let factory;
let platformContext;
function getFactory() {
    factory ??= NitroModules.createHybridObject('SccKv');
    return factory;
}
function getBaseDirectory() {
    platformContext ??=
        NitroModules.createHybridObject('SccKvPlatformContext');
    return platformContext.getBaseDirectory();
}
function getTtlMs(options) {
    const ttl = options?.ttlMs;
    if (ttl === undefined)
        return undefined;
    return getPositiveSafeInteger(ttl, 'ttlMs');
}
function getPositiveSafeInteger(value, name) {
    if (!Number.isSafeInteger(value) || value <= 0) {
        throw new TypeError(`${name} must be a positive safe integer`);
    }
    return value;
}
function getOptionalPositiveSafeInteger(value, name) {
    if (value === undefined)
        return undefined;
    return getPositiveSafeInteger(value, name);
}
function isPromiseLike(value) {
    return (typeof value === 'object' &&
        value !== null &&
        'then' in value &&
        typeof value.then === 'function');
}
const TAG_STR = 0;
const TAG_NUM = 1;
const TAG_BOOL = 2;
const TAG_BYTES = 3;
const TAG_JSON = 4;
function encodeValue(value) {
    if (typeof value === 'string') {
        return { tag: TAG_STR, bytes: new TextEncoder().encode(value) };
    }
    if (typeof value === 'number') {
        const bytes = new Uint8Array(8);
        new DataView(bytes.buffer).setFloat64(0, value, true);
        return { tag: TAG_NUM, bytes };
    }
    if (typeof value === 'boolean') {
        return { tag: TAG_BOOL, bytes: new Uint8Array([value ? 1 : 0]) };
    }
    return { tag: TAG_BYTES, bytes: new Uint8Array(value) };
}
/**
 * Packs a transaction's staged ops into the wire format decoded by the native
 * `applyBatch`: `[u32 count]` then each op `[u8 kind][u32 keyLen][key]` and,
 * for a set (`kind === 1`), `[u8 tag][u32 valLen][val]`. All little-endian.
 */
function encodeBatch(ops) {
    const enc = new TextEncoder();
    const parts = ops.map((op) => ({ ...op, keyBytes: enc.encode(op.key) }));
    let size = 4;
    for (const p of parts) {
        size += 1 + 4 + p.keyBytes.length;
        if (!p.del)
            size += 1 + 4 + p.bytes.length;
    }
    const out = new Uint8Array(size);
    const view = new DataView(out.buffer);
    let off = 0;
    view.setUint32(off, parts.length, true);
    off += 4;
    for (const p of parts) {
        out[off] = p.del ? 0 : 1;
        off += 1;
        view.setUint32(off, p.keyBytes.length, true);
        off += 4;
        out.set(p.keyBytes, off);
        off += p.keyBytes.length;
        if (!p.del) {
            out[off] = p.tag;
            off += 1;
            view.setUint32(off, p.bytes.length, true);
            off += 4;
            out.set(p.bytes, off);
            off += p.bytes.length;
        }
    }
    return out.buffer;
}
class TransactionContext {
    store;
    writes = new Map();
    constructor(store) {
        this.store = store;
    }
    set(key, value) {
        this.writes.set(key, { kind: 'value', value });
    }
    setJSON(key, value) {
        this.writes.set(key, { kind: 'json', value, json: JSON.stringify(value) });
    }
    getString(key) {
        const write = this.writes.get(key);
        if (write !== undefined) {
            return write.kind === 'value' && typeof write.value === 'string'
                ? write.value
                : undefined;
        }
        return this.store.getString(key);
    }
    getNumber(key) {
        const write = this.writes.get(key);
        if (write !== undefined) {
            return write.kind === 'value' && typeof write.value === 'number'
                ? write.value
                : undefined;
        }
        return this.store.getNumber(key);
    }
    getBoolean(key) {
        const write = this.writes.get(key);
        if (write !== undefined) {
            return write.kind === 'value' && typeof write.value === 'boolean'
                ? write.value
                : undefined;
        }
        return this.store.getBoolean(key);
    }
    getBuffer(key) {
        const write = this.writes.get(key);
        if (write !== undefined) {
            return write.kind === 'value' && write.value instanceof ArrayBuffer
                ? write.value
                : undefined;
        }
        return this.store.getBuffer(key);
    }
    getJSON(key) {
        const write = this.writes.get(key);
        if (write !== undefined) {
            return write.kind === 'json' ? write.value : undefined;
        }
        return this.store.getJSON(key);
    }
    contains(key) {
        const write = this.writes.get(key);
        if (write !== undefined)
            return write.kind !== 'delete';
        return this.store.contains(key);
    }
    delete(key) {
        this.writes.set(key, { kind: 'delete' });
    }
    drain() {
        return [...this.writes].map(([key, write]) => ({ key, write }));
    }
}
export class KV {
    native;
    listeners = new Set();
    nativeSubscription;
    keyPrefix;
    constructor(native, keyPrefix = '') {
        this.native = native;
        this.keyPrefix = keyPrefix;
    }
    /**
     * Fires after every mutation of the underlying store — including writes
     * made through other KV objects opened with the same id. `key` is null
     * after clearAll ("everything changed"). Delivery is asynchronous on the
     * JS thread.
     */
    addOnValueChangedListener(listener) {
        this.listeners.add(listener);
        this.nativeSubscription ??= this.native.addListener((key) => {
            const localKey = this.toLocalChangedKey(key ?? null);
            if (localKey === undefined)
                return;
            for (const l of this.listeners)
                l(localKey);
        });
        return {
            remove: () => {
                this.listeners.delete(listener);
                if (this.listeners.size === 0 &&
                    this.nativeSubscription !== undefined) {
                    this.native.removeListener(this.nativeSubscription);
                    this.nativeSubscription = undefined;
                }
            },
        };
    }
    /**
     * Stages reads and writes through `tx`, then commits every staged write as a
     * single atomic native batch (one WAL record — all of it survives a crash, or
     * none of it). The callback must be synchronous; reads see prior staged writes.
     */
    transaction(callback) {
        const tx = new TransactionContext(this);
        const result = callback(tx);
        if (isPromiseLike(result)) {
            throw new TypeError('transaction callback must be synchronous');
        }
        const staged = tx.drain();
        if (staged.length > 0) {
            const ops = staged.map(({ key, write }) => {
                const fullKey = this.fullKey(key);
                if (write.kind === 'delete')
                    return { del: true, key: fullKey };
                if (write.kind === 'json') {
                    return {
                        del: false,
                        key: fullKey,
                        tag: TAG_JSON,
                        bytes: new TextEncoder().encode(write.json),
                    };
                }
                const { tag, bytes } = encodeValue(write.value);
                return { del: false, key: fullKey, tag, bytes };
            });
            this.native.applyBatch(encodeBatch(ops));
        }
        return result;
    }
    namespace(prefix) {
        const normalized = prefix.endsWith(':') ? prefix : `${prefix}:`;
        return new KV(this.native, this.fullKey(normalized));
    }
    getKeysByPrefix(prefix) {
        const fullPrefix = this.fullKey(prefix);
        return this.native.getAllKeys().filter((key) => key.startsWith(fullPrefix));
    }
    deleteByPrefix(prefix) {
        const keys = this.getKeysByPrefix(prefix);
        let removed = 0;
        for (const key of keys) {
            if (this.native.remove(key))
                removed += 1;
        }
        return removed;
    }
    observeJSON(key, selector, listener, equals = Object.is) {
        let selected = selector(this.getJSON(key));
        listener(selected);
        return this.addOnValueChangedListener((changedKey) => {
            if (changedKey !== null && changedKey !== key)
                return;
            const next = selector(this.getJSON(key));
            if (!equals(selected, next)) {
                selected = next;
                listener(next);
            }
        });
    }
    set(key, value, options) {
        const ttl = getTtlMs(options);
        const fullKey = this.fullKey(key);
        if (ttl !== undefined) {
            if (typeof value === 'string')
                this.native.setStringTtl(fullKey, value, ttl);
            else if (typeof value === 'number')
                this.native.setNumberTtl(fullKey, value, ttl);
            else if (typeof value === 'boolean')
                this.native.setBooleanTtl(fullKey, value, ttl);
            else
                this.native.setBufferTtl(fullKey, value, ttl);
            return;
        }
        if (typeof value === 'string')
            this.native.setString(fullKey, value);
        else if (typeof value === 'number')
            this.native.setNumber(fullKey, value);
        else if (typeof value === 'boolean')
            this.native.setBoolean(fullKey, value);
        else
            this.native.setBuffer(fullKey, value);
    }
    setJSON(key, value, options) {
        const ttl = getTtlMs(options);
        const json = JSON.stringify(value);
        const fullKey = this.fullKey(key);
        if (ttl !== undefined) {
            this.native.setJsonTtl(fullKey, json, ttl);
            return;
        }
        this.native.setJson(fullKey, json);
    }
    getString(key) {
        return this.native.getString(this.fullKey(key));
    }
    getNumber(key) {
        return this.native.getNumber(this.fullKey(key));
    }
    getBoolean(key) {
        return this.native.getBoolean(this.fullKey(key));
    }
    getBuffer(key) {
        return this.native.getBuffer(this.fullKey(key));
    }
    getJSON(key) {
        const json = this.native.getJson(this.fullKey(key));
        return json === undefined ? undefined : JSON.parse(json);
    }
    contains(key) {
        return this.native.contains(this.fullKey(key));
    }
    delete(key) {
        return this.native.remove(this.fullKey(key));
    }
    getAllKeys() {
        if (this.keyPrefix === '')
            return this.native.getAllKeys();
        return this.getKeysByPrefix('').map((key) => key.slice(this.keyPrefix.length));
    }
    clearAll() {
        if (this.keyPrefix !== '')
            return this.deleteByPrefix('');
        const removed = this.size;
        this.native.clearAll();
        return removed;
    }
    flush() {
        this.native.flush();
    }
    /** Batch string write — one bridge crossing for the whole record set. */
    setMany(entries) {
        const keys = Object.keys(entries);
        this.native.setManyString(keys.map((key) => this.fullKey(key)), keys.map((k) => entries[k]));
    }
    /** Batch string read; missing keys come back as undefined. */
    getMany(keys) {
        return this.native
            .getManyString(keys.map((key) => this.fullKey(key)))
            .map((v) => v ?? undefined);
    }
    get size() {
        if (this.keyPrefix !== '')
            return this.getAllKeys().length;
        return this.native.size();
    }
    close() {
        this.native.close();
    }
    setAsync(key, value) {
        const fullKey = this.fullKey(key);
        if (typeof value === 'string')
            return this.native.setStringAsync(fullKey, value);
        if (typeof value === 'number')
            return this.native.setNumberAsync(fullKey, value);
        if (typeof value === 'boolean')
            return this.native.setBooleanAsync(fullKey, value);
        return this.native.setBufferAsync(fullKey, value);
    }
    setJSONAsync(key, value) {
        return this.native.setJsonAsync(this.fullKey(key), JSON.stringify(value));
    }
    getStringAsync(key) {
        return this.native.getStringAsync(this.fullKey(key));
    }
    getNumberAsync(key) {
        return this.native.getNumberAsync(this.fullKey(key));
    }
    getBooleanAsync(key) {
        return this.native.getBooleanAsync(this.fullKey(key));
    }
    getBufferAsync(key) {
        return this.native.getBufferAsync(this.fullKey(key));
    }
    async getJSONAsync(key) {
        const json = await this.native.getJsonAsync(this.fullKey(key));
        return json === undefined ? undefined : JSON.parse(json);
    }
    containsAsync(key) {
        return this.native.containsAsync(this.fullKey(key));
    }
    deleteAsync(key) {
        return this.native.removeAsync(this.fullKey(key));
    }
    getAllKeysAsync() {
        if (this.keyPrefix === '')
            return this.native.getAllKeysAsync();
        return Promise.resolve(this.getAllKeys());
    }
    async clearAllAsync() {
        if (this.keyPrefix !== '') {
            this.clearAll();
            return;
        }
        return this.native.clearAllAsync();
    }
    flushAsync() {
        return this.native.flushAsync();
    }
    setManyAsync(entries) {
        const keys = Object.keys(entries);
        return this.native.setManyStringAsync(keys.map((key) => this.fullKey(key)), keys.map((k) => entries[k]));
    }
    async getManyAsync(keys) {
        const values = await this.native.getManyStringAsync(keys.map((key) => this.fullKey(key)));
        return values.map((v) => v ?? undefined);
    }
    fullKey(key) {
        return `${this.keyPrefix}${key}`;
    }
    toLocalChangedKey(key) {
        if (key === null)
            return null;
        if (this.keyPrefix === '')
            return key;
        if (!key.startsWith(this.keyPrefix))
            return undefined;
        return key.slice(this.keyPrefix.length);
    }
}
export function createKV(options = {}) {
    const id = options.id ?? 'default';
    const maxEntries = getOptionalPositiveSafeInteger(options.maxEntries, 'maxEntries');
    const ttlSweepIntervalMs = getOptionalPositiveSafeInteger(options.ttlSweepIntervalMs, 'ttlSweepIntervalMs');
    if (options.persistence === 'none') {
        return new KV(getFactory().inMemory(id, maxEntries, ttlSweepIntervalMs));
    }
    const dir = options.path ?? getBaseDirectory();
    const strict = options.durability === 'strict';
    return new KV(getFactory().open(dir, id, strict, options.recreate ?? false, options.encryptionKey, maxEntries, ttlSweepIntervalMs));
}
let defaultInstance;
export function getDefaultKV() {
    defaultInstance ??= createKV();
    return defaultInstance;
}
