# Homebrew formula for Kimberlite
# Install: brew install kimberlitedb/tap/kimberlite
# Or tap first: brew tap kimberlitedb/tap && brew install kimberlite

class Kimberlite < Formula
  desc "Compliance-first database for regulated industries"
  homepage "https://kimberlite.dev"
  version "0.4.0"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/kimberlitedb/kimberlite/releases/download/v#{version}/kimberlite-macos-aarch64.zip"
      sha256 "4d686eaf2603eab8a0dd69675fdc024f938b9abdfd6f3bbf4ffb2570b12ad680"
    end

    on_intel do
      url "https://github.com/kimberlitedb/kimberlite/releases/download/v#{version}/kimberlite-macos-x86_64.zip"
      sha256 "235390d6f43e01ed681e668b890d955f9a275efc148cbe15de2fdb84a8f89330"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/kimberlitedb/kimberlite/releases/download/v#{version}/kimberlite-linux-aarch64.zip"
      sha256 "4de65f912e5b1eb5046ddc341fdb2f04217036de59fdbb9e3d585c3673d02255"
    end

    on_intel do
      url "https://github.com/kimberlitedb/kimberlite/releases/download/v#{version}/kimberlite-linux-x86_64.zip"
      sha256 "f7d03c2c746def044cb012896b9baa6d47802d77083edc87a07773d63f98e300"
    end
  end

  def install
    bin.install "kimberlite"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/kimberlite version")
  end

  def caveats
    <<~EOS
      Kimberlite has been installed!

      Quick start:
        kimberlite init my-project
        cd my-project
        kimberlite dev

      Documentation: https://kimberlite.dev/docs
    EOS
  end
end
