// 领域层：端口（依赖倒置的边界）。domain 只定义接口，实现都在 infrastructure。
// 应用层用例只依赖这里的端口，不感知 Node/cargo 等具体实现。

import type { CliCommand, TelemetryEvent } from './telemetry.ts';

export interface ProcessResult {
  code: number;
  stdout: string;
  stderr: string;
}

export interface ProcessOptions {
  cwd?: string;
}

/** 进程执行端口：跑 cargo / node 等外部命令。 */
export interface ProcessPort {
  run(cmd: string, args: string[], opts?: ProcessOptions): Promise<ProcessResult>;
}

/** 文件系统端口。 */
export interface FsPort {
  exists(path: string): Promise<boolean>;
  copyFile(from: string, to: string): Promise<void>;
  /** 文件字节数，不存在返回 null。 */
  statSize(path: string): Promise<number | null>;
  /** 更新文件 mtime（touch；构建时间戳刷新用，见 build-demo.ts）。 */
  touch(path: string): Promise<void>;
}

/** 静态服务器端口（serve 命令）。 */
export interface ServerPort {
  /**
   * 以 root 为根启动静态服务器；首选 preferredPort，被占用时自动递增找下一个
   * 可用端口（EADDRINUSE 重试，最多 MAX_PORT_ATTEMPTS 次）。
   * `telemetry` 提供时挂载 WebGPU 环境验证所需的上报端点。
   * 返回关闭函数与实际监听端口；log 回调收到请求日志（接入报告器）。
   */
  serve(
    root: string,
    preferredPort: number,
    log: (msg: string) => void,
    telemetry?: import('../infrastructure/telemetry-store.ts').TelemetryStore,
  ): Promise<ServeResult>;
}

export interface ServeResult {
  close: () => Promise<void>;
  port: number;
}

/** 遥测事件接收端口：页面上报（POST /api/telemetry）与 SSE 命令下发（/api/events）。 */
export interface TelemetryPort {
  /**
   * 接收页面上报事件（由服务端 /api/telemetry 处理器调用）。
   * 返回是否被消费（false = 无订阅者，静默丢弃）。
   */
  push(events: TelemetryEvent[]): boolean;
  /**
   * 向所有已连接的调试页下发命令（SSE /api/events 写入）。
   * 无连接时静默（页面下次连接后由 CLI 补发 ping 探测）。
   */
  broadcast(command: CliCommand): void;
  /** 当前已连接调试页数。 */
  connectionCount(): number;
  /** 快照最近事件（汇总用）。 */
  snapshot(): readonly TelemetryEvent[];
}


/** 终端报告端口：所有用户可见输出都经它（便于测试与未来换 JSON 输出）。 */
export interface Reporter {
  section(title: string): void;
  ok(msg: string): void;
  fail(msg: string): void;
  info(msg: string): void;
  warn(msg: string): void;
  table(headers: readonly string[], rows: readonly (readonly string[])[]): void;
}
