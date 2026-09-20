import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`Invalid semver version: ${version}`);
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

// Replace only the version value so formatting and comments stay untouched.
function replaceVersion(relativePath, pattern) {
  const path = resolve(root, relativePath);
  const content = readFileSync(path, 'utf8');
  if (!pattern.test(content)) {
    throw new Error(`Could not find a version field in ${relativePath}.`);
  }
  writeFileSync(path, content.replace(pattern, `$1${version}$2`));
}

replaceVersion('package.json', /("version"\s*:\s*")[^"]*(")/);
replaceVersion('src-tauri/tauri.conf.json', /("version"\s*:\s*")[^"]*(")/);
replaceVersion('src-tauri/Cargo.toml', /(^version\s*=\s*")[^"]*(")/m);

console.log(`Set application version to ${version}.`);
