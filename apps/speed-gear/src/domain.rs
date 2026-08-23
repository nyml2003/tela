//! 变速齿轮的领域状态与后端边界。

#[cfg(target_os = "windows")]
use crate::hook;

/// Windows 进程的不可复用身份。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProcessIdentity {
    /// 系统进程号。
    pub pid: u32,
    /// 进程创建时间的单调标识（由枚举后端提供）。
    pub creation_time: u64,
}

/// 进程可操作性。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessAccess {
    /// 可作为调速目标。
    Available,
    /// 当前权限不足。
    PermissionDenied,
    /// 不是 x64 目标。
    ArchitectureMismatch,
    /// 系统或受保护进程。
    Protected,
    /// 目标已经退出。
    Exited,
}

/// 供 Transfer 展示的进程资料。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessInfo {
    /// 进程身份。
    pub identity: ProcessIdentity,
    /// 显示名称。
    pub name: String,
    /// 可选完整路径。
    pub path: Option<String>,
    /// 是否有可见顶层窗口。
    pub visible_window: bool,
    /// 当前可操作性。
    pub access: ProcessAccess,
}

/// 受控倍率值。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rate(f64);

impl Rate {
    /// 最小倍率。
    pub const MIN: f64 = 0.25;
    /// 最大倍率。
    pub const MAX: f64 = 4.0;
    /// 正常倍率。
    pub const NORMAL: f64 = 1.0;

    /// 创建并规范化倍率。
    pub fn new(value: f64) -> Self {
        Self(value.clamp(Self::MIN, Self::MAX))
    }

    /// 读取倍率。
    pub fn value(self) -> f64 {
        self.0
    }

    /// 以正常倍率为中心的对数滑块位置。
    pub fn position(self) -> f64 {
        ((self.0.ln() - Self::MIN.ln()) / (Self::MAX.ln() - Self::MIN.ln())).clamp(0.0, 1.0)
    }

    /// 从滑块位置创建倍率。
    pub fn from_position(position: f64) -> Self {
        Self::new(
            (Self::MIN.ln() + (Self::MAX.ln() - Self::MIN.ln()) * position.clamp(0.0, 1.0)).exp(),
        )
    }
}

/// 单目标连接阶段。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    /// 未选择目标。
    NoTarget,
    /// 已选择但尚未连接。
    Selected(ProcessIdentity),
    /// 正在建立连接。
    Connecting(ProcessIdentity),
    /// 已连接并确认倍率。
    Connected(ProcessIdentity),
    /// 连接失败。
    Failed(ProcessIdentity, SpeedBackendError),
    /// 目标已结束。
    TargetExited(ProcessIdentity),
}

/// 后端错误。错误必须足够具体，让 UI 给出可操作反馈。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpeedBackendError {
    /// 目标已经退出。
    TargetExited,
    /// 当前用户权限不足。
    PermissionDenied,
    /// 目标架构不兼容。
    ArchitectureMismatch,
    /// 目标受保护或不允许控制。
    ProtectedTarget,
    /// 调速能力无法建立。
    HookUnavailable,
    /// 后端通信失败。
    Communication(String),
}

/// 显式连接的异步建立状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectStatus {
    /// 目标端能力已经验证，可以启用倍率控制。
    Ready,
    /// 目标端模块仍在加载或等待第一次握手。
    Pending,
}

/// 外部调速后端。
pub trait SpeedBackend {
    /// 列出当前用户可见的候选进程。
    fn enumerate(&mut self, self_pid: u32) -> Result<Vec<ProcessInfo>, SpeedBackendError>;

    /// 显式建立一个目标连接，初始倍率必须为正常倍率。
    fn connect(&mut self, target: &ProcessIdentity) -> Result<ConnectStatus, SpeedBackendError>;

    /// 轮询正在建立的连接；同步后端可以直接报告 `Ready`。
    fn poll_connect(
        &mut self,
        _target: &ProcessIdentity,
    ) -> Result<ConnectStatus, SpeedBackendError> {
        Ok(ConnectStatus::Ready)
    }

    /// 更新当前目标倍率。
    fn set_rate(&mut self, target: &ProcessIdentity, rate: Rate) -> Result<(), SpeedBackendError>;

    /// 停止调速并把目标基线恢复到正常倍率。
    fn stop(&mut self, target: &ProcessIdentity) -> Result<(), SpeedBackendError>;

    /// 控制端异常结束时由后端协议保障目标回到正常倍率。
    fn heartbeat(&mut self, target: &ProcessIdentity) -> Result<(), SpeedBackendError>;
}

/// 当前进程列表与选择状态。
#[derive(Clone, Debug, Default)]
pub struct ProcessCatalog {
    items: Vec<ProcessInfo>,
    query: String,
    selected: Option<ProcessIdentity>,
}

impl ProcessCatalog {
    /// 替换枚举快照，同时保留仍存在的查询和目标选择。
    pub fn replace(&mut self, mut items: Vec<ProcessInfo>) {
        items.sort_by(|left, right| {
            right
                .visible_window
                .cmp(&left.visible_window)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.identity.pid.cmp(&right.identity.pid))
        });
        let selected = self
            .selected
            .clone()
            .filter(|identity| items.iter().any(|item| &item.identity == identity));
        self.items = items;
        self.selected = selected;
    }

    /// 当前全部快照。
    pub fn items(&self) -> &[ProcessInfo] {
        &self.items
    }

    /// 当前搜索词。
    pub fn query(&self) -> &str {
        &self.query
    }

    /// 设置搜索词。
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
    }

    /// 当前选择。
    pub fn selected(&self) -> Option<&ProcessIdentity> {
        self.selected.as_ref()
    }

    /// 选择一个仍存在且可操作的目标。
    pub fn select(&mut self, identity: ProcessIdentity) -> Result<(), SpeedBackendError> {
        let item = self
            .items
            .iter()
            .find(|item| item.identity == identity)
            .ok_or(SpeedBackendError::TargetExited)?;
        if item.access != ProcessAccess::Available {
            return Err(match item.access {
                ProcessAccess::PermissionDenied => SpeedBackendError::PermissionDenied,
                ProcessAccess::ArchitectureMismatch => SpeedBackendError::ArchitectureMismatch,
                ProcessAccess::Protected => SpeedBackendError::ProtectedTarget,
                ProcessAccess::Exited => SpeedBackendError::TargetExited,
                ProcessAccess::Available => {
                    SpeedBackendError::Communication("unreachable".to_owned())
                }
            });
        }
        self.selected = Some(identity);
        Ok(())
    }

    /// 清空选择。
    pub fn clear_selection(&mut self) {
        self.selected = None;
    }

    /// 返回经过搜索的可见列表。
    pub fn filtered(&self) -> impl Iterator<Item = &ProcessInfo> {
        let query = self.query.trim().to_lowercase();
        self.items.iter().filter(move |item| {
            query.is_empty()
                || item.name.to_lowercase().contains(&query)
                || item.identity.pid.to_string().contains(&query)
                || item
                    .path
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&query)
        })
    }
}

/// 应用领域状态。
#[derive(Clone, Debug)]
pub struct SpeedGearState {
    /// 进程目录。
    pub processes: ProcessCatalog,
    /// 连接阶段。
    pub connection: ConnectionState,
    /// 当前已确认倍率。
    pub rate: Rate,
}

impl Default for SpeedGearState {
    fn default() -> Self {
        Self {
            processes: ProcessCatalog::default(),
            connection: ConnectionState::NoTarget,
            rate: Rate::new(Rate::NORMAL),
        }
    }
}

/// 领域控制器使用的后端无关错误结果。
pub type BackendResult<T> = Result<T, SpeedBackendError>;

/// Windows 后端：当前用户权限范围内的进程枚举与明确能力错误。
///
/// 进程枚举和身份校验属于平台边界；调速能力必须由随产品交付的目标端模块提供。目标
/// 端模块未就绪或拒绝目标时，这里返回 `HookUnavailable`，不会把“已加载进程”冒充为
/// “已连接调速”。
#[cfg(target_os = "windows")]
pub struct WindowsSpeedBackend {
    self_pid: u32,
    connection: Option<(ProcessIdentity, hook::HookConnection)>,
}

#[cfg(target_os = "windows")]
impl WindowsSpeedBackend {
    /// 创建 Windows 后端。
    pub fn new() -> Self {
        Self {
            self_pid: std::process::id(),
            connection: None,
        }
    }
}

#[cfg(target_os = "windows")]
impl Default for WindowsSpeedBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
impl SpeedBackend for WindowsSpeedBackend {
    fn enumerate(&mut self, self_pid: u32) -> Result<Vec<ProcessInfo>, SpeedBackendError> {
        self.self_pid = self_pid;
        enumerate_windows_processes(self_pid)
    }

    fn connect(&mut self, target: &ProcessIdentity) -> Result<ConnectStatus, SpeedBackendError> {
        if let Some((current, connection)) = &self.connection {
            if current == target {
                connection.set_rate((Rate::NORMAL * 1_000.0) as u64);
                return Ok(if connection.initialized() {
                    ConnectStatus::Ready
                } else {
                    ConnectStatus::Pending
                });
            }
        }
        self.connection = None;
        let connection = hook::HookConnection::connect(target.pid, target.creation_time)
            .map_err(SpeedBackendError::Communication)?;
        let status = if connection.initialized() {
            ConnectStatus::Ready
        } else {
            ConnectStatus::Pending
        };
        self.connection = Some((target.clone(), connection));
        Ok(status)
    }

    fn poll_connect(
        &mut self,
        target: &ProcessIdentity,
    ) -> Result<ConnectStatus, SpeedBackendError> {
        let Some((current, connection)) = &self.connection else {
            return Err(SpeedBackendError::HookUnavailable);
        };
        if current != target {
            return Err(SpeedBackendError::TargetExited);
        }
        if connection.initialized() {
            return Ok(ConnectStatus::Ready);
        }
        if connection.timed_out() {
            self.connection = None;
            return Err(SpeedBackendError::Communication(
                "目标 DLL 握手超时".to_owned(),
            ));
        }
        Ok(ConnectStatus::Pending)
    }

    fn set_rate(&mut self, target: &ProcessIdentity, rate: Rate) -> Result<(), SpeedBackendError> {
        let Some((current, connection)) = &self.connection else {
            return Err(SpeedBackendError::HookUnavailable);
        };
        if current != target {
            return Err(SpeedBackendError::TargetExited);
        }
        connection.set_rate((rate.value() * 1_000.0).round() as u64);
        Ok(())
    }

    fn stop(&mut self, target: &ProcessIdentity) -> Result<(), SpeedBackendError> {
        let Some((current, connection)) = &self.connection else {
            return Err(SpeedBackendError::HookUnavailable);
        };
        if current != target {
            return Err(SpeedBackendError::TargetExited);
        }
        connection.stop();
        self.connection = None;
        Ok(())
    }

    fn heartbeat(&mut self, target: &ProcessIdentity) -> Result<(), SpeedBackendError> {
        let Some((current, connection)) = &self.connection else {
            return Err(SpeedBackendError::HookUnavailable);
        };
        if current != target {
            return Err(SpeedBackendError::TargetExited);
        }
        connection.heartbeat();
        Ok(())
    }
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
fn enumerate_windows_processes(self_pid: u32) -> Result<Vec<ProcessInfo>, SpeedBackendError> {
    use std::collections::BTreeSet;

    use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_AMD64;
    use windows::Win32::System::Threading::{
        GetProcessInformation, IsProcessCritical, IsWow64Process2, OpenProcess,
        PROCESS_PROTECTION_LEVEL_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
        PROTECTION_LEVEL_NONE, ProcessProtectionLevelInfo,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };

    let mut visible_pids: BTreeSet<u32> = BTreeSet::new();
    unsafe extern "system" fn collect_visible_window(
        hwnd: windows::Win32::Foundation::HWND,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::core::BOOL {
        if unsafe { IsWindowVisible(hwnd).as_bool() } {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid != 0 {
                unsafe { (&mut *(lparam.0 as *mut BTreeSet<u32>)).insert(pid) };
            }
        }
        windows::core::BOOL(1)
    }
    unsafe {
        EnumWindows(
            Some(collect_visible_window),
            windows::Win32::Foundation::LPARAM(&mut visible_pids as *mut _ as isize),
        )
        .map_err(|error| SpeedBackendError::Communication(format!("EnumWindows: {error}")))?;
    }

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.map_err(|_| {
        SpeedBackendError::Communication("CreateToolhelp32Snapshot failed".to_owned())
    })?;
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(SpeedBackendError::Communication(
            "invalid process snapshot".to_owned(),
        ));
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut result = Vec::new();
    let mut current = unsafe { Process32FirstW(snapshot, &mut entry).is_ok() };
    while current {
        let pid = entry.th32ProcessID;
        if pid != self_pid {
            let name_end = entry
                .szExeFile
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..name_end]);
            let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) };
            let (access, creation_time, path) = if let Ok(handle) = handle {
                let creation_time = process_creation_time(handle).unwrap_or_default();
                let path = process_path(handle);
                let mut process_machine =
                    windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE(0);
                let mut native_machine =
                    windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE(0);
                let architecture_ok = unsafe {
                    IsWow64Process2(handle, &mut process_machine, Some(&mut native_machine)).is_ok()
                } && process_machine.0 == 0
                    && native_machine == IMAGE_FILE_MACHINE_AMD64;
                let mut protection = PROCESS_PROTECTION_LEVEL_INFORMATION::default();
                let protection_level = unsafe {
                    GetProcessInformation(
                        handle,
                        ProcessProtectionLevelInfo,
                        (&mut protection as *mut PROCESS_PROTECTION_LEVEL_INFORMATION).cast(),
                        std::mem::size_of::<PROCESS_PROTECTION_LEVEL_INFORMATION>() as u32,
                    )
                    .is_ok()
                        && protection.ProtectionLevel != PROTECTION_LEVEL_NONE
                };
                let mut critical = windows::core::BOOL(0);
                let critical = unsafe {
                    IsProcessCritical(handle, &mut critical).is_ok() && critical.as_bool()
                };
                let access = if creation_time == 0 {
                    ProcessAccess::PermissionDenied
                } else if protection_level || critical {
                    ProcessAccess::Protected
                } else if !architecture_ok {
                    ProcessAccess::ArchitectureMismatch
                } else {
                    ProcessAccess::Available
                };
                let _ = unsafe { CloseHandle(handle) };
                (access, creation_time, path)
            } else {
                (ProcessAccess::PermissionDenied, 0, None)
            };
            result.push(ProcessInfo {
                identity: ProcessIdentity { pid, creation_time },
                name,
                path,
                visible_window: visible_pids.contains(&pid),
                access,
            });
        }
        current = unsafe { Process32NextW(snapshot, &mut entry).is_ok() };
    }
    let _ = unsafe { CloseHandle(snapshot) };
    Ok(result)
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
fn process_creation_time(handle: windows::Win32::Foundation::HANDLE) -> Option<u64> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::GetProcessTimes;
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) }.ok()?;
    Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
fn process_path(handle: windows::Win32::Foundation::HANDLE) -> Option<String> {
    use windows::Win32::System::Threading::{PROCESS_NAME_FORMAT, QueryFullProcessImageNameW};
    use windows::core::PWSTR;
    let mut buffer = [0u16; 1024];
    let mut length = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

#[cfg(test)]
mod tests {
    use super::{ProcessAccess, ProcessCatalog, ProcessIdentity, ProcessInfo, Rate};

    fn process(pid: u32, visible: bool) -> ProcessInfo {
        ProcessInfo {
            identity: ProcessIdentity {
                pid,
                creation_time: pid as u64,
            },
            name: format!("app-{pid}"),
            path: None,
            visible_window: visible,
            access: ProcessAccess::Available,
        }
    }

    #[test]
    fn refresh_preserves_query_and_live_selection_but_not_reused_pid() {
        let mut catalog = ProcessCatalog::default();
        catalog.replace(vec![process(7, true), process(8, false)]);
        catalog.set_query("app");
        catalog.select(process(7, true).identity).unwrap();
        catalog.replace(vec![process(8, false), process(7, true)]);
        assert_eq!(catalog.query(), "app");
        assert_eq!(catalog.selected().unwrap().pid, 7);
        let mut reused = process(7, false);
        reused.identity.creation_time = 71;
        catalog.replace(vec![reused]);
        assert!(catalog.selected().is_none());
    }

    #[test]
    fn rate_round_trips_the_logarithmic_normal_center() {
        let rate = Rate::from_position(0.5);
        assert!((rate.value() - 1.0).abs() < 1e-9);
        assert!((Rate::new(0.25).position() - 0.0).abs() < 1e-9);
        assert!((Rate::new(4.0).position() - 1.0).abs() < 1e-9);
    }
}
