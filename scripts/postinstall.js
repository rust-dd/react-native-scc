const { execFileSync } = require('child_process')
const fs = require('fs')
const path = require('path')
const os = require('os')

const ROOT = path.resolve(__dirname, '..')

function isFresh(script) {
  try {
    execFileSync('bash', [path.join(ROOT, 'scripts', script), '--check'], {
      stdio: 'ignore',
    })
    return true
  } catch {
    return false
  }
}

const iosOk = os.platform() !== 'darwin' || isFresh('build-ios.sh')
const androidOk = isFresh('build-android.sh')

if (iosOk && androidOk) {
  process.exit(0)
}

function hasRustToolchain() {
  try {
    execFileSync('rustc', ['--version'], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

function findAndroidNdk() {
  if (process.env.ANDROID_NDK_HOME) return process.env.ANDROID_NDK_HOME
  const sdkRoots = [
    process.env.ANDROID_HOME,
    process.env.ANDROID_SDK_ROOT,
    path.join(os.homedir(), 'Library', 'Android', 'sdk'),
    path.join(os.homedir(), 'Android', 'Sdk'),
  ].filter(Boolean)
  for (const sdk of sdkRoots) {
    const ndkDir = path.join(sdk, 'ndk')
    if (!fs.existsSync(ndkDir)) continue
    const versions = fs
      .readdirSync(ndkDir)
      .filter((d) => !d.startsWith('.'))
      .sort((a, b) => b.localeCompare(a, undefined, { numeric: true }))
    if (versions.length > 0) return path.join(ndkDir, versions[0])
  }
  return null
}

function build(script, label, env) {
  console.log(`[react-native-scc-storage] Building Rust for ${label}...`)
  try {
    execFileSync('bash', [path.join(ROOT, 'scripts', script), '--if-needed'], {
      stdio: 'inherit',
      cwd: ROOT,
      env: { ...process.env, ...env },
    })
  } catch {
    console.warn(
      `[react-native-scc-storage] ${label} build failed. Run: npm run rust:build:${label.toLowerCase()}`
    )
  }
}

if (!hasRustToolchain()) {
  const missing = []
  if (!iosOk) missing.push('iOS')
  if (!androidOk) missing.push('Android')
  console.warn(
    `[react-native-scc-storage] Prebuilt ${missing.join(' and ')} Rust libraries are missing or out of date.\n` +
      '  Install Rust (https://rustup.rs) and run: npm run rust:build\n' +
      '  Or install the package from npm which includes prebuilt binaries.'
  )
  process.exit(0)
}

if (!iosOk) {
  build('build-ios.sh', 'iOS', {})
}

if (!androidOk) {
  const ndk = findAndroidNdk()
  if (ndk) {
    build('build-android.sh', 'Android', { ANDROID_NDK_HOME: ndk })
  }
}
