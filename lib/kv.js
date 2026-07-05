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
export class KV {
    native;
    listeners = new Set();
    nativeSubscription;
    constructor(native) {
        this.native = native;
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
            for (const l of this.listeners)
                l(key ?? null);
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
    set(key, value, options) {
        const ttl = options?.ttlMs;
        if (ttl !== undefined) {
            if (typeof value === 'string')
                this.native.setStringTtl(key, value, ttl);
            else if (typeof value === 'number')
                this.native.setNumberTtl(key, value, ttl);
            else if (typeof value === 'boolean')
                this.native.setBooleanTtl(key, value, ttl);
            else
                this.native.setBufferTtl(key, value, ttl);
            return;
        }
        if (typeof value === 'string')
            this.native.setString(key, value);
        else if (typeof value === 'number')
            this.native.setNumber(key, value);
        else if (typeof value === 'boolean')
            this.native.setBoolean(key, value);
        else
            this.native.setBuffer(key, value);
    }
    setJSON(key, value, options) {
        const ttl = options?.ttlMs;
        if (ttl !== undefined) {
            this.native.setJsonTtl(key, JSON.stringify(value), ttl);
            return;
        }
        this.native.setJson(key, JSON.stringify(value));
    }
    getString(key) {
        return this.native.getString(key);
    }
    getNumber(key) {
        return this.native.getNumber(key);
    }
    getBoolean(key) {
        return this.native.getBoolean(key);
    }
    getBuffer(key) {
        return this.native.getBuffer(key);
    }
    getJSON(key) {
        const json = this.native.getJson(key);
        return json === undefined ? undefined : JSON.parse(json);
    }
    contains(key) {
        return this.native.contains(key);
    }
    delete(key) {
        return this.native.remove(key);
    }
    getAllKeys() {
        return this.native.getAllKeys();
    }
    clearAll() {
        this.native.clearAll();
    }
    flush() {
        this.native.flush();
    }
    /** Batch string write — one bridge crossing for the whole record set. */
    setMany(entries) {
        const keys = Object.keys(entries);
        this.native.setManyString(keys, keys.map((k) => entries[k]));
    }
    /** Batch string read; missing keys come back as undefined. */
    getMany(keys) {
        return this.native.getManyString(keys).map((v) => v ?? undefined);
    }
    get size() {
        return this.native.size();
    }
    close() {
        this.native.close();
    }
    setAsync(key, value) {
        if (typeof value === 'string')
            return this.native.setStringAsync(key, value);
        if (typeof value === 'number')
            return this.native.setNumberAsync(key, value);
        if (typeof value === 'boolean')
            return this.native.setBooleanAsync(key, value);
        return this.native.setBufferAsync(key, value);
    }
    setJSONAsync(key, value) {
        return this.native.setJsonAsync(key, JSON.stringify(value));
    }
    getStringAsync(key) {
        return this.native.getStringAsync(key);
    }
    getNumberAsync(key) {
        return this.native.getNumberAsync(key);
    }
    getBooleanAsync(key) {
        return this.native.getBooleanAsync(key);
    }
    getBufferAsync(key) {
        return this.native.getBufferAsync(key);
    }
    async getJSONAsync(key) {
        const json = await this.native.getJsonAsync(key);
        return json === undefined ? undefined : JSON.parse(json);
    }
    containsAsync(key) {
        return this.native.containsAsync(key);
    }
    deleteAsync(key) {
        return this.native.removeAsync(key);
    }
    getAllKeysAsync() {
        return this.native.getAllKeysAsync();
    }
    clearAllAsync() {
        return this.native.clearAllAsync();
    }
    flushAsync() {
        return this.native.flushAsync();
    }
    setManyAsync(entries) {
        const keys = Object.keys(entries);
        return this.native.setManyStringAsync(keys, keys.map((k) => entries[k]));
    }
    async getManyAsync(keys) {
        const values = await this.native.getManyStringAsync(keys);
        return values.map((v) => v ?? undefined);
    }
}
export function createKV(options = {}) {
    const id = options.id ?? 'default';
    if (options.persistence === 'none') {
        return new KV(getFactory().inMemory(id));
    }
    const dir = options.path ?? getBaseDirectory();
    const strict = options.durability === 'strict';
    return new KV(getFactory().open(dir, id, strict, options.recreate ?? false, options.encryptionKey));
}
let defaultInstance;
export function getDefaultKV() {
    defaultInstance ??= createKV();
    return defaultInstance;
}
