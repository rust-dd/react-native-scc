import { KV } from './kv';
interface Snapshot<T> {
    value: T | undefined;
}
type SnapshotReader<T> = (kv: KV, key: string, previous?: Snapshot<T>) => Snapshot<T>;
type SnapshotEquals<T> = (previous: Snapshot<T>, next: Snapshot<T>) => boolean;
declare class KVSnapshotSource<T> {
    private readonly store;
    private readonly key;
    private readonly read;
    private readonly equals;
    private snapshot;
    private readonly listeners;
    private subscription;
    constructor(store: KV, key: string, read: SnapshotReader<T>, equals: SnapshotEquals<T>);
    readonly getSnapshot: () => Snapshot<T>;
    readonly subscribe: (listener: () => void) => (() => void);
    invalidate(): void;
}
interface JSONSnapshot<T> extends Snapshot<T> {
    json: string | undefined;
}
declare function sameBuffer(previous: Snapshot<ArrayBuffer>, next: Snapshot<ArrayBuffer>): boolean;
export declare function useKVString(key: string, kv?: KV): [string | undefined, (value: string | undefined) => void];
export declare function useKVNumber(key: string, kv?: KV): [number | undefined, (value: number | undefined) => void];
export declare function useKVBoolean(key: string, kv?: KV): [boolean | undefined, (value: boolean | undefined) => void];
export declare function useKVBuffer(key: string, kv?: KV): [ArrayBuffer | undefined, (value: ArrayBuffer | undefined) => void];
export declare function useKVJSON<T = unknown>(key: string, kv?: KV): [T | undefined, (value: T | undefined) => void];
interface SelectionInstance<S> {
    hasValue: boolean;
    value: S | undefined;
}
declare function createSelectionGetter<T, S>(source: KVSnapshotSource<T>, selector: (value: T | undefined) => S, equals: (a: S, b: S) => boolean, instance: SelectionInstance<S>): () => S;
export declare function useKVSelector<T = unknown, S = T | undefined>(key: string, selector: (value: T | undefined) => S, kv?: KV, equals?: (a: S, b: S) => boolean): S;
/** @internal Test seam for snapshot identity and selector memoization. */
export declare const __hookInternals: {
    KVSnapshotSource: typeof KVSnapshotSource;
    createSelectionGetter: typeof createSelectionGetter;
    readBuffer: SnapshotReader<ArrayBuffer>;
    readJSON: <T>(kv: KV, key: string, previous?: Snapshot<T>) => JSONSnapshot<T>;
    sameBuffer: typeof sameBuffer;
    sameJSON: <T>(previous: Snapshot<T>, next: Snapshot<T>) => boolean;
};
export {};
