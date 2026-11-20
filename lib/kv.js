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
function serializeJSON(value) {
    const json = JSON.stringify(value);
    if (json === undefined) {
        throw new TypeError('value must be JSON-serializable');
    }
    return json;
}
// Nitro only type-checks ArrayBuffers in debug; release builds would read arbitrary memory.
function asBuffer(value) {
    if (value instanceof ArrayBuffer)
        return value;
    throw new TypeError('value must be a string, number, boolean or ArrayBuffer');
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
function nullsToUndefined(values) {
    for (let index = 0; index < values.length; index++) {
        if (values[index] === null)
            values[index] = undefined;
    }
    return values;
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
export const INTERNAL_GET_JSON_TEXT = Symbol('scc.getJSONText');
export const INTERNAL_IS_CLOSED = Symbol('scc.isClosed');
export const INTERNAL_SET_JSON = Symbol('scc.setJSON');
const textEncoder = typeof TextEncoder === 'undefined' ? undefined : new TextEncoder();
/** UTF-8 encode with a manual fallback: JSC and Hermes < RN 0.74 lack TextEncoder. */
function utf8Encode(value) {
    if (textEncoder !== undefined)
        return textEncoder.encode(value);
    const out = [];
    for (let i = 0; i < value.length; i++) {
        let cp = value.codePointAt(i);
        if (cp > 0xffff)
            i++;
        else if (cp >= 0xd800 && cp <= 0xdfff)
            cp = 0xfffd;
        if (cp < 0x80)
            out.push(cp);
        else if (cp < 0x800)
            out.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f));
        else if (cp < 0x10000)
            out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
        else
            out.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3f), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
    }
    return new Uint8Array(out);
}
function encodeValue(value) {
    if (typeof value === 'string') {
        return { tag: TAG_STR, bytes: utf8Encode(value) };
    }
    if (typeof value === 'number') {
        const bytes = new Uint8Array(8);
        new DataView(bytes.buffer).setFloat64(0, value, true);
        return { tag: TAG_NUM, bytes };
    }
    if (typeof value === 'boolean') {
        return { tag: TAG_BOOL, bytes: new Uint8Array([value ? 1 : 0]) };
    }
    return { tag: TAG_BYTES, bytes: new Uint8Array(asBuffer(value)) };
}
// Native applyBatch format, little-endian: `[u32 count]`, then per op `[u8 kind][u32 keyLen][key]`,
// plus `[u8 tag][u32 valLen][val]` for a set (kind 1).
function encodeBatch(ops) {
    const parts = ops.map((op) => ({ ...op, keyBytes: utf8Encode(op.key) }));
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
    committed = false;
    constructor(store) {
        this.store = store;
    }
    assertOpen() {
        if (this.committed)
            throw new Error('transaction already committed');
    }
    set(key, value) {
        this.assertOpen();
        // Copy staged buffers: kv.set marshals at call time, so later caller-side
        // mutation must not leak into the commit here either.
        const primitive = typeof value === 'string' ||
            typeof value === 'number' ||
            typeof value === 'boolean';
        this.writes.set(key, {
            kind: 'value',
            value: primitive ? value : asBuffer(value).slice(0),
        });
    }
    setJSON(key, value) {
        this.assertOpen();
        this.writes.set(key, { kind: 'json', json: serializeJSON(value) });
    }
    getString(key) {
        this.assertOpen();
        const write = this.writes.get(key);
        if (write !== undefined) {
            return write.kind === 'value' && typeof write.value === 'string'
                ? write.value
                : undefined;
        }
        return this.store.getString(key);
    }
    getNumber(key) {
        this.assertOpen();
        const write = this.writes.get(key);
        if (write !== undefined) {
            return write.kind === 'value' && typeof write.value === 'number'
                ? write.value
                : undefined;
        }
        return this.store.getNumber(key);
    }
    getBoolean(key) {
        this.assertOpen();
        const write = this.writes.get(key);
        if (write !== undefined) {
            return write.kind === 'value' && typeof write.value === 'boolean'
                ? write.value
                : undefined;
        }
        return this.store.getBoolean(key);
    }
    getBuffer(key) {
        this.assertOpen();
        const write = this.writes.get(key);
        if (write !== undefined) {
            return write.kind === 'value' && write.value instanceof ArrayBuffer
                ? write.value.slice(0)
                : undefined;
        }
        return this.store.getBuffer(key);
    }
    getJSON(key) {
        this.assertOpen();
        const write = this.writes.get(key);
        if (write !== undefined) {
            // Parse the staged snapshot so reads match what commit will write and
            // mutating the returned object cannot desync them (KV.getJSON parity).
            return write.kind === 'json' ? JSON.parse(write.json) : undefined;
        }
        return this.store.getJSON(key);
    }
    contains(key) {
        this.assertOpen();
        const write = this.writes.get(key);
        if (write !== undefined)
            return write.kind !== 'delete';
        return this.store.contains(key);
    }
    delete(key) {
        this.assertOpen();
        this.writes.set(key, { kind: 'delete' });
    }
    drain() {
        this.committed = true;
        return [...this.writes].map(([key, write]) => ({ key, write }));
    }
}
function nativeCalls(native) {
    const create = native
        .createFastPath;
    return typeof create === 'function' ? create.call(native) : native;
}
/** State a root store shares with its namespace views: one native listener, one fast path. */
class StoreHub {
    native;
    calls;
    members = new Set();
    views = new Map();
    subscription;
    deliveries = 0;
    closed = false;
    constructor(native, calls) {
        this.native = native;
        this.calls = calls;
    }
}
let defaultInstance;
let defaultOptions = {};
export class KV {
    ownsNative;
    storeKey;
    native;
    calls;
    listeners = new Set();
    keyListeners = new Map();
    hub;
    keyPrefix;
    constructor(native, keyPrefix = '', ownsNative = true, storeKey, hub) {
        this.ownsNative = ownsNative;
        this.storeKey = storeKey;
        this.hub = hub ?? new StoreHub(native, nativeCalls(native));
        this.native = native;
        this.keyPrefix = keyPrefix;
        this.calls = this.hub.calls;
        if (keyPrefix === '' && this.calls !== native) {
            // Fast-path functions ignore `this`, so a root store can expose them directly.
            this.getString = this.calls.getString;
            this.getNumber = this.calls.getNumber;
            this.getBoolean = this.calls.getBoolean;
            this.contains = this.calls.contains;
            this.delete = this.calls.remove;
        }
    }
    /** Fires asynchronously on the JS thread after every mutation from any handle; `key` is null after clearAll. */
    addOnValueChangedListener(listener) {
        return this.subscribe(listener, undefined);
    }
    /** Like addOnValueChangedListener, but only for `key` (and clearAll, as null). */
    addOnKeyChangedListener(key, listener) {
        return this.subscribe(listener, key);
    }
    subscribe(listener, key) {
        const hub = this.hub;
        if (hub.closed)
            throw new Error('Cannot subscribe to a closed KV instance');
        const entry = { listener, since: hub.deliveries };
        const entries = key === undefined ? this.listeners : this.keyEntries(key);
        entries.add(entry);
        hub.members.add(this);
        try {
            hub.subscription ??= this.native.addListener(() => KV.deliver(hub));
        }
        catch (error) {
            this.unsubscribe(entries, entry, key);
            throw error;
        }
        return { remove: () => this.unsubscribe(entries, entry, key) };
    }
    keyEntries(key) {
        let entries = this.keyListeners.get(key);
        if (entries === undefined) {
            entries = new Set();
            this.keyListeners.set(key, entries);
        }
        return entries;
    }
    unsubscribe(entries, entry, key) {
        entries.delete(entry);
        if (key !== undefined &&
            entries.size === 0 &&
            this.keyListeners.get(key) === entries) {
            this.keyListeners.delete(key);
        }
        if (this.listeners.size > 0 || this.keyListeners.size > 0)
            return;
        const hub = this.hub;
        hub.members.delete(this);
        if (hub.members.size === 0 && hub.subscription !== undefined) {
            this.native.removeListener(hub.subscription);
            hub.subscription = undefined;
        }
    }
    static deliver(hub) {
        const id = hub.subscription;
        if (id === undefined)
            return;
        const changes = hub.native.takeChanges(id);
        // Entries subscribed during this delivery have since === generation and are skipped.
        const generation = ++hub.deliveries;
        const failure = { failed: false };
        for (const member of hub.members)
            member.dispatch(changes, generation, failure);
        if (failure.failed)
            throw failure.error;
    }
    dispatch(changes, generation, failure) {
        const notify = (entry, key) => {
            if (entry.since >= generation)
                return;
            try {
                entry.listener(key);
            }
            catch (error) {
                if (!failure.failed)
                    Object.assign(failure, { error, failed: true });
            }
        };
        let cleared = false;
        let touched;
        for (const changed of changes) {
            const key = this.toLocalChangedKey(changed);
            if (key === undefined)
                continue;
            for (const entry of this.listeners)
                notify(entry, key);
            if (key === null)
                cleared = true;
            else if (this.keyListeners.has(key))
                (touched ??= new Set()).add(key);
        }
        // Key listeners re-read the value, so one call per key per delivery is enough.
        if (cleared) {
            for (const entries of this.keyListeners.values()) {
                for (const entry of entries)
                    notify(entry, null);
            }
        }
        else if (touched !== undefined) {
            for (const key of touched) {
                const entries = this.keyListeners.get(key);
                if (entries !== undefined)
                    for (const entry of entries)
                        notify(entry, key);
            }
        }
    }
    /** Commits staged writes as one crash-atomic WAL record (readers may see keys land one by one); the callback must be sync.
     *  If the commit throws, no listener fires and memory may be ahead of what survives a restart. */
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
                        bytes: utf8Encode(write.json),
                    };
                }
                const { tag, bytes } = encodeValue(write.value);
                return { del: false, key: fullKey, tag, bytes };
            });
            this.native.applyBatch(encodeBatch(ops));
        }
        return result;
    }
    /** Views are cached per prefix, so a namespace created during render keeps its identity. */
    namespace(prefix) {
        const normalized = prefix.endsWith(':') ? prefix : `${prefix}:`;
        const fullPrefix = this.fullKey(normalized);
        let view = this.hub.views.get(fullPrefix);
        if (view === undefined) {
            view = new KV(this.native, fullPrefix, false, undefined, this.hub);
            this.hub.views.set(fullPrefix, view);
        }
        return view;
    }
    getKeysByPrefix(prefix) {
        return this.toLocalKeys(this.native.getKeysByPrefix(this.fullKey(prefix)));
    }
    /** Deletes the matching snapshot as one native crash-atomic WAL batch. */
    deleteByPrefix(prefix) {
        return this.native.deleteByPrefix(this.fullKey(prefix));
    }
    observeJSON(key, selector, listener, equals = Object.is) {
        let selected;
        let initialized = false;
        const subscription = this.addOnKeyChangedListener(key, () => {
            const next = selector(this.getJSON(key));
            if (!initialized || !equals(selected, next)) {
                initialized = true;
                selected = next;
                listener(next);
            }
        });
        try {
            selected = selector(this.getJSON(key));
            initialized = true;
            listener(selected);
            return subscription;
        }
        catch (error) {
            subscription.remove();
            throw error;
        }
    }
    set(key, value, options) {
        const fullKey = this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`;
        const ttl = options === undefined ? undefined : getTtlMs(options);
        if (ttl !== undefined) {
            if (typeof value === 'string')
                this.native.setStringTtl(fullKey, value, ttl);
            else if (typeof value === 'number')
                this.native.setNumberTtl(fullKey, value, ttl);
            else if (typeof value === 'boolean')
                this.native.setBooleanTtl(fullKey, value, ttl);
            else
                this.native.setBufferTtl(fullKey, asBuffer(value), ttl);
            return;
        }
        if (typeof value === 'string')
            this.calls.setString(fullKey, value);
        else if (typeof value === 'number')
            this.calls.setNumber(fullKey, value);
        else if (typeof value === 'boolean')
            this.calls.setBoolean(fullKey, value);
        else
            this.native.setBuffer(fullKey, asBuffer(value));
    }
    setJSON(key, value, options) {
        const ttl = getTtlMs(options);
        const json = serializeJSON(value);
        const fullKey = this.fullKey(key);
        if (ttl !== undefined) {
            this.native.setJsonTtl(fullKey, json, ttl);
            return;
        }
        this.native.setJson(fullKey, json);
    }
    [INTERNAL_SET_JSON](key, value) {
        const json = serializeJSON(value);
        this.native.setJson(this.fullKey(key), json);
        return json;
    }
    getString(key) {
        return this.calls.getString(this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`);
    }
    getNumber(key) {
        return this.calls.getNumber(this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`);
    }
    getBoolean(key) {
        return this.calls.getBoolean(this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`);
    }
    getBuffer(key) {
        return this.native.getBuffer(this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`);
    }
    getJSON(key) {
        const json = this[INTERNAL_GET_JSON_TEXT](key);
        return json === undefined ? undefined : JSON.parse(json);
    }
    [INTERNAL_GET_JSON_TEXT](key) {
        return this.calls.getJson(this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`);
    }
    contains(key) {
        return this.calls.contains(this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`);
    }
    delete(key) {
        return this.calls.remove(this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`);
    }
    getAllKeys() {
        if (this.keyPrefix === '')
            return this.native.getAllKeys();
        return this.getKeysByPrefix('');
    }
    clearAll() {
        if (this.keyPrefix !== '')
            return this.deleteByPrefix('');
        return this.native.clearAll();
    }
    flush() {
        this.native.flush();
    }
    /** Batch string write — one bridge crossing for the whole record set. */
    setMany(entries) {
        const keys = Object.keys(entries);
        if (keys.length === 0)
            return;
        this.calls.setManyString(this.fullKeys(keys), Object.values(entries));
    }
    /** Batch string read; missing keys come back as undefined. */
    getMany(keys) {
        if (keys.length === 0)
            return [];
        return nullsToUndefined(this.calls.getManyString(this.fullKeys(keys)));
    }
    get size() {
        if (this.keyPrefix !== '')
            return this.native.countByPrefix(this.keyPrefix);
        return this.native.size();
    }
    /** Closes the root store and its namespace views; a view itself cannot be closed. */
    close() {
        if (!this.ownsNative) {
            throw new Error('Cannot close a KV namespace; close its root KV instance');
        }
        if (this.hub.closed)
            return;
        this.detach();
        try {
            this.native.close();
        }
        finally {
            this.hub.closed = true;
            const fallback = defaultInstance;
            if (fallback === this) {
                defaultInstance = undefined;
            }
            else if (fallback !== undefined && fallback.storeKey === this.storeKey) {
                defaultInstance = undefined;
                fallback.abandon();
            }
        }
    }
    get [INTERNAL_IS_CLOSED]() {
        return this.hub.closed;
    }
    detach() {
        const hub = this.hub;
        if (hub.subscription !== undefined) {
            this.native.removeListener(hub.subscription);
            hub.subscription = undefined;
        }
        const entries = [];
        for (const member of hub.members) {
            entries.push(...member.listeners);
            for (const keyed of member.keyListeners.values())
                entries.push(...keyed);
            member.listeners.clear();
            member.keyListeners.clear();
        }
        hub.members.clear();
        return entries;
    }
    // Closed through another handle; subscribers must re-resolve the default.
    abandon() {
        const entries = this.detach();
        this.hub.closed = true;
        queueMicrotask(() => {
            let failure;
            for (const entry of entries) {
                try {
                    entry.listener(null);
                }
                catch (error) {
                    failure ??= { error };
                }
            }
            if (failure !== undefined)
                throw failure.error;
        });
    }
    async setAsync(key, value) {
        const fullKey = this.fullKey(key);
        if (typeof value === 'string')
            return this.native.setStringAsync(fullKey, value);
        if (typeof value === 'number')
            return this.native.setNumberAsync(fullKey, value);
        if (typeof value === 'boolean')
            return this.native.setBooleanAsync(fullKey, value);
        return this.native.setBufferAsync(fullKey, asBuffer(value));
    }
    async setJSONAsync(key, value) {
        const json = serializeJSON(value);
        return this.native.setJsonAsync(this.fullKey(key), json);
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
    async getAllKeysAsync() {
        if (this.keyPrefix === '')
            return this.native.getAllKeysAsync();
        return this.toLocalKeys(await this.native.getKeysByPrefixAsync(this.keyPrefix));
    }
    async clearAllAsync() {
        if (this.keyPrefix !== '') {
            await this.native.deleteByPrefixAsync(this.keyPrefix);
            return;
        }
        return this.native.clearAllAsync();
    }
    flushAsync() {
        return this.native.flushAsync();
    }
    setManyAsync(entries) {
        const keys = Object.keys(entries);
        if (keys.length === 0)
            return Promise.resolve();
        return this.native.setManyStringAsync(this.fullKeys(keys), Object.values(entries));
    }
    async getManyAsync(keys) {
        if (keys.length === 0)
            return [];
        return nullsToUndefined(await this.native.getManyStringAsync(this.fullKeys(keys)));
    }
    fullKey(key) {
        return this.keyPrefix === '' ? key : `${this.keyPrefix}${key}`;
    }
    fullKeys(keys) {
        if (this.keyPrefix === '')
            return keys;
        return keys.map((key) => `${this.keyPrefix}${key}`);
    }
    toLocalKeys(keys) {
        if (this.keyPrefix === '')
            return keys;
        const start = this.keyPrefix.length;
        return keys.map((key) => key.slice(start));
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
        return new KV(getFactory().inMemory(id, options.recreate ?? false, maxEntries, ttlSweepIntervalMs), '', true, `mem\0${id}`);
    }
    if (options.encryptionKey === '') {
        throw new TypeError('encryptionKey must not be empty');
    }
    const dir = options.path ?? getBaseDirectory();
    const strict = options.durability === 'strict';
    return new KV(getFactory().open(dir, id, strict, options.recreate ?? false, options.encryptionKey, maxEntries, ttlSweepIntervalMs), '', true, `disk\0${dir}\0${id}`);
}
/** Sets the options hooks and adapters without an explicit `kv` open their store with. */
export function configureDefaultKV(options) {
    if (options.recreate !== undefined) {
        throw new TypeError('configureDefaultKV does not accept recreate');
    }
    if (defaultInstance !== undefined) {
        throw new Error('the default KV store is already open; configure it before first use');
    }
    defaultOptions = { ...options };
}
export function getDefaultKV() {
    defaultInstance ??= createKV(defaultOptions);
    return defaultInstance;
}
