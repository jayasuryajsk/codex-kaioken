#!/usr/bin/env node
import { chmodSync, createWriteStream, existsSync, renameSync, rmSync } from 'node:fs';
import { pipeline } from 'node:stream/promises';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import fetch from 'node-fetch';
import tar from 'tar';

const version = process.env.CODEX_KAIOKEN_VERSION || '0.1.5';
const platform = process.platform;
const arch = process.arch;

const mapping = {
  'darwin-x64': 'codex-kaioken-x86_64-apple-darwin.tar.gz',
  'darwin-arm64': 'codex-kaioken-aarch64-apple-darwin.tar.gz',
  'linux-x64': 'codex-kaioken-x86_64-unknown-linux-musl.tar.gz',
  'linux-arm64': 'codex-kaioken-aarch64-unknown-linux-musl.tar.gz',
};

const key = `${platform}-${arch}`;
if (!mapping[key]) {
  console.error(`Unsupported platform: ${key}`);
  process.exit(1);
}

const asset = mapping[key];
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

  const extractedPath = join(here, 'codex-kaioken');
  rmSync(extractedPath, { force: true });

  console.log('Extracting...');
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

  chmodSync(extractedPath, 0o755);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
