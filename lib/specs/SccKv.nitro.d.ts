import type { HybridObject } from 'react-native-nitro-modules';
import type { SccKvInstance } from './SccKvInstance.nitro';
export interface SccKv extends HybridObject<{
    ios: 'c++';
    android: 'c++';
}> {
    open(dir: string, id: string, strictDurability: boolean, recreate: boolean, encryptionKey?: string): SccKvInstance;
    inMemory(id: string): SccKvInstance;
}
