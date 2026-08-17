// 开发 bundle 的浏览器网络入口。持久化缓存故意不在这里出现：开发页启动一次就读取
// `latest.json` 和其声明的一个 `.tela` archive，验证交给 Rust SDK。

import type { TelaWebviewBindings } from './bindings';

export interface LoadedDevelopmentBundle {
  readonly indexUrl: URL;
  readonly archiveUrl: URL;
  readonly bundleId: string;
  readonly guestWasm: Uint8Array;
}

/** Fetches, validates and exposes one current development guest module. */
export async function loadDevelopmentBundle(
  bindings: TelaWebviewBindings,
  bundleIndex: string | URL,
): Promise<LoadedDevelopmentBundle> {
  const indexUrl = new URL(bundleIndex, window.location.href);
  const indexResponse = await fetch(indexUrl, { cache: 'no-store' });
  if (!indexResponse.ok) {
    throw new Error(`请求开发 bundle 索引失败: ${indexResponse.status} ${indexResponse.statusText}`);
  }
  const index = bindings.parse_development_index(
    new Uint8Array(await indexResponse.arrayBuffer()),
  );
  const archiveUrl = new URL(index.bundle_url, indexUrl);
  const archiveResponse = await fetch(archiveUrl, { cache: 'no-store' });
  if (!archiveResponse.ok) {
    throw new Error(`请求开发 bundle 失败: ${archiveResponse.status} ${archiveResponse.statusText}`);
  }
  const bundle = bindings.validate_development_bundle(
    index,
    new Uint8Array(await archiveResponse.arrayBuffer()),
  );
  return {
    indexUrl,
    archiveUrl,
    bundleId: bundle.bundle_id,
    guestWasm: bundle.app_wasm(),
  };
}
