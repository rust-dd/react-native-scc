package com.margelo.nitro.scckv

import com.margelo.nitro.NitroModules
import java.io.File

class HybridSccKvPlatformContext : HybridSccKvPlatformContextSpec() {
  override fun getBaseDirectory(): String {
    val context = NitroModules.applicationContext
      ?: throw Error("react-native-scc: no application context")
    val dir = File(context.filesDir, "react-native-scc")
    dir.mkdirs()
    return dir.absolutePath
  }
}
