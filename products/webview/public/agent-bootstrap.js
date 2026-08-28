import init, * as agentWasm from '/tela_agent_demo.js';
import { showAgentStartupError, startAgentDemo } from './assets/tela-web/agent.js';

try {
  await init({ module_or_path: '/tela_agent_demo_bg.wasm' });
  await startAgentDemo(agentWasm);
} catch (error) {
  showAgentStartupError(error);
}
