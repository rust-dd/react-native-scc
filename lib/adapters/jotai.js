import { atomWithStorage } from 'jotai/utils';
import { getDefaultKV, INTERNAL_GET_JSON_TEXT, INTERNAL_IS_CLOSED, INTERNAL_SET_JSON, } from '../kv';
/**
 * Atom persisted in a KV instance. Reads synchronously on init and
 * reacts to writes made outside jotai (other KV handles included).
 * Setting it to `undefined` deletes the key.
 */
export function atomWithKV(key, initialValue, kv) {
    const store = () => kv ?? getDefaultKV();
    // The change event of our own write must hand back the written object, not a reparsed copy.
    let written;
    return atomWithStorage(key, initialValue, {
        getItem: (k, fallback) => {
            const value = store().getJSON(k);
            return value === undefined ? fallback : value;
        },
        setItem: (k, value) => {
            if (value === undefined) {
                written = { json: undefined, value };
                store().delete(k);
                return;
            }
            written = { json: store()[INTERNAL_SET_JSON](k, value), value };
        },
        removeItem: (k) => {
            written = undefined;
            store().delete(k);
        },
        subscribe: (k, callback, fallback) => {
            let subscription;
            const listen = () => {
                const target = store();
                subscription = target.addOnKeyChangedListener(k, () => {
                    if (kv === undefined && target[INTERNAL_IS_CLOSED])
                        listen();
                    const json = store()[INTERNAL_GET_JSON_TEXT](k);
                    if (written !== undefined && written.json === json) {
                        callback(written.value);
                        return;
                    }
                    written = undefined;
                    callback(json === undefined ? fallback : JSON.parse(json));
                });
            };
            listen();
            return () => subscription.remove();
        },
    }, { getOnInit: true });
}
