import ignore from 'ignore';
import { ImportLimits } from '../users-api';

/** Filter browser-selected files without reading project contents into memory. */
export async function folderFiles(files: File[], limits: ImportLimits) {
  const entries = files.map(file => ({ file, path: file.webkitRelativePath.split('/').slice(1).join('/') }))
    .filter(entry => !entry.path.split('/').some(part => part.toLowerCase() === '.git'));
  const rules = new Map<string, ReturnType<typeof ignore>>();
  // Only ignore files need their content read; the browser streams other files.
  for (const entry of entries) {
    if (entry.path.split('/').pop() !== '.gitignore') continue;
    if (entry.file.size > 1024 * 1024) throw new Error('A .gitignore file is too large to process.');
    const directory = entry.path.includes('/') ? entry.path.slice(0, entry.path.lastIndexOf('/') + 1) : '';
    rules.set(directory, ignore({ ignorecase: false }).add(await entry.file.text()));
  }
  const included = entries.filter(entry => {
    const parts = entry.path.split('/');
    if (!entry.path || parts.some(part => !part || part === '.' || part === '..')) throw new Error('The selected folder contains an invalid file path.');
    const excluded = (path: string) => {
      let ignored = false;
      const segments = path.replace(/\/$/, '').split('/');
      for (let depth = 0; depth < segments.length; depth++) {
        const directory = segments.slice(0, depth).join('/') + (depth ? '/' : '');
        const match = rules.get(directory)?.test(path.slice(directory.length));
        if (!match) continue;
        if (match.ignored) ignored = true;
        else if (match.unignored) ignored = false;
      }
      return ignored;
    };
    // Check parents first: rules inside an excluded directory cannot rescue it.
    for (let depth = 1; depth < parts.length; depth++) {
      if (excluded(parts.slice(0, depth).join('/') + '/')) return false;
    }
    return !excluded(entry.path);
  });
  const bytes = included.reduce((total, entry) => total + entry.file.size, 0);
  if (bytes > limits.max_bytes) throw new Error(`Project exceeds the ${Math.floor(limits.max_bytes / (1024 * 1024))} MiB import limit.`);
  if (included.length > limits.max_files) throw new Error(`Project exceeds the ${limits.max_files} file import limit.`);
  return included;
}
