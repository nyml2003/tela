// 文件管理器浏览器入口。页面只选择 DOM canvas 和 bundle 索引；启动、输入、guest 与 WGPU
// 生命周期由 tela-webview-sdk 统一负责，避免浏览器页再次成为第二个应用宿主。

import { startTelaWebview } from './webview-sdk/index';

declare global {
  interface Window {
    /** 开发控制台兼容入口：原子替换当前 guest 的完整键位表快照。 */
    telaReplaceKeymap?: (snapshot: string | object) => boolean;
  }
}

async function start(): Promise<void> {
  const canvas = document.querySelector<HTMLCanvasElement>('canvas');
  if (!canvas) throw new Error('缺少文件管理器 canvas');
  const session = await startTelaWebview({
    canvas,
    bundleIndex: new URL('/tela-dev/latest.json', window.location.href),
  });
  window.telaReplaceKeymap = (snapshot) => session.replaceKeymap(snapshot);
  window.addEventListener('pagehide', () => {
    session.close();
    delete window.telaReplaceKeymap;
  }, { once: true });
}

function showStartupError(error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  console.error(error);
  document.body.dataset.error = message;
  const notice = document.querySelector<HTMLElement>('#startup-error');
  if (notice) {
    notice.hidden = false;
    notice.textContent = `启动失败：${message}`;
  }
}

void start().catch(showStartupError);
