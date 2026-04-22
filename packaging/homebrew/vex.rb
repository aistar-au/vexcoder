# Homebrew formula for vexapi — ADR-024 Phase G (PG-03).
#
# This template is maintained in the main vexapi repository under
# packaging/homebrew/vex.rb.  At release time scripts/update_homebrew_formula.py
# substitutes version and sha256 values and pushes the result to the
# aistar-au/homebrew-vex tap repository.
#
# Manual installation from a published tag:
#   brew tap aistar-au/vex
#   brew install aistar-au/vex/vex
#
# All placeholder values of the form __PLACEHOLDER__ are replaced by the
# update script.  Do not edit them manually.

class Vex < Formula
  desc "Zero-licensing-cost coding agent with TUI and headless batch mode"
  homepage "https://github.com/aistar-au/vexapi"
  license "MIT"
  version "__VERSION__"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/aistar-au/vexapi/releases/download/__VERSION__/vex-__VERSION__-aarch64-apple-darwin.tar.gz"
      sha256 "__SHA256_AARCH64_APPLE_DARWIN__"
    else
      url "https://github.com/aistar-au/vexapi/releases/download/__VERSION__/vex-__VERSION__-x86_64-apple-darwin.tar.gz"
      sha256 "__SHA256_X86_64_APPLE_DARWIN__"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/aistar-au/vexapi/releases/download/__VERSION__/vex-__VERSION__-aarch64-unknown-linux-musl.tar.gz"
      sha256 "__SHA256_AARCH64_UNKNOWN_LINUX_MUSL__"
    else
      url "https://github.com/aistar-au/vexapi/releases/download/__VERSION__/vex-__VERSION__-x86_64-unknown-linux-musl.tar.gz"
      sha256 "__SHA256_X86_64_UNKNOWN_LINUX_MUSL__"
    end
  end

  def install
    bin.install "vex"
  end

  def caveats
    <<~EOS
      Set your model endpoint token before starting vexapi:
        export VEX_MODEL_TOKEN=<your-token>

      For the native macOS app from the release .dmg, store the token in the macOS keychain:
        security add-generic-password -s vexapi -a VEX_MODEL_TOKEN -w <token>

      Scaffold a new project workspace:
        vex init

      For non-interactive batch use:
        vex exec --task "describe what to do" --format jsonl
    EOS
  end

  test do
    assert_match "vexapi", shell_output("#{bin}/vex --help 2>&1")
  end
end
