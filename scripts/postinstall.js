const { execSync } = require('child_process')
const fs = require('fs')
const path = require('path')
const os = require('os')

const ROOT = path.resolve(__dirname, '..')
const IOS_LIB = path.join(ROOT, 'ios', 'Libs', 'scc_kv_ffi.xcframework')
const ANDROID_LIB = path.join(
  ROOT,
  'android',
  'src',
  'main',
  'jniLibs',
  'arm64-v8a',
  'libscc_kv_ffi.a'
)

const iosOk = fs.existsSync(IOS_LIB)
const androidOk = fs.existsSync(ANDROID_LIB)

if (iosOk && androidOk) {
  process.exit(0)
}

function hasRustToolchain() {
  try {
    execSync('rustc --version', { stdio: 'ignore' })
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
      .sort()
      .reverse()
    if (versions.length > 0) return path.join(ndkDir, versions[0])
  }
  return null
}

function build(script, label, env) {
  const scriptPath = path.join(ROOT, 'scripts', script)
  if (!fs.existsSync(scriptPath)) return
  console.log(`[react-native-scc-storage] Building Rust for ${label}...`)
  try {
    execSync(`bash "${scriptPath}"`, {
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
    `[react-native-scc-storage] Prebuilt ${missing.join(' and ')} Rust libraries not found.\n` +
      '  Install Rust (https://rustup.rs) and run: npm run rust:build\n' +
      '  Or install the package from npm which includes prebuilt binaries.'
  )
  process.exit(0)
}

if (os.platform() === 'darwin' && !iosOk) {
  build('build-ios.sh', 'iOS', {})
}

if (!androidOk) {
  const ndk = findAndroidNdk()
  if (ndk) {
    build('build-android.sh', 'Android', { ANDROID_NDK_HOME: ndk })
  }
}
