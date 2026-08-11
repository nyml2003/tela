// Node 26 的 @types/node 不再提供全局 WebAssembly 类型，声明本项目用到的最小子集。
// 只声明 `instantiate`（bytes + importObject 形式）与 Instance 导出表。

declare namespace WebAssembly {
  interface Instance {
    readonly exports: Record<string, unknown>;
  }

  interface InstantiatedSource {
    instance: Instance;
  }

  function instantiate(
    bytes: ArrayBuffer | Uint8Array,
    importObject?: Record<string, unknown>,
  ): Promise<InstantiatedSource>;
}
