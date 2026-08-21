# 001 — 启动"鼠标转圈"定位与 WM_SETCURSOR 光标修复

日期：2026-08-22 ｜ 涉及：`crates/targets/win32-static`、`apps/win32-editor` ｜ 状态：已修复（Release 验证通过）

## 现象

`tela-win32-editor-host.exe` 启动后**窗口展示正常，但鼠标指针一直转圈**（箭头+旋转圆圈），
看似"未响应"，但窗口内容、标题栏都在，进程也不退出。

## 误判与正解

第一反应是 **UI 线程卡死**（卡在 wgpu 初始化 / 帧构建 / 渲染里的某个无限循环或阻塞）。
据此做了两轮"屏蔽代码"定位（均排除）：

| 刀 | 屏蔽内容 | 结果 |
|---|---|---|
| 第一刀 | 注释 `WM_PAINT` 里 `GpuSession::new` + `gpu.render` 整段 | 仍转圈 → GPU 路径排除 |
| 第二刀 | 注释 `App::ensure_frame` 主体，直接 `return false` | 仍转圈 → 帧构建管线排除 |

**转折点**：从 cmd 控制台启动并捕获 stderr（此前日志都看不到）。已有的消息循环
eprintln 显示：

```
tela win32-static: msg #10 0x0113 wparam=0x7eb hwnd=0x60a26 dt=15ms
tela win32-static: msg #11 0x0113 wparam=0x7eb hwnd=0x60a26 dt=15ms
...
```

200 条消息持续泵送，dt 全部 <30ms —— **UI 线程活着，消息循环完全正常**。

## 根因

`window.rs` 的 `WM_SETCURSOR => LRESULT(1)` 把消息**吞掉但从不设置光标**：

```rust
// 修复前：返回 1 表示"已处理"，但没 SetCursor → 光标冻结
WM_SETCURSOR => LRESULT(1),
```

Windows 在进程启动时会给 GUI 程序的窗口显示"应用启动中"光标（`IDC_APPSTARTING`，
箭头+转圈）。正常应用收到 `WM_SETCURSOR` 后应把光标换成 `IDC_ARROW`；我们吞掉消息
却什么都不做，光标就**永远停留在启动中样式**——看起来像卡死，实际只是光标没换。

## 修复

```rust
WM_SETCURSOR => {
    // 光标样式必须显式设回箭头：仅返回 1 会吞掉消息，光标会停留在进程
    // 启动时的"应用启动中"（箭头+转圈）样式，看起来像卡死。
    unsafe {
        let _ = SetCursor(LoadCursorW(None, IDC_ARROW).ok());
    }
    LRESULT(1)
}
```

注意：`LoadCursorW` 返回 `Result<HCURSOR>`，`SetCursor` 接收 `Option<HCURSOR>`，
要用 `.ok()` 转换（直接传会编译失败，Win32 GNU 交叉构建报错）。

## 附带发现：日志 cap 的 break 会杀死消息循环

排障时发现第二个 bug：消息循环里日志上限的 `break` 直接**退出了整个消息循环**：

```rust
if message_count >= 200 {
    eprintln!("...");
    break; // ❌ 退出消息循环 → 应用约 3 秒后自己退出
}
```

本机进程内有一个隐藏窗口（hwnd=0x60a26，疑似 IME 输入法窗口）以 ~15ms 间隔狂发
`WM_TIMER`（0x0113），把消息数瞬间刷到 200，应用随即退出。修复：改用标志只静音
日志，不退出循环：

```rust
if !log_messages { continue; }
// ...打印...
if message_count >= 200 {
    log_messages = false;
}
```

> 后续已把逐条消息日志整体删除（排障期才临时开启），消息循环只保留退出时的汇总
> `message loop exited after N messages`，避免 IME 的 WM_TIMER 洪流刷爆控制台。

## 附带发现：点击后页面不更新 = 缺 InvalidateRect

点击导航按钮后 route 确实变了（`rebuild route=Settings` 日志证明帧状态在重建），
但画面停在旧页。根因：`WM_LBUTTONDOWN`/`WM_LBUTTONUP` 处理器 dispatch 指针事件后
**没有 `request_redraw(hwnd)`**。App 侧的 `invalidate_frame()` 只置脏标志，
没有 `InvalidateRect` 就不会有下一次 `WM_PAINT`，画面永不刷新。

```rust
// WM_LBUTTONDOWN / WM_LBUTTONUP / WM_MOUSEMOVE 内，dispatch 之后：
state.session.dispatch_pointer(event);
request_redraw(hwnd); // ❌ 缺失：状态变了但不重绘
```

教训：**Win32 壳里每次"状态可能变化的输入事件"处理完都必须主动请求重绘**，
不能指望系统自动刷。`WM_MOUSEMOVE` 同样需要（hover 高亮、光标反馈依赖它）。

## 附带发现：hover 光标不变的两层根因

1. DSL 按钮只声明 `clickable={true}`，**没声明 `hoverable={true}`** → 框架
   `update_hover` 只在 hoverable 节点上设置 hover_key（`ViewStateStore::hover_key()`），
   按钮根本没有 hover 状态。可点击 ≠ 可悬停，两个标志独立，按钮必须两个都开。
2. `WM_SETCURSOR` 硬编码箭头 → 即使有 hover 状态也不变手型。修复：shell 通过
   session 的 `hover_interactive()`（转发 `view_state.hover_key().is_some()`）查询，
   hover 时 `SetCursor(LoadCursorW(None, IDC_HAND).ok())`。
3. 连带：App 的 `handle_framed_actions` 把 `UiAction::Hover` 当普通动作忽略了
   （`_ => {}`），导致 hover 高亮也不刷新——需把 Hover 纳入 `changed = true`。

## 经验沉淀

1. **"鼠标转圈"不等于卡死**。先看消息循环日志（msg #n + dt 耗时）判断 UI 线程是否
   活着，再决定要不要往"卡死"方向查。
2. **分层屏蔽法**：每次只注释/屏蔽一段（GPU → 帧构建 → 应用层），每轮 `git diff`
   保持单点改动，配合"转圈消失 = 该层嫌疑"二分。本案例两刀都排除后，问题其实根本
   不在这两层——屏蔽法本身没错，但**先拿到日志可以少走弯路**。
3. **Win32 GUI 程序从控制台跑**：stderr 重定向到文件（`exe 2>log.txt`），
   `eprintln!` 每条自动 flush，能精确看到卡在哪个阶段。
4. **消息循环逐条日志只在排障期临时开启**（msg + 耗时 + 静音上限），定位完即删，
   保留退出汇总 `message loop exited after N messages`。
5. `GetMessageW(None)` 会收到**本线程所有窗口**的消息，包括 IME/隐藏窗口的
   `WM_TIMER` 洪流，统计消息量时注意别被带偏。
6. Windows 光标协议：`WM_SETCURSOR` 返回 1 前必须自己 `SetCursor`，否则光标样式
   不会更新；交给 `DefWindowProcW` 处理也行（会用窗口类光标或默认箭头）。
