require "json"

package = JSON.parse(File.read(File.join(__dir__, "package.json")))

Pod::Spec.new do |s|
  s.name         = "NitroSccKv"
  s.version      = package["version"]
  s.summary      = package["description"]
  s.homepage     = package["homepage"] || package["repository"]["url"]
  s.license      = package["license"]
  s.authors      = package["author"]

  s.platforms    = { :ios => min_ios_version_supported }
  s.source       = { :git => package["repository"]["url"], :tag => "v#{s.version}" }

  s.source_files = [
    "ios/**/*.{swift}",
    "ios/**/*.{m,mm}",
    "cpp/**/*.{hpp,cpp,h}",
  ]

  # vendored_frameworks is resolved at pod install, so a missing or stale build must happen now, not in the script phase.
  build_script = File.join(__dir__, "scripts", "build-ios.sh")
  if File.exist?(build_script) && !system("bash", build_script, "--check")
    unless system({ "PATH" => "#{Dir.home}/.cargo/bin:#{ENV['PATH']}" }, "bash", build_script, "--if-needed")
      Pod::UI.warn "[react-native-scc-storage] Building the Rust library failed. Run: npm run rust:build:ios"
    end
  end
  s.vendored_frameworks = "ios/Libs/scc_kv_ffi.xcframework"

  s.pod_target_xcconfig = {
    "HEADER_SEARCH_PATHS" => "$(PODS_TARGET_SRCROOT)/cpp",
  }

  s.script_phase = {
    :name => "Build Rust Library",
    :script => 'export PATH="$HOME/.cargo/bin:$PATH"; bash "${PODS_TARGET_SRCROOT}/scripts/build-ios.sh" --if-needed',
    :execution_position => :before_compile,
  }

  load 'nitrogen/generated/ios/NitroSccKv+autolinking.rb'
  add_nitrogen_files(s)

  s.dependency 'React-jsi'
  s.dependency 'React-callinvoker'
  install_modules_dependencies(s)
end
