# device/006-getTimeStamp

> **状态：📐 设计已确认，MVP 定稿。** `std.device.getTimeStamp`：墙钟时间与时区信息。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.device.getTimeStamp` 提供**墙钟时间**（wall clock）与当前时区信息。它回答"宿主认为现在是什么时刻、在哪个时区"，宿主时钟为权威；guest 用它做时间展示、过期判断与日期计算，不自行读取系统时钟（WASM guest 无可靠墙钟）。

**与单调时钟的区分**：输入事件已携带单调时钟 `timestamp_micros`（相对时间，用于手势/动画），`std.device.getTimeStamp` 是绝对时间（epoch 毫秒 + 时区）。两者语义不同，不得混用。

## 2. 请求

```text
BridgeRequest::Now { version: VersionPolicy }
```

首期版本：`std.device.getTimeStamp` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::Now(TimeInfo {
    unix_millis: u64,             // 自 Unix epoch 起的毫秒数
    timezone_offset_seconds: i32, // 当前时区相对 UTC 偏移（含 DST），秒
    timezone_id: String,          // IANA 时区 id，如 "Asia/Shanghai"
})
```

| 字段 | 语义 |
|---|---|
| `unix_millis` | 绝对墙钟时刻，`u64`（1970-01-01T00:00:00Z 之后）。**不保证单调**：用户改系统时间、NTP 校准都会造成跳变，guest 不得用于计时 |
| `timezone_offset_seconds` | 取时刻时生效的时区偏移，**含夏令时**；东八区 = `28800` |
| `timezone_id` | IANA 时区 id；获取不到时用 `"UTC"` 兜底 |

### 语义边界

- 三个字段是**同一时刻的原子快照**：host 一次性读取，不允许跨调用拼装（避免 DST 切换窗口内偏移与 id 不一致）。
- 跨时区计算（未来时刻的偏移、DST 规则）不在本能力范围：guest 需要时自行解析 `timezone_id`，或等待未来 `std.device.getTimeStamp` 版本扩展。
- 单调计时请使用输入事件时间戳或宿主注入的帧时钟，不得用 `unix_millis` 差分。

## 4. Host 实现

本桥为本地能力，**应同一轮回投**。三字段必须同一时刻原子读取；宿主实现按各自平台 API 保证。

| Target | 来源 |
|---|---|
| webview | `Date.now()`（unix 毫秒）、`Date.getTimezoneOffset()`（注意符号相反）、`Intl.DateTimeFormat().resolvedOptions().timeZone` |
| win32 | `GetSystemTimeAsFileTime`/`GetSystemTimePreciseAsFileTime`、`GetTimeZoneInformation`（DST 感知偏移）、Windows 注册表时区 id（或 `_tzname` 兜底） |
| macOS | `NSDate`、`NSTimeZone`（`secondsFromGMT` 含 DST、`name` 为 IANA id） |
| android | `System.currentTimeMillis`、`TimeZone.getDefault()`（`getRawOffset`+DST 修正或 `getOffset(now)`、`getID`） |
| iOS | `NSDate`/`CFAbsoluteTime` 换算、`NSTimeZone.localTimeZone`（同 macOS） |

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：上述字段集 |

## 6. 验收

- 三字段原子性：单元测试模拟 DST 切换窗口，偏移与 id 一致。
- 偏移含 DST：已知时区（如 `Europe/Berlin` 冬季/夏季）分别断言。
- 与单调时钟区分：文档断言 + 测试明确 `unix_millis` 不用于计时。
