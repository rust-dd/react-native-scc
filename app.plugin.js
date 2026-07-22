const { withDangerousMod, withPlugins } = require('expo/config-plugins')
const path = require('path')
const fs = require('fs')

function getPackageRoot() {
  const pkgJson = require.resolve('react-native-scc-storage/package.json')
  return path.dirname(pkgJson)
}

function buildRustIfMissing(libPath, buildScriptName, platform) {
  if (fs.existsSync(libPath)) return

  let pkgRoot
  try {
    pkgRoot = getPackageRoot()
  } catch {
    console.warn(`[react-native-scc-storage] Could not resolve package root.`)
    return
  }

  const buildScript = path.join(pkgRoot, 'scripts', buildScriptName)
  if (!fs.existsSync(buildScript)) {
    console.warn(
      `[react-native-scc-storage] ${platform} Rust libs not found and build script missing.`
    )
    return
  }

  console.log(
    `[react-native-scc-storage] ${platform} Rust libraries not found. Building...`
  )
  const { execSync } = require('child_process')
  try {
    execSync(`bash "${buildScript}"`, { stdio: 'inherit', cwd: pkgRoot })
  } catch {
    console.warn(
      `[react-native-scc-storage] ${platform} Rust build failed. ` +
        `Run manually: cd node_modules/react-native-scc-storage && npm run rust:build:${platform.toLowerCase()}`
    )
  }
}

function withSccKvIOS(config) {
  return withDangerousMod(config, [
    'ios',
    async (config) => {
      let pkgRoot
      try { pkgRoot = getPackageRoot() } catch { return config }
      const libPath = path.join(pkgRoot, 'ios', 'Libs', 'scc_kv_ffi.xcframework')
      buildRustIfMissing(libPath, 'build-ios.sh', 'iOS')
      return config
    },
  ])
}

function withSccKvAndroid(config) {
  return withDangerousMod(config, [
    'android',
    async (config) => {
      let pkgRoot
      try { pkgRoot = getPackageRoot() } catch { return config }
      const libPath = path.join(pkgRoot, 'android', 'src', 'main', 'jniLibs', 'arm64-v8a', 'libscc_kv_ffi.a')
      buildRustIfMissing(libPath, 'build-android.sh', 'Android')
      return config
    },
  ])
}

function withSccKv(config) {
  return withPlugins(config, [withSccKvIOS, withSccKvAndroid])
}

module.exports = withSccKv
