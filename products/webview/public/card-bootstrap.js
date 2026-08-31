import init, * as probeWasm from '/tela_webview_probe.js';
import { showCardStartupError, startCardProbe } from './assets/tela-web/card.js';

try {
  await init({ module_or_path: '/tela_webview_probe_bg.wasm' });
  await startCardProbe(probeWasm);
} catch (error) {
  showCardStartupError(error);
}
