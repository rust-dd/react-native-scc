const { getDefaultConfig } = require('expo/metro-config')
const path = require('path')

const projectRoot = __dirname
const workspaceRoot = path.resolve(projectRoot, '..')

const config = getDefaultConfig(projectRoot)

config.watchFolders = [workspaceRoot]

config.resolver.nodeModulesPaths = [
  path.resolve(projectRoot, 'node_modules'),
  path.resolve(workspaceRoot, 'node_modules'),
]

config.resolver.blockList = [
  /target\/.*/,
  /ios\/Libs\/.*/,
  /android\/src\/main\/jniLibs\/.*/,
  new RegExp(
    path
      .resolve(workspaceRoot, 'node_modules', 'react')
      .replace(/[/\\]/g, '[/\\\\]') + '[\\/\\\\].*'
  ),
  new RegExp(
    path
      .resolve(workspaceRoot, 'node_modules', 'react-native')
      .replace(/[/\\]/g, '[/\\\\]') + '[\\/\\\\].*'
  ),
  new RegExp(
    path
      .resolve(workspaceRoot, 'node_modules', 'jotai')
      .replace(/[/\\]/g, '[/\\\\]') + '[\\/\\\\].*'
  ),
]

config.resolver.extraNodeModules = {
  jotai: path.resolve(projectRoot, 'node_modules', 'jotai'),
  react: path.resolve(projectRoot, 'node_modules', 'react'),
  'react-native': path.resolve(projectRoot, 'node_modules', 'react-native'),
  'react-native-nitro-modules': path.resolve(
    projectRoot,
    'node_modules',
    'react-native-nitro-modules'
  ),
}

module.exports = config
