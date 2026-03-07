# Release signing

Every artifact published from the automated release workflow is signed.
Signatures prove that a given file was produced by this repository's
`.github/workflows/release.yml` workflow rather than by a third party,
a compromised mirror, or a local build on someone else's machine.

## Phase 1: Sigstore cosign

The current signing path is keyless Sigstore signing. There is no long-lived
private key stored in the repository or in GitHub Actions secrets.

When the release workflow runs on GitHub Actions, it requests a short-lived
certificate from Fulcio and binds that certificate to the workflow identity
using GitHub's OIDC token. Published releases are signed from:

```text
https://github.com/aistar-au/vexcoder/.github/workflows/release.yml
@refs/tags/<version>
```

The certificate and signature are recorded in Rekor, Sigstore's public
transparency log. Each published artifact ships with a `.sigstore.json`
bundle file alongside the archive and `.sha256` checksum file.

### Verify a release download

Install `cosign` once:

```sh
# macOS
brew install cosign

# Windows
winget install sigstore.cosign
```

On Linux, install `cosign` with your distro package manager or from the
official Sigstore installation instructions.

Then verify an archive:

```sh
cosign verify-blob \
  vex-<version>-<triple>.tar.gz \
  --bundle vex-<version>-<triple>.tar.gz.sigstore.json \
  --certificate-identity \
    "https://github.com/aistar-au/vexcoder/.github/workflows/release.yml@refs/tags/<version>" \
  --certificate-oidc-issuer \
    "https://token.actions.githubusercontent.com"
```

For Windows zip artifacts, replace both `.tar.gz` paths with `.zip`.

A successful verification reports:

```text
Verified OK
```

That confirms the file matches what the release workflow produced.

If `cosign` is unavailable, you can still verify the detached SHA-256 file:

```sh
sha256sum -c vex-<version>-<triple>.tar.gz.sha256
```

On Windows:

```powershell
Get-FileHash vex-<version>-<triple>.zip -Algorithm SHA256
```

### Workflow-dispatch smoke tests

Manual `workflow_dispatch` runs also produce signatures, but those signatures
bind to the ref that triggered the run instead of a release tag. The published
GitHub Releases verification flow above applies to tagged releases.

## Phase 2: SignPath.io OSS code signing

The planned next step for Windows is SignPath.io's open-source program. That
adds Authenticode signing backed by a public CA while keeping the release flow
fully automated in GitHub Actions.

This improves the Windows trust story, but it does not provide immediate
SmartScreen reputation by itself. The Sigstore bundle can still be published
alongside the signed artifact for supply-chain verification.

## Phase 3: EV Authenticode

For wider Windows end-user distribution, the final signing phase is an EV
code-signing certificate from a provider such as DigiCert or Sectigo.
That requires hardware-backed key storage and stricter identity checks, but it
is the path that gives the fastest SmartScreen trust for new downloads.

## Cloudflare TLS vs package signing

Cloudflare certificates secure HTTPS for the website and docs. They are TLS
certificates, not code-signing certificates.

Package signing uses a different certificate purpose and trust chain:

- TLS website certificate: `EKU 1.3.6.1.5.5.7.3.1`
- Code-signing certificate: `EKU 1.3.6.1.5.5.7.3.3`

The release download is already protected in transit by GitHub's TLS
certificate. The `.sigstore.json` bundle adds an independent proof that the
artifact itself came from this repository's release workflow.
