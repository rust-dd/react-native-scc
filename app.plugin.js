const { withDangerousMod, withPlugins } = require('expo/config-plugins')
const path = require('path')
const fs = require('fs')

function getPackageRoot() {
  const pkgJson = require.resolve('react-native-scc-storage/package.json')
  return path.dirname(pkgJson)
}

function buildRustIfNeeded(buildScriptName, platform) {
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
      `[react-native-scc-storage] ${platform} build script missing.`
    )
    return
  }

  const { execFileSync } = require('child_process')
  try {
    execFileSync('bash', [buildScript, '--if-needed'], { stdio: 'inherit', cwd: pkgRoot })
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
      buildRustIfNeeded('build-ios.sh', 'iOS')
      return config
    },
  ])
}

function withSccKvAndroid(config) {
  return withDangerousMod(config, [
    'android',
    async (config) => {
      buildRustIfNeeded('build-android.sh', 'Android')
      return config
    },
  ])
}

function withSccKv(config) {
  return withPlugins(config, [withSccKvIOS, withSccKvAndroid])
}

module.exports = withSccKv
