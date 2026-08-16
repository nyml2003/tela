// 应用层：serve 用例——开发静态服务器（替代 scripts/serve-demo.mjs 手工命令）。
import type { FsPort, Reporter, ServerPort, ServeResult } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';

export interface ServeDeps {
  fs: FsPort;
  server: ServerPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

/** 常驻服务：端口被占用时自动递增（EADDRINUSE 重试，见 HttpServerPort）。 */
export async function runServe(deps: ServeDeps, preferredPort: number): Promise<ServeResult | undefined> {
  const { fs, server, reporter, workspace } = deps;
  reporter.section('开发服务器（serve）');
  const hasBrowserPage = await fs.exists(`${workspace.distDir}/index.html`);
  const desktopBundle = workspace.bundle('desktop');
  const mobileBundle = workspace.bundle('mobile');
  const hasDesktopSdkBundle = await fs.exists(desktopBundle.indexPath());
  const hasMobileSdkBundle = await fs.exists(mobileBundle.indexPath());
  const hasWebviewShell = (await fs.exists(workspace.webviewSdkGluePath()))
    && (await fs.exists(workspace.webviewSdkWasmPath()));
  if (!hasBrowserPage && !hasDesktopSdkBundle && !hasMobileSdkBundle) {
    reporter.fail(`缺少 ${workspace.distDir}/index.html、${desktopBundle.indexPath()} 和 ${mobileBundle.indexPath()}`);
    reporter.info('请先运行: ops build（浏览器）或 ops build bundle（原生 SDK）');
    return undefined;
  }
  const result = await server.serve(workspace.distDir, preferredPort, (msg) => reporter.info(msg));
  reporter.ok(`监听 0.0.0.0:${result.port}`);
  if (hasBrowserPage && hasDesktopSdkBundle && hasWebviewShell) {
    reporter.info('可用页面（URL 单独一行，点击直达）：');
    reporter.info(`http://127.0.0.1:${result.port}/              （统一 bundle + WGPU WebView SDK）`);
    reporter.info(`http://127.0.0.1:${result.port}/rawgpu.html   （原生 WebGPU 自检页）`);
  } else if (hasBrowserPage) {
    reporter.warn('浏览器页面构建不完整：还需要 WebView shell 与 tela-dev bundle；运行 ops build。');
  }
  if (hasDesktopSdkBundle) {
    reporter.info(`桌面 SDK bundle（本机）: http://127.0.0.1:${result.port}/tela-dev/latest.json`);
    reporter.info('跨机器请以开发机可达的局域网 IP 替换 127.0.0.1，并传给 SDK 的 --bundle-index。');
  }
  if (hasMobileSdkBundle) {
    reporter.info(`Android mobile bundle（本机）: http://127.0.0.1:${result.port}/tela-mobile/latest.json`);
    reporter.info('Android USB 真机请改用 ops android serve；它固定绑定 127.0.0.1:8000 并配合 adb reverse。');
  }
  reporter.info('Ctrl+C 停止');
  return result;
}
