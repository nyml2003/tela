// esbuild 构建：浏览器演示入口 → demo/assets/tela-web/。
// 每次先清理受控输出目录，避免静态服务意外保留已删除页面的旧 bundle。
import esbuild from 'esbuild';
import { rm } from 'node:fs/promises';

const entries = { app: 'src/main.ts' };

await rm('../demo/assets/tela-web', { recursive: true, force: true });

await esbuild.build({
  entryPoints: entries,
  bundle: true,
  format: 'esm',
  outdir: '../demo/assets/tela-web',
  entryNames: '[name]',
  plugins: [],
  sourcemap: true,
  target: 'es2022',
  logLevel: 'info',
});

console.log('tela-web 构建完成 → demo/assets/tela-web/');
