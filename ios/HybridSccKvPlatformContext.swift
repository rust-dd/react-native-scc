import Foundation
import NitroModules

class HybridSccKvPlatformContext: HybridSccKvPlatformContextSpec {
  func getBaseDirectory() throws -> String {
    let urls = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
    guard let support = urls.first else {
      throw RuntimeError.error(withMessage: "Application Support directory not found")
    }
    let base = support.appendingPathComponent("react-native-scc", isDirectory: true)
    try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
    return base.path
  }
}
