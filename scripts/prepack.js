const { execSync } = require('child_process')

const path = require('path')

const ROOT = path.resolve(__dirname, '..')

function run(cmd) {
  console.log(`> ${cmd}`)
  execSync(cmd, { stdio: 'inherit', cwd: ROOT })
}

console.log('[prepack] Building iOS Rust libraries...')
run('bash scripts/build-ios.sh')

console.log('[prepack] Building Android Rust libraries...')
run('bash scripts/build-android.sh')

console.log('[prepack] Building TypeScript...')
run('npx tsc')

console.log('[prepack] Done.')
