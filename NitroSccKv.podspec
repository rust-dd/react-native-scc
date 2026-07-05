require "json"

package = JSON.parse(File.read(File.join(__dir__, "package.json")))

Pod::Spec.new do |s|
  s.name         = "NitroSccKv"
  s.version      = package["version"]
  s.summary      = package["description"]
  s.homepage     = package["homepage"] || package["repository"]["url"]
  s.license      = package["license"]
  s.authors      = package["author"]

  s.platforms    = { :ios => min_ios_version_supported, :visionos => 1.0 }
  s.source       = { :git => package["repository"]["url"], :tag => "#{s.version}" }

  s.source_files = [
    "ios/**/*.{swift}",
    "ios/**/*.{m,mm}",
    "cpp/**/*.{hpp,cpp,h}",
  ]

  s.vendored_frameworks = "ios/Libs/scc_kv_ffi.xcframework"

  s.pod_target_xcconfig = {
    "HEADER_SEARCH_PATHS" => "$(PODS_TARGET_SRCROOT)/cpp",
  }

  s.script_phase = {
    :name => "Build Rust Library",
    :script => 'if [ ! -d "${PODS_TARGET_SRCROOT}/ios/Libs/scc_kv_ffi.xcframework" ]; then bash "${PODS_TARGET_SRCROOT}/scripts/build-ios.sh"; fi',
    :execution_position => :before_compile,
  }

  load 'nitrogen/generated/ios/NitroSccKv+autolinking.rb'
  add_nitrogen_files(s)

  s.dependency 'React-jsi'
  s.dependency 'React-callinvoker'
  install_modules_dependencies(s)
end
