# Releases

Versions follow [Semantic Versioning](https://semver.org). Each release on GitHub contains:

| File | What it is |
|---|---|
| `kudari-X.Y.Z.tar.xz` | the source, with all Rust crates, so it builds without network |
| `kudari-X.Y.Z-x86_64-linux.tar.xz` | the installed files under `usr/`, built on Arch Linux |
| `kudari-X.Y.Z.cdx.json`, `kudari-X.Y.Z.spdx.json` | SBOMs of the source in CycloneDX and SPDX |
| `kudari-X.Y.Z-x86_64-windows.msix` | the package for Windows 11 |
| `kudari-X.Y.Z-x86_64-windows.zip` | the same program to unpack and run, without installing |
| `kudari-X.Y.Z-x86_64-windows.cer` | the certificate that signs both |
| `kudari-X.Y.Z-x86_64-windows.cdx.json` | the SBOM of the Windows packages: the crates, Qt and the runtime of Visual C++ |
| `MarcJose.Kudari*.yaml` | the manifests for winget |
| `SHA256SUMS`, `SHA512SUMS` | checksums of all files |
| `*.sigstore.json` | the Sigstore signatures of the checksums |

## Checking a download

The checksums are signed without a key through Sigstore; the signature names the workflow
and the tag that made them. With [cosign](https://docs.sigstore.dev/cosign/), check the
signature, then the files:

```sh
cosign verify-blob SHA256SUMS --bundle SHA256SUMS.sigstore.json \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com \
    --certificate-identity-regexp '^https://github.com/MarcJose/kudari/\.github/workflows/release\.yml@refs/tags/v'
sha256sum --check --ignore-missing SHA256SUMS
```

The source, the package and the SBOMs also have a build provenance attestation, which the
GitHub CLI checks:

```sh
gh attestation verify kudari-1.2.0.tar.xz --repo MarcJose/kudari
```

## The Windows packages

The program in both and the MSIX package are signed with a self-signed certificate of
Kudari, valid for three years; every release has the same one until it is renewed. Windows
installs an MSIX package only from a publisher it trusts, so the certificate has to be
trusted first, as the README shows. Check the `.cer` file like any other download, through
`SHA256SUMS` and its signature above, and on Windows that it signed the package:

```powershell
Get-AuthenticodeSignature kudari-1.2.0-x86_64-windows.msix | Format-List Status, SignerCertificate
```

The packages bring Qt and the runtime of Visual C++ along, with their licenses under
`licenses\`. They are not reproducible: the compiler of Microsoft writes the time of the
build into the program.

winget does not trust the certificate either, so its manifests install the ZIP archive as a
portable program and put its directory on the path; the command is `kudari`. They
are not submitted on their own: a pull request to
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) under
`manifests/m/MarcJose/Kudari/X.Y.Z/` takes them in.

## Rebuilding the package

The package is reproducible. On Arch Linux, it can be rebuilt from the source tarball:

```sh
tools/dist.sh binary kudari-1.2.0.tar.xz out/
```

With the same versions of Rust, Qt and the other Arch packages as the release build, it
matches the published one byte for byte, wherever it is built. The release workflow checks
that with two builds on separate runners before it publishes.

## Making a release

Commit messages follow Conventional Commits, as [CONTRIBUTING.md](../CONTRIBUTING.md)
describes, so the changelog and the next version can be made from them. A release is
prepared with

```sh
tools/release.py            # or tools/release.py 1.2.0 to choose the version
```

It takes the next version from the commits since the last release (git-cliff), writes it
into `Cargo.toml`, `Cargo.lock`, the metainfo and the man page (CMake and the program read it
from `Cargo.toml`), adds the section of the release to `CHANGELOG.md` from the commits, and
commits and tags. A section written by hand beforehand is kept. `tools/release.py --check`
tells whether all of these agree; `tests/check.sh` runs it.

Pushing the tag starts `.github/workflows/release.yml`. It runs the CI and security checks,
builds the source tarball with all crates and its SBOMs, builds the package from it twice on
separate runners and compares them, and publishes everything with checksums, signatures and
build provenance. `tools/dist.sh` makes the same files locally, and `tools/update-pins.py`
renews the container images the workflows pin, which Dependabot does not see.

The Windows packages come from `tools/windows-package.ps1` on a Windows runner, signed with
the certificate in the secrets `WINDOWS_CERTIFICATE` (the PKCS #12 file in Base64) and
`WINDOWS_CERTIFICATE_PASSWORD`. Without them, the release fails. `tools/windows-certificate.sh`
makes a new certificate and, with `--upload`, stores it there; its subject becomes the
publisher of the MSIX package, so a new subject makes a package that Windows treats as
another program.
