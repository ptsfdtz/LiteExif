import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const packageJson = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8'));
const tauriConfig = JSON.parse(readFileSync(resolve(root, 'src-tauri/tauri.conf.json'), 'utf8'));
const cargo = spawnSync(
  'cargo',
  ['metadata', '--no-deps', '--format-version', '1', '--manifest-path', 'src-tauri/Cargo.toml'],
  { cwd: root, encoding: 'utf8' },
);

if (cargo.status !== 0) {
  process.stderr.write(cargo.stderr);
  process.exit(cargo.status ?? 1);
}

const cargoMetadata = JSON.parse(cargo.stdout);
const rustPackage = cargoMetadata.packages.find((candidate) => candidate.name === 'liteexif');
if (!rustPackage) throw new Error('Cargo metadata does not contain the liteexif package.');

const versions = {
  package: packageJson.version,
  tauri: tauriConfig.version,
  cargo: rustPackage.version,
};
const uniqueVersions = new Set(Object.values(versions));
if (uniqueVersions.size !== 1) {
  throw new Error(`Version mismatch: ${JSON.stringify(versions)}`);
}

const version = packageJson.version;
// The release workflow syncs the version from the tag before verifying, so the
// tag-match check is opt-in and never blocks the regular CI run.
if (
  process.env.CHECK_RELEASE_TAG === '1' &&
  process.env.GITHUB_REF_TYPE === 'tag' &&
  process.env.GITHUB_REF_NAME !== `v${version}`
) {
  throw new Error(`Release tag ${process.env.GITHUB_REF_NAME} does not match v${version}.`);
}

console.log(`Version ${version} is consistent.`);
