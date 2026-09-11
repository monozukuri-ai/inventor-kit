import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { resolve, relative } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const target = resolve(root, '../python/inventor_kit/viewer/static');
const hash = p => createHash('sha256').update(readFileSync(p)).digest('hex');
const walk = p => readdirSync(p, { withFileTypes: true }).flatMap(e => e.isDirectory() ? walk(resolve(p, e.name)) : [resolve(p, e.name)]);
const packages = ['three-cad-viewer', 'three-cad-viewer/node_modules/three', 'three', 'n8ao', 'postprocessing'];
const licenses = packages.map(p => {
  const directory = resolve(root, 'node_modules', p);
  const pkg = JSON.parse(readFileSync(resolve(directory, 'package.json')));
  const license = readdirSync(directory).find(n => /^LICENSE(\.md)?$/.test(n));
  if (!license) throw new Error(`Missing license for ${p}`);
  return `${pkg.name} ${pkg.version}\n${readFileSync(resolve(directory, license), 'utf8').replace(/[ \t]+$/gm, '')}`;
}).join('\n\n--------\n\n');
writeFileSync(resolve(target, 'THIRD_PARTY_LICENSES.txt'), licenses);
const inputs = [...walk(resolve(root, 'src')), ...walk(resolve(root, 'scripts')),
  ...['package.json', 'package-lock.json', 'index.html', 'tsconfig.json', 'vite.config.ts'].map(p => resolve(root, p))];
const outputs = walk(target).filter(p => !p.endsWith('/manifest.json'));
const entries = (paths, base) => Object.fromEntries(paths.sort().map(p => [relative(base, p).replaceAll('\\', '/'), hash(p)]));
writeFileSync(resolve(target, 'manifest.json'), JSON.stringify({ schema_version: 1,
  command: 'npm ci --prefix viewer && npm run build --prefix viewer',
  build_environment: { node: process.version, npm: process.env.npm_config_user_agent?.match(/npm\/([^ ]+)/)?.[1] ?? 'unknown' },
  inputs: entries(inputs, root), outputs: entries(outputs, target) }, null, 2) + '\n');
