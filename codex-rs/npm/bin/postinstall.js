#!/usr/bin/env node
import { chmodSync, createReadStream, createWriteStream, existsSync, renameSync, rmSync } from 'node:fs';
import { pipeline } from 'node:stream/promises';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import fetch from 'node-fetch';
import tar from 'tar';
import unzipper from 'unzipper';

const version = process.env.CODEX_KAIOKEN_VERSION || '0.1.7';
const platform = process.platform;
const arch = process.arch;

const mapping = {
  'darwin-x64': 'codex-kaioken-x86_64-apple-darwin.tar.gz',
  'darwin-arm64': 'codex-kaioken-aarch64-apple-darwin.tar.gz',
  'linux-x64': 'codex-kaioken-x86_64-unknown-linux-musl.tar.gz',
  'linux-arm64': 'codex-kaioken-aarch64-unknown-linux-musl.tar.gz',
  'win32-x64': 'codex-kaioken-x86_64-pc-windows-msvc.zip',
  'win32-arm64': 'codex-kaioken-aarch64-pc-windows-msvc.zip',
};

const key = `${platform}-${arch}`;
if (!mapping[key]) {
  console.error(`Unsupported platform: ${key}`);
  process.exit(1);
}

const asset = mapping[key];
const isZip = asset.endsWith('.zip');
const url = `https://github.com/jayasuryajsk/codex-kaioken/releases/download/v${version}/${asset}`;
const dest = join(tmpdir(), asset);
const localTarball = process.env.CODEX_KAIOKEN_LOCAL_TARBALL;
const here = dirname(fileURLToPath(import.meta.url));

async function main() {
  let archivePath = localTarball;
  if (!archivePath) {
    console.log(`Fetching ${asset}...`);
    const res = await fetch(url);
    if (!res.ok) {
      console.error(`Failed to download ${url}: ${res.status} ${res.statusText}`);
      process.exit(1);
    }
    await pipeline(res.body, createWriteStream(dest));
    archivePath = dest;
  } else {
    console.log(`Using local tarball ${archivePath}`);
  }

  // Determine expected binary name after extraction
  const ext = platform === 'win32' ? '.exe' : '';
  const finalBinaryName = `codex-kaioken${ext}`;
  const extractedPath = join(here, finalBinaryName);
  rmSync(extractedPath, { force: true });

  console.log('Extracting...');

  if (isZip) {
    // Extract zip archive (Windows)
    // The zip contains codex-kaioken.exe directly
    await pipeline(
      createReadStream(archivePath),
      unzipper.Extract({ path: here })
    );

    // Verify the binary was extracted
    if (!existsSync(extractedPath)) {
      console.error(`Extracted binary missing: ${extractedPath}`);
      process.exit(1);
    }
  } else {
    // Extract tar.gz archive (macOS/Linux)
    await tar.x({ file: archivePath, cwd: here });
    const binaryName = asset.replace('.tar.gz', '');
    const platformPath = join(here, binaryName);

    if (!existsSync(platformPath) && !existsSync(extractedPath)) {
      console.error(`Extracted binary missing: tried ${platformPath} and ${extractedPath}`);
      process.exit(1);
    }

    if (existsSync(platformPath) && platformPath !== extractedPath) {
      rmSync(extractedPath, { force: true });
      renameSync(platformPath, extractedPath);
    }
  }

  // Make executable (no-op on Windows, but harmless)
  chmodSync(extractedPath, 0o755);
  console.log(`Installed ${finalBinaryName}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
