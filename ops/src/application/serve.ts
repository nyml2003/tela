// 应用层：serve 用例——开发静态服务器（替代 scripts/serve-demo.mjs 手工命令）。
import type { Reporter, ServerPort, ServeResult } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';

export interface ServeDeps {
  server: ServerPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

/** 常驻服务：端口被占用时自动递增（EADDRINUSE 重试，见 HttpServerPort）。 */
export async function runServe(deps: ServeDeps, preferredPort: number): Promise<ServeResult> {
  const { server, reporter, workspace } = deps;
  reporter.section('开发服务器（serve）');
  const result = await server.serve(workspace.demoDir, preferredPort, (msg) => reporter.info(msg));
  reporter.ok(`监听 0.0.0.0:${result.port}`);
  reporter.info('可用页面（URL 单独一行，点击直达）：');
  reporter.info(`http://127.0.0.1:${result.port}/              （蓝色矩形；?backend=raster|wgpu|auto）`);
  reporter.info(`http://127.0.0.1:${result.port}/mvp.html      （独立 wgpu MVP 对照）`);
  reporter.info(`http://127.0.0.1:${result.port}/rawgpu.html   （GPU 自检：ops verify gpu 自动打开）`);
  reporter.info('Ctrl+C 停止');
  return result;
}
