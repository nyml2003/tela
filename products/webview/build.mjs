// esbuild 构建：web/ 源码与静态页面模板 → 可删除的 dist/ 发布目录。
// 只清理自己的 bundle 子目录，不能删除由 Rust 构建写入的 wasm 工件。
import esbuild from 'esbuild';
import { cp, mkdir, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const distDir = fileURLToPath(new URL('../../dist', import.meta.url));
const publicDir = fileURLToPath(new URL('./public', import.meta.url));
const assetDir = join(distDir, 'assets/tela-web');
const entries = { app: 'src/main.ts', agent: 'src/agent.ts', card: 'src/card.ts', rawgpu: 'src/rawgpu.ts' };

await mkdir(distDir, { recursive: true });
await rm(assetDir, { recursive: true, force: true });
await Promise.all([
  cp(join(publicDir, 'index.html'), join(distDir, 'index.html')),
  cp(join(publicDir, 'agent.html'), join(distDir, 'agent.html')),
  cp(join(publicDir, 'agent-bootstrap.js'), join(distDir, 'agent-bootstrap.js')),
  cp(join(publicDir, 'card.html'), join(distDir, 'card.html')),
  cp(join(publicDir, 'card-bootstrap.js'), join(distDir, 'card-bootstrap.js')),
  cp(join(publicDir, 'rawgpu.html'), join(distDir, 'rawgpu.html')),
]);

await esbuild.build({
  entryPoints: entries,
  bundle: true,
  format: 'esm',
  outdir: assetDir,
  entryNames: '[name]',
  plugins: [],
  sourcemap: true,
  target: 'es2022',
  logLevel: 'info',
});

console.log('tela-web 构建完成 → dist/assets/tela-web/');
