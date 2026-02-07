# Homebrew formula for Kimberlite
# Install: brew install kimberlitedb/tap/kimberlite
# Or tap first: brew tap kimberlitedb/tap && brew install kimberlite

class Kimberlite < Formula
  desc "Compliance-first database for regulated industries"
  homepage "https://kimberlite.dev"
  version "0.6.0"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/kimberlitedb/kimberlite/releases/download/v#{version}/kimberlite-macos-aarch64.zip"
      sha256 "PLACEHOLDER_MACOS_ARM64_SHA256"
    end

    on_intel do
      url "https://github.com/kimberlitedb/kimberlite/releases/download/v#{version}/kimberlite-macos-x86_64.zip"
      sha256 "PLACEHOLDER_MACOS_X86_64_SHA256"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/kimberlitedb/kimberlite/releases/download/v#{version}/kimberlite-linux-aarch64.zip"
      sha256 "PLACEHOLDER_LINUX_ARM64_SHA256"
    end

    on_intel do
      url "https://github.com/kimberlitedb/kimberlite/releases/download/v#{version}/kimberlite-linux-x86_64.zip"
      sha256 "PLACEHOLDER_LINUX_X86_64_SHA256"
    end
  end

  def install
    bin.install "kimberlite"
    bin.install_symlink "kimberlite" => "kmb"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/kimberlite version")
  end

  def caveats
    <<~EOS
      Kimberlite has been installed!

      Quick start:
        kmb init my-project
        cd my-project
        kmb dev

      Documentation: https://kimberlite.dev/docs
    EOS
  end
end
