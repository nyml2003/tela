// 应用层：Android 真机开发服务。ADB reverse 只允许固定回环端口，不能继承通用 serve 的 LAN/递增语义。

import { ANDROID_BUNDLE_INDEX_URL, ANDROID_DEV_PORT } from '../domain/android.ts';
import type { FsPort, Reporter, ServerPort, ServeResult } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';

export interface AndroidServeDeps {
  fs: FsPort;
  server: ServerPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export async function runAndroidServe(deps: AndroidServeDeps): Promise<ServeResult | undefined> {
  const { fs, server, reporter, workspace } = deps;
  const mobile = workspace.bundle('mobile');
  reporter.section('Android 开发服务器（ADB reverse）');

  const [hasIndex, hasArchive] = await Promise.all([
    fs.exists(mobile.indexPath()),
    fs.exists(mobile.archivePath()),
  ]);
  if (!hasIndex || !hasArchive) {
    reporter.fail(`缺少 Android mobile bundle：${mobile.indexPath()} 或 ${mobile.archivePath()}`);
    reporter.info('请先运行: nix develop .#android --command ops build android');
    return undefined;
  }

  try {
    const result = await server.serveExact(
      workspace.distDir,
      '127.0.0.1',
      ANDROID_DEV_PORT,
      (message) => reporter.info(message),
    );
    reporter.ok(`仅监听 127.0.0.1:${ANDROID_DEV_PORT}`);
    reporter.info(`真机通过 adb reverse 请求: ${ANDROID_BUNDLE_INDEX_URL}`);
    reporter.info('另开终端运行 ops android deploy [--serial <serial>]；Ctrl+C 停止服务。');
    return result;
  } catch (error) {
    reporter.fail(`Android 开发服务器无法绑定 127.0.0.1:${ANDROID_DEV_PORT}`);
    reporter.info(String(error));
    return undefined;
  }
}
