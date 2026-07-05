import type { HybridObject } from 'react-native-nitro-modules'

export interface SccKvPlatformContext
  extends HybridObject<{ ios: 'swift'; android: 'kotlin' }> {
  getBaseDirectory(): string
}
