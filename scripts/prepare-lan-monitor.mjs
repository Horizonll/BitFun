import { cp, mkdir, readdir, rm, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const sourceDirectory = path.join(repositoryRoot, 'dist');
const targetDirectory = path.join(
  repositoryRoot,
  'src',
  'apps',
  'desktop',
  'resources',
  'lan-monitor',
);

await stat(path.join(sourceDirectory, 'lan-monitor.html'));
await mkdir(targetDirectory, { recursive: true });
for (const entry of await readdir(targetDirectory)) {
  if (entry !== '.gitignore') {
    await rm(path.join(targetDirectory, entry), { recursive: true, force: true });
  }
}

for (const entry of ['lan-monitor.html', 'assets', 'fonts', 'Logo-ICON-128.png']) {
  const source = path.join(sourceDirectory, entry);
  try {
    await stat(source);
    await cp(source, path.join(targetDirectory, entry), { recursive: true });
  } catch (error) {
    if (entry === 'lan-monitor.html' || entry === 'assets') {
      throw error;
    }
  }
}
