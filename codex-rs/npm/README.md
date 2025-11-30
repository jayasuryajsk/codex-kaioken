# codex-kaioken (npm wrapper)

This package installs the Codex Kaioken CLI by downloading the appropriate binary from GitHub Releases.

## Installation

```bash
npm install -g codex-kaioken
```

During `postinstall` the package fetches the `codex-kaioken` binary for your platform, marks it executable, and exposes it on your `PATH`.

## Environment variables

- `CODEX_KAIOKEN_VERSION`: override the binary version (defaults to the package version).
- `CODEX_KAIOKEN_LOCAL_TARBALL`: path to a local tarball (useful when testing without a published GitHub Release).

## License

Apache-2.0
