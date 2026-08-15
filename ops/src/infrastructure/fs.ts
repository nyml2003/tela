// 基础设施层：文件系统适配器（Node 内置 fs/promises）。
import { copyFile as cp, mkdir, rename, rm, stat, utimes } from 'node:fs/promises';
import type { FsPort } from '../domain/ports.ts';

export class NodeFsPort implements FsPort {
  async exists(path: string): Promise<boolean> {
    return (await this.statSize(path)) !== null;
  }

  async ensureDir(path: string): Promise<void> {
    await mkdir(path, { recursive: true });
  }

  async resetDir(path: string): Promise<void> {
    await rm(path, { recursive: true, force: true });
    await mkdir(path, { recursive: true });
  }

  async copyFile(from: string, to: string): Promise<void> {
    await cp(from, to);
  }

  async rename(from: string, to: string): Promise<void> {
    await rename(from, to);
  }

  async statSize(path: string): Promise<number | null> {
    try {
      const s = await stat(path);
      return s.size;
    } catch {
      return null;
    }
  }

  async touch(path: string): Promise<void> {
    const now = new Date();
    await utimes(path, now, now);
  }
}
