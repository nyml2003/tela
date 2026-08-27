//! 可选 JSONL 持久化：每事件一行，启动重放重建事件日志，超限轮转保留一份 `.1`。
//!
//! 无数据库依赖；`fsync` 不做（服务器重启丢尾部几行是 v1 可接受的损耗，`seq` 语义靠
//! "从重放最大值续增" 保持不回退）。

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tela_cc_protocol::Event;

/// 当前日志超过该字节数时轮转到 `.1`（只保留一代）。
pub const ROTATE_BYTES: u64 = 16 * 1024 * 1024;

/// JSONL 持久化句柄；`Mutex` 串行化追加与轮转。
pub struct PersistHandle {
    inner: Mutex<PersistInner>,
    dir: PathBuf,
}

struct PersistInner {
    writer: BufWriter<File>,
    written: u64,
}

impl PersistHandle {
    /// 打开（或创建）`dir/events.jsonl`，返回句柄与重放出的历史事件。
    ///
    /// 重放顺序：先 `.1`（旧代）再当前文件。当前文件超 [`ROTATE_BYTES`] 时在打开前
    /// 先轮转，避免句柄持有期间改名。
    pub fn open(dir: &Path) -> std::io::Result<(Self, Vec<Event>)> {
        fs::create_dir_all(dir)?;
        let current = dir.join("events.jsonl");
        if fs::metadata(&current).map(|meta| meta.len()).unwrap_or(0) > ROTATE_BYTES {
            rotate(&current)?;
        }
        let replayed = replay_dir(dir)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&current)?;
        let written = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        Ok((
            Self {
                inner: Mutex::new(PersistInner {
                    writer: BufWriter::new(file),
                    written,
                }),
                dir: dir.to_owned(),
            },
            replayed,
        ))
    }

    /// 追加一条事件；轮转失败只影响持久性，不影响内存日志（调用方已入日志）。
    pub fn append(&self, event: &Event) {
        let mut inner = self.inner.lock().expect("persist poisoned");
        let line = match serde_json::to_string(event) {
            Ok(line) => line,
            Err(error) => {
                eprintln!("tela-cc-relay: serialize event for persistence: {error}");
                return;
            }
        };
        let mut failed = false;
        {
            let writer = &mut inner.writer;
            if writeln!(writer, "{line}").is_ok() && writer.flush().is_ok() {
                inner.written += line.len() as u64 + 1;
            } else {
                failed = true;
            }
        }
        if failed {
            eprintln!("tela-cc-relay: append event to persistence failed");
            return;
        }
        if inner.written > ROTATE_BYTES {
            let current = self.dir.join("events.jsonl");
            if let Err(error) = rotate(&current) {
                eprintln!("tela-cc-relay: rotate persistence: {error}");
                return;
            }
            match OpenOptions::new().create(true).append(true).open(&current) {
                Ok(file) => {
                    inner.writer = BufWriter::new(file);
                    inner.written = 0;
                }
                Err(error) => {
                    eprintln!("tela-cc-relay: reopen persistence after rotate: {error}");
                }
            }
        }
    }
}

/// 把 `events.jsonl` 改名为 `events.jsonl.1`（覆盖旧代）。
fn rotate(current: &Path) -> std::io::Result<()> {
    let previous = rotation_path(current);
    if current.exists() {
        fs::rename(current, previous)?;
    }
    Ok(())
}

fn rotation_path(current: &Path) -> PathBuf {
    let mut name = current.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

/// 按旧代 → 当前的顺序重放全部事件；损坏行跳过并告警。
fn replay_dir(dir: &Path) -> std::io::Result<Vec<Event>> {
    let current = dir.join("events.jsonl");
    let rotation = rotation_path(&current);
    let mut events = Vec::new();
    if rotation.exists() {
        replay_file(&rotation, &mut events);
    }
    if current.exists() {
        replay_file(&current, &mut events);
    }
    Ok(events)
}

fn replay_file(path: &Path, events: &mut Vec<Event>) {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("tela-cc-relay: open {} for replay: {error}", path.display());
            return;
        }
    };
    for line in BufReader::new(file).lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("tela-cc-relay: read {}: {error}", path.display());
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Event>(&line) {
            Ok(event) => events.push(event),
            Err(error) => {
                eprintln!(
                    "tela-cc-relay: skip corrupted line in {}: {error}",
                    path.display()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tela_cc_protocol::{EventKind, NoticeLevel};

    fn notice(text: &str, seq: u64) -> Event {
        Event {
            seq,
            ts_ms: seq,
            kind: EventKind::Notice {
                level: NoticeLevel::Info,
                text: text.to_owned(),
            },
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tela-cc-relay-test-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn events_survive_reopen_and_seq_source_is_the_max() {
        let dir = temp_dir("reopen");
        let (handle, replayed) = PersistHandle::open(&dir).expect("open");
        assert!(replayed.is_empty());
        handle.append(&notice("a", 1));
        handle.append(&notice("b", 5));
        drop(handle);

        let (_handle, replayed) = PersistHandle::open(&dir).expect("reopen");
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[1].seq, 5);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotation_keeps_one_generation_and_replays_both() {
        let dir = temp_dir("rotate");
        let (handle, _) = PersistHandle::open(&dir).expect("open");
        let big = "x".repeat(64 * 1024);
        // 每行 ~64KiB；写满 257 行必然超过 16MiB 触发一次轮转。
        for seq in 1..=257 {
            handle.append(&notice(&big, seq));
        }
        drop(handle);

        let (_handle, replayed) = PersistHandle::open(&dir).expect("reopen");
        assert_eq!(replayed.len(), 257, "rotation must not lose events");
        assert!(dir.join("events.jsonl.1").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
