// 基础设施层：遥测存储（环形缓冲 + SSE 客户端管理）。
import type { ServerResponse } from 'node:http';
import type { TelemetryPort } from '../domain/ports.ts';
import type { CliCommand, TelemetryEvent } from '../domain/telemetry.ts';
import { TELEMETRY_CAPACITY } from '../domain/telemetry.ts';

interface SseClient {
  res: ServerResponse;
}

/** SSE 心跳间隔（EventSource 断线自动重连，注释行防代理超时）。 */
const HEARTBEAT_MS = 15_000;

export class TelemetryStore implements TelemetryPort {
  private readonly events: TelemetryEvent[] = [];
  private readonly clients = new Set<SseClient>();
  private readonly heartbeat: ReturnType<typeof setInterval>;

  constructor() {
    this.heartbeat = setInterval(() => {
      for (const c of this.clients) {
        try {
          c.res.write(': hb\n\n');
        } catch {
          this.clients.delete(c);
        }
      }
    }, HEARTBEAT_MS);
    // 心跳定时器不阻止进程退出（CLI Ctrl+C 后服务关闭）。
    this.heartbeat.unref?.();
  }

  push(events: TelemetryEvent[]): boolean {
    for (const e of events) {
      this.events.push(e);
      if (this.events.length > TELEMETRY_CAPACITY) {
        this.events.shift();
      }
    }
    return this.clients.size > 0;
  }

  broadcast(command: CliCommand): void {
    const payload = `data: ${JSON.stringify(command)}\n\n`;
    for (const c of this.clients) {
      try {
        c.res.write(payload);
      } catch {
        this.clients.delete(c);
      }
    }
  }

  connectionCount(): number {
    return this.clients.size;
  }

  snapshot(): readonly TelemetryEvent[] {
    return this.events;
  }

  /** SSE 连接注册（server.ts /api/events 处理器调用）。 */
  registerSse(res: ServerResponse): void {
    res.writeHead(200, {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      Connection: 'keep-alive',
    });
    res.write(': connected\n\n');
    const client: SseClient = { res };
    this.clients.add(client);
    res.on('close', () => this.clients.delete(client));
  }

  /** 关闭：清空连接（服务关闭时调用）。 */
  dispose(): void {
    clearInterval(this.heartbeat);
    for (const c of this.clients) {
      try {
        c.res.end();
      } catch {
        // 已关闭
      }
    }
    this.clients.clear();
  }
}
