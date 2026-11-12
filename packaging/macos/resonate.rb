cask "resonate" do
  arch arm: "aarch64", intel: "x86_64_v3"

  version "@VERSION@"
  sha256 arm:   "@ARM64_IMAGE_SHA256@",
         intel: "@X64_IMAGE_SHA256@"

  url "@DOWNLOAD_URL@/resonate-#{version}-#{arch}-macos.dmg"
  name "Resonate"
  desc "Music player for audio enthusiasts"
  homepage "@REPOSITORY_URL@"

  depends_on macos: ">= :ventura"

  app "Resonate.app"
  binary "#{appdir}/Resonate.app/Contents/MacOS/resonate"
  manpage "#{appdir}/Resonate.app/Contents/Resources/man/man1/resonate.1"
  manpage "#{appdir}/Resonate.app/Contents/Resources/man/man5/resonate-config.toml.5"
  bash_completion "#{appdir}/Resonate.app/Contents/Resources/completions/resonate.bash"
  fish_completion "#{appdir}/Resonate.app/Contents/Resources/completions/resonate.fish"
  zsh_completion "#{appdir}/Resonate.app/Contents/Resources/completions/_resonate"
end
