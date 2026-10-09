import { folderFiles } from './folder-files';

function selected(path: string, contents = '') {
  const file = new File([contents], path.split('/').pop()!);
  Object.defineProperty(file, 'webkitRelativePath', { value: `Project/${path}` });
  Object.defineProperty(file, 'text', { configurable: true, value: async () => contents });
  return file;
}
const limits = { max_bytes: 104857600, max_files: 10000 };

it('honors nested negations without rescuing files inside an excluded parent', async () => {
  const files = [
    selected('.gitignore', 'build/\n*.log\n'), selected('build/.gitignore', '!keep.txt'),
    selected('build/keep.txt'), selected('src/.gitignore', '!keep.log\nnested/\n'),
    selected('src/keep.log'), selected('src/drop.log'), selected('src/nested/.gitignore', '!keep.txt'),
    selected('src/nested/keep.txt'), selected('.git/config'), selected('README.md'),
  ];
  const result = await folderFiles(files, limits);
  expect(result.map(entry => entry.path)).toEqual(['.gitignore', 'src/.gitignore', 'src/keep.log', 'README.md']);
});

it('checks contents and file counts after filtering, without reading normal file contents', async () => {
  const file = selected('data.bin', '12345');
  const read = vi.spyOn(file, 'text');
  await expect(folderFiles([file], { ...limits, max_bytes: 4 })).rejects.toThrow('import limit');
  await expect(folderFiles([file, selected('second')], { ...limits, max_files: 1 })).rejects.toThrow('file import limit');
  expect(read).not.toHaveBeenCalled();
  expect(await folderFiles([file], { ...limits, max_bytes: 5 })).toHaveLength(1);
});
