use anyhow::{Context, Result};
use log::{error, info};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::RwLock;
use windows_service::service::{ServiceControl, ServiceState, ServiceType, ServiceStatus, ServiceControlAccept, ServiceExitCode};
use windows_service::service_control_handler::{ServiceStatusHandle, ServiceControlHandlerResult};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Services::*;

/// 计算宽字符串长度
unsafe fn wcslen(s: *const u16) -> usize {
    let mut len = 0;
    while *s.offset(len) != 0 {
        len += 1;
    }
    len as usize
}

/// 服务主机 - 负责管理子进程的生命周期
pub struct ServiceHost {
    service_name: String,
    executable_path: PathBuf,
    arguments: Vec<String>,
    working_directory: Option<PathBuf>,
    stdout_path: Option<PathBuf>,
    stderr_path: Option<PathBuf>,
    child_process: Arc<RwLock<Option<Child>>>,
    status_handle: Option<ServiceStatusHandle>,
    stop_requested: Arc<RwLock<bool>>,
}

impl ServiceHost {
    pub fn new(
        service_name: String,
        executable_path: PathBuf,
        arguments: Vec<String>,
        working_directory: Option<PathBuf>,
        stdout_path: Option<PathBuf>,
        stderr_path: Option<PathBuf>,
    ) -> Self {
        Self {
            service_name,
            executable_path,
            arguments,
            working_directory,
            stdout_path,
            stderr_path,
            child_process: Arc::new(RwLock::new(None)),
            status_handle: None,
            stop_requested: Arc::new(RwLock::new(false)),
        }
    }

    /// 启动服务
    pub fn start_service(&mut self) -> Result<()> {
        info!("Starting service: {}", self.service_name);
        info!("Executable: {:?}", self.executable_path);
        info!("Arguments: {:?}", self.arguments);
        info!("Working directory: {:?}", self.working_directory);

        // 启动子进程
        self.start_child_process().context("Failed to start child process")?;

        // 启动服务监控任务
        self.start_monitor_task();

        Ok(())
    }

    /// 停止服务
    pub fn stop_service(&mut self) -> Result<()> {
        info!("Stopping service: {}", self.service_name);

        // 停止子进程
        self.stop_child_process().context("Failed to stop child process")?;

        Ok(())
    }

    /// 启动子进程
    async fn start_child_process_async(&self) -> Result<Child> {
        let mut cmd = Command::new(&self.executable_path);

        // 设置工作目录
        if let Some(work_dir) = &self.working_directory {
            cmd.current_dir(work_dir);
        }

        // 设置参数
        cmd.args(&self.arguments);

        // 配置标准输入/输出/错误
        cmd.stdin(Stdio::null());

        // 配置输出重定向
        if let Some(stdout_path) = &self.stdout_path {
            let stdout_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(stdout_path)
                .context(format!("Failed to open stdout file: {:?}", stdout_path))?;
            cmd.stdout(Stdio::from(stdout_file));
        } else {
            cmd.stdout(Stdio::null());
        }

        if let Some(stderr_path) = &self.stderr_path {
            let stderr_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(stderr_path)
                .context(format!("Failed to open stderr file: {:?}", stderr_path))?;
            cmd.stderr(Stdio::from(stderr_file));
        } else {
            cmd.stderr(Stdio::null());
        }

        // 启动进程
        let child = cmd.spawn()
            .context(format!("Failed to start process: {:?}", self.executable_path))?;

        info!("Started child process with PID: {}", child.id());
        Ok(child)
    }

    /// 同步启动子进程
    fn start_child_process(&self) -> Result<()> {
        let child_process = self.child_process.clone();
        let executable_path = self.executable_path.clone();
        let working_directory = self.working_directory.clone();
        let stdout_path = self.stdout_path.clone();
        let stderr_path = self.stderr_path.clone();
        let arguments = self.arguments.clone();
        let service_name = self.service_name.clone();

        tokio::spawn(async move {
            info!("Attempting to start child process for service: {}", service_name);
            info!("Command: {:?} {:?}", executable_path, arguments);

            let mut cmd = Command::new(&executable_path);

            if let Some(work_dir) = &working_directory {
                info!("Setting working directory to: {:?}", work_dir);
                cmd.current_dir(work_dir);
            }

            cmd.args(&arguments);
            cmd.stdin(Stdio::null());

            if let Some(stdout_path) = &stdout_path {
                info!("Redirecting stdout to: {:?}", stdout_path);
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(stdout_path)
                {
                    Ok(stdout_file) => {
                        cmd.stdout(Stdio::from(stdout_file));
                    }
                    Err(e) => {
                        error!("Failed to open stdout file: {:?}", e);
                        cmd.stdout(Stdio::null());
                    }
                }
            } else {
                cmd.stdout(Stdio::null());
            }

            if let Some(stderr_path) = &stderr_path {
                info!("Redirecting stderr to: {:?}", stderr_path);
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(stderr_path)
                {
                    Ok(stderr_file) => {
                        cmd.stderr(Stdio::from(stderr_file));
                    }
                    Err(e) => {
                        error!("Failed to open stderr file: {:?}", e);
                        cmd.stderr(Stdio::null());
                    }
                }
            } else {
                cmd.stderr(Stdio::null());
            }

            match cmd.spawn() {
                Ok(child) => {
                    info!("Successfully started child process with PID: {}", child.id());
                    *child_process.write().await = Some(child);
                }
                Err(e) => {
                    error!("Failed to start child process: {}", e);
                    error!("Command: {:?} {:?}", executable_path, arguments);
                }
            }
        });

        Ok(())
    }

    /// 停止子进程
    fn stop_child_process(&self) -> Result<()> {
        let child_process = self.child_process.clone();

        // 在异步环境中停止进程
        tokio::spawn(async move {
            let mut child_guard = child_process.write().await;
            if let Some(mut child) = child_guard.take() {
                info!("Stopping child process with PID: {}", child.id());

                // 尝试优雅关闭
                if let Err(e) = child.kill() {
                    error!("Failed to kill child process: {}", e);
                }

                // 等待进程退出
                match child.wait() {
                    Ok(status) => {
                        info!("Child process exited with status: {}", status);
                    }
                    Err(e) => {
                        error!("Failed to wait for child process: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// 启动监控任务
    fn start_monitor_task(&self) {
        let child_process = self.child_process.clone();
        let executable_path = self.executable_path.clone();
        let working_directory = self.working_directory.clone();
        let stdout_path = self.stdout_path.clone();
        let stderr_path = self.stderr_path.clone();
        let arguments = self.arguments.clone();

        tokio::spawn(async move {
            loop {
                // 检查子进程是否还在运行
                {
                    let mut child_guard = child_process.write().await;
                    if let Some(ref mut child) = *child_guard {
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                info!("Child process exited with status: {}, restarting...", status);
                                *child_guard = None;

                                // 延迟重启
                                tokio::time::sleep(Duration::from_secs(5)).await;

                                // 重新启动子进程
                                let mut cmd = Command::new(&executable_path);

                                if let Some(work_dir) = &working_directory {
                                    cmd.current_dir(work_dir);
                                }

                                cmd.args(&arguments);
                                cmd.stdin(Stdio::null());

                                if let Some(stdout_path) = &stdout_path {
                                    let stdout_file = std::fs::OpenOptions::new()
                                        .create(true)
                                        .append(true)
                                        .open(stdout_path)
                                        .unwrap();
                                    cmd.stdout(Stdio::from(stdout_file));
                                } else {
                                    cmd.stdout(Stdio::null());
                                }

                                if let Some(stderr_path) = &stderr_path {
                                    let stderr_file = std::fs::OpenOptions::new()
                                        .create(true)
                                        .append(true)
                                        .open(stderr_path)
                                        .unwrap();
                                    cmd.stderr(Stdio::from(stderr_file));
                                } else {
                                    cmd.stderr(Stdio::null());
                                }

                                match cmd.spawn() {
                                    Ok(new_child) => {
                                        info!("Restarted child process with PID: {}", new_child.id());
                                        *child_guard = Some(new_child);
                                    }
                                    Err(e) => {
                                        error!("Failed to restart child process: {}", e);
                                        // 等待更长时间后重试
                                        tokio::time::sleep(Duration::from_secs(30)).await;
                                    }
                                }
                            }
                            Ok(None) => {
                                // 进程仍在运行
                            }
                            Err(e) => {
                                error!("Failed to check child process status: {}", e);
                            }
                        }
                    }
                }

                // 等待一段时间再次检查
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }

    /// 处理服务控制请求
    pub fn handle_service_control(&mut self, control: ServiceControl) -> ServiceControlHandlerResult {
        match control {
            ServiceControl::Stop => {
                info!("Received stop request for service: {}", self.service_name);
                if let Err(e) = self.stop_service() {
                    error!("Failed to stop service: {}", e);
                    return ServiceControlHandlerResult::NoError;
                }
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    }
}

/// 从注册表读取服务配置
pub fn load_service_config(service_name: &str) -> Result<(PathBuf, Vec<String>, Option<PathBuf>, Option<PathBuf>, Option<PathBuf>)> {
    use windows_sys::Win32::System::Registry::*;
    use windows_sys::Win32::System::Services::*;

    // 首先从服务配置中获取目标可执行文件路径
    let scm = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if scm == 0 {
        return Err(anyhow::anyhow!("Failed to open Service Control Manager"));
    }

    let service_name_w = service_name.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let service = unsafe { OpenServiceW(scm, service_name_w.as_ptr(), SERVICE_QUERY_CONFIG) };

    if service == 0 {
        unsafe { CloseServiceHandle(scm); }
        return Err(anyhow::anyhow!("Failed to open service: {}", service_name));
    }

    // 查询服务配置
    let mut bytes_needed = 0u32;
    unsafe { QueryServiceConfigW(service, std::ptr::null_mut(), 0, &mut bytes_needed); }

    if bytes_needed == 0 {
        unsafe {
            CloseServiceHandle(service);
            CloseServiceHandle(scm);
        }
        return Err(anyhow::anyhow!("Failed to query service config size"));
    }

    let mut buffer = vec![0u8; bytes_needed as usize];
    let config_ptr = buffer.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW;

    let result = unsafe { QueryServiceConfigW(service, config_ptr, bytes_needed, &mut bytes_needed) };

    if result == 0 {
        unsafe {
            CloseServiceHandle(service);
            CloseServiceHandle(scm);
        }
        return Err(anyhow::anyhow!("Failed to query service config"));
    }

    // 解析二进制路径和参数
    let service_config = unsafe { &*config_ptr };
    let binary_path = unsafe {
        OsString::from_wide(std::slice::from_raw_parts(
            service_config.lpBinaryPathName,
            wcslen(service_config.lpBinaryPathName)
        )).to_string_lossy().to_string()
    };

    unsafe {
        CloseServiceHandle(service);
        CloseServiceHandle(scm);
    }

    // 现在从Parameters注册表项读取额外的配置
    let key_path = format!("SYSTEM\\CurrentControlSet\\Services\\{}\\Parameters", service_name);
    let key_path_w = key_path.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();

    let mut hkey = HKEY::default();
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            key_path_w.as_ptr(),
            0,
            KEY_READ,
            &mut hkey,
        )
    };

    let mut executable_path = PathBuf::new();
    let mut arguments = Vec::new();
    let mut working_directory = None;
    let mut stdout_path = None;
    let mut stderr_path = None;

    if result == ERROR_SUCCESS {
        // 读取目标可执行文件路径
        if let Ok(target_exe) = read_reg_string(hkey, "TargetExecutable") {
            executable_path = PathBuf::from(target_exe);
        }

        // 读取工作目录
        if let Ok(work_dir) = read_reg_string(hkey, "WorkingDirectory") {
            working_directory = Some(PathBuf::from(work_dir));
        }

        // 读取输出路径
        if let Ok(stdout) = read_reg_string(hkey, "StdoutPath") {
            stdout_path = Some(PathBuf::from(stdout));
        }

        if let Ok(stderr) = read_reg_string(hkey, "StderrPath") {
            stderr_path = Some(PathBuf::from(stderr));
        }

        // 读取参数
        if let Ok(args_json) = read_reg_string(hkey, "Arguments") {
            if let Ok(args) = serde_json::from_str::<Vec<String>>(&args_json) {
                arguments = args;
            }
        }

        unsafe { RegCloseKey(hkey); }
    }

    Ok((executable_path, arguments, working_directory, stdout_path, stderr_path))
}

/// 读取注册表字符串值
fn read_reg_string(hkey: HKEY, name: &str) -> Result<String> {
    use windows_sys::Win32::System::Registry::*;

    let name_w = name.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();

    let mut buffer_type = 0u32;
    let mut buffer_size = 0u32;

    // 查询缓冲区大小
    let result = unsafe {
        RegQueryValueExW(
            hkey,
            name_w.as_ptr(),
            std::ptr::null_mut(),
            &mut buffer_type,
            std::ptr::null_mut(),
            &mut buffer_size,
        )
    };

    if result != ERROR_SUCCESS || buffer_type != REG_SZ {
        return Err(anyhow::anyhow!("Failed to query registry value"));
    }

    // 读取数据
    let mut buffer = vec![0u16; (buffer_size / 2) as usize];
    let result = unsafe {
        RegQueryValueExW(
            hkey,
            name_w.as_ptr(),
            std::ptr::null_mut(),
            &mut buffer_type,
            buffer.as_mut_ptr() as *mut _,
            &mut buffer_size,
        )
    };

    if result != ERROR_SUCCESS {
        return Err(anyhow::anyhow!("Failed to read registry value"));
    }

    // 移除null终止符并转换为字符串
    if let Some(null_pos) = buffer.iter().position(|&c| c == 0) {
        buffer.truncate(null_pos);
    }

    Ok(String::from_utf16_lossy(&buffer))
}

/// 从服务二进制路径解析出目标可执行文件路径
fn parse_target_executable_path(_binary_path: &str) -> Result<PathBuf> {
    // 注意：这个函数现在需要service_name参数，但由于调用结构限制，
    // 我们将直接在load_service_config中处理路径解析
    Err(anyhow::anyhow!("此函数已弃用，请在load_service_config中直接处理"))
}

/// 启动服务主循环
pub fn run_service(service_name: &str) -> Result<()> {
    // 从注册表读取配置
    let (executable_path, arguments, working_directory, stdout_path, stderr_path) = load_service_config(service_name)
        .context("Failed to load service config")?;

    // 验证可执行文件是否存在
    if !executable_path.exists() {
        return Err(anyhow::anyhow!("Target executable does not exist: {:?}", executable_path));
    }

    info!("Loading service '{}' with executable: {:?}", service_name, executable_path);

    // 检查是否在服务环境中运行
    if std::env::var("RUST_NSSM_DEBUG").unwrap_or_default() == "1" {
        info!("Running in debug mode (non-service environment)");
        run_debug_mode(service_name, executable_path, arguments, working_directory, stdout_path, stderr_path)
    } else {
        // 使用windows_service crate来正确实现Windows服务
        run_windows_service(service_name, executable_path, arguments, working_directory, stdout_path, stderr_path)
    }
}

/// 运行Windows服务 - 使用服务分发器正确实现
fn run_windows_service(
    service_name: &str,
    executable_path: PathBuf,
    arguments: Vec<String>,
    working_directory: Option<PathBuf>,
    stdout_path: Option<PathBuf>,
    stderr_path: Option<PathBuf>,
) -> Result<()> {
    use windows_service::service_dispatcher;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    log_to_file(&format!("Starting Windows service mode for: {}", service_name));

    // 将服务配置转换为可传递给服务主函数的格式
    let service_name_os = OsString::from_wide(service_name.encode_utf16().collect::<Vec<u16>>().as_slice());

    // 存储服务配置到全局变量，以便服务主函数可以访问
    // 这里使用线程局部存储或全局状态
    if let Err(e) = set_service_global_config(
        service_name.to_string(),
        executable_path,
        arguments,
        working_directory,
        stdout_path,
        stderr_path,
    ) {
        let error_msg = format!("Failed to set service global config: {}", e);
        log_to_file(&error_msg);
        return Err(anyhow::anyhow!("{}", error_msg));
    }

    log_to_file("Starting service dispatcher...");

    // 使用服务分发器启动服务 - 这是正确的Windows服务启动方式
    match service_dispatcher::start(service_name_os, ffi_service_main) {
        Ok(()) => {
            log_to_file("Service dispatcher started successfully");
            Ok(())
        }
        Err(e) => {
            let error_msg = format!("Failed to start service dispatcher: {}", e);
            log_to_file(&error_msg);
            Err(anyhow::anyhow!("{}", error_msg))
        }
    }
}

// 全局服务配置存储
static mut SERVICE_CONFIG: Option<ServiceConfig> = None;

/// 服务配置结构
#[derive(Clone)]
struct ServiceConfig {
    name: String,
    executable_path: PathBuf,
    arguments: Vec<String>,
    working_directory: Option<PathBuf>,
    stdout_path: Option<PathBuf>,
    stderr_path: Option<PathBuf>,
}

/// 设置服务全局配置
fn set_service_global_config(
    name: String,
    executable_path: PathBuf,
    arguments: Vec<String>,
    working_directory: Option<PathBuf>,
    stdout_path: Option<PathBuf>,
    stderr_path: Option<PathBuf>,
) -> Result<()> {
    unsafe {
        SERVICE_CONFIG = Some(ServiceConfig {
            name,
            executable_path,
            arguments,
            working_directory,
            stdout_path,
            stderr_path,
        });
    }
    Ok(())
}

/// 获取服务全局配置
fn get_service_global_config() -> Result<ServiceConfig> {
    unsafe {
        SERVICE_CONFIG.clone().ok_or_else(|| anyhow::anyhow!("Service config not set"))
    }
}

/// FFI服务主函数 - Windows服务入口点
extern "system" fn ffi_service_main(argc: u32, argv: *mut *mut u16) {
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service::{ServiceControl, ServiceState, ServiceStatus, ServiceType};

    log_to_file("FFI service main called");

    // 获取服务配置
    let config = match get_service_global_config() {
        Ok(config) => config,
        Err(e) => {
            log_to_file(&format!("Failed to get service config: {}", e));
            return;
        }
    };

    let service_name = config.name.clone();

    // 定义服务控制处理器
    let stop_requested = Arc::new(Mutex::new(false));
    let stop_requested_clone = stop_requested.clone();
    let service_name_clone = service_name.clone();

    let service_control_handler = move |control| -> ServiceControlHandlerResult {
        match control {
            ServiceControl::Stop => {
                log_to_file(&format!("Received stop request for service: {}", service_name_clone));

                // 设置停止标志
                if let Ok(mut stop) = stop_requested_clone.lock() {
                    *stop = true;
                }

                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Shutdown => {
                log_to_file(&format!("Received shutdown request for service: {}", service_name_clone));

                // 设置停止标志
                if let Ok(mut stop) = stop_requested_clone.lock() {
                    *stop = true;
                }

                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    log_to_file("Registering service control handler...");

    // 注册服务控制处理器
    let handler_result = service_control_handler::register(service_name.clone(), service_control_handler);
    let status_handle = match handler_result {
        Ok(handle) => {
            log_to_file("Service control handler registered successfully");
            handle
        }
        Err(e) => {
            let error_msg = format!("Failed to register service control handler: {}", e);
            log_to_file(&error_msg);
            return;
        }
    };

    // 设置服务状态为运行中
    let status = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: windows_service::service::ServiceControlAccept::STOP,
        exit_code: windows_service::service::ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    };

    log_to_file("Setting service status to RUNNING...");
    if let Err(e) = status_handle.set_service_status(status) {
        log_to_file(&format!("Failed to set service status to running: {}", e));
        return;
    }

    log_to_file(&format!("Service '{}' started successfully", service_name));

    // 启动子进程管理器
    let stop_requested_clone = stop_requested.clone();
    let executable_path_clone = config.executable_path.clone();
    let arguments_clone = config.arguments.clone();
    let working_directory_clone = config.working_directory.clone();
    let stdout_path_clone = config.stdout_path.clone();
    let stderr_path_clone = config.stderr_path.clone();
    let service_name_clone = service_name.clone();

    log_to_file("Starting child process manager...");

    // 在单独的线程中管理子进程
    std::thread::spawn(move || {
        manage_child_process(
            &service_name_clone,
            &executable_path_clone,
            &arguments_clone,
            &working_directory_clone,
            &stdout_path_clone,
            &stderr_path_clone,
            &stop_requested_clone,
        );
    });

    log_to_file("Entering main service loop...");

    // 主循环 - 等待停止信号
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));

        // 检查是否收到停止请求
        if let Ok(stop) = stop_requested.lock() {
            if *stop {
                log_to_file("Stop signal received, breaking main loop");
                break;
            }
        }
    }

    // 更新服务状态为已停止
    let status = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: windows_service::service::ServiceControlAccept::empty(),
        exit_code: windows_service::service::ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    };

    log_to_file("Setting service status to STOPPED...");
    if let Err(e) = status_handle.set_service_status(status) {
        log_to_file(&format!("Failed to set service status to stopped: {}", e));
    } else {
        log_to_file(&format!("Service '{}' stopped successfully", service_name));
    }
}

/// 记录到文件
fn log_to_file(message: &str) {
    use std::fs::OpenOptions;
    use std::io::Write;

    let log_file = "D:\\dev\\Rust\\rust-nssm\\service_detailed.log";
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)
    {
        let _ = writeln!(file, "[{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), message);
    }
}

/// 管理子进程的函数
fn manage_child_process(
    service_name: &str,
    executable_path: &PathBuf,
    arguments: &[String],
    working_directory: &Option<PathBuf>,
    stdout_path: &Option<PathBuf>,
    stderr_path: &Option<PathBuf>,
    stop_requested: &Arc<Mutex<bool>>,
) {
    let mut attempt = 0u32;
    const MAX_ATTEMPTS: u32 = 5;
    const INITIAL_DELAY: u64 = 2;

    loop {
        // 检查是否收到停止请求
        if let Ok(stop) = stop_requested.lock() {
            if *stop {
                info!("Stop requested, exiting child process manager");
                break;
            }
        }

        // 尝试启动子进程
        match start_child_process_once(service_name, executable_path, arguments, working_directory, stdout_path, stderr_path) {
            Ok(mut child) => {
                attempt = 0; // 重置尝试计数

                // 等待子进程退出
                loop {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            info!("Child process exited with status: {}", status);
                            break;
                        }
                        Ok(None) => {
                            // 进程仍在运行，检查停止信号
                            if let Ok(stop) = stop_requested.lock() {
                                if *stop {
                                    info!("Stop requested, killing child process");
                                    let _ = child.kill();
                                    let _ = child.wait();
                                    return;
                                }
                            }
                            std::thread::sleep(std::time::Duration::from_secs(1));
                        }
                        Err(e) => {
                            error!("Error waiting for child process: {}", e);
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to start child process: {}", e);
                attempt += 1;

                if attempt >= MAX_ATTEMPTS {
                    error!("Max attempts reached, giving up");
                    break;
                }

                // 指数退避
                let delay = INITIAL_DELAY * u64::pow(2, attempt.min(8)); // 最多256秒
                info!("Retrying in {} seconds (attempt {}/{})", delay, attempt, MAX_ATTEMPTS);
                std::thread::sleep(std::time::Duration::from_secs(delay));
            }
        }

        // 在下次尝试前等待一下
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// 启动子进程一次
fn start_child_process_once(
    service_name: &str,
    executable_path: &PathBuf,
    arguments: &[String],
    working_directory: &Option<PathBuf>,
    stdout_path: &Option<PathBuf>,
    stderr_path: &Option<PathBuf>,
) -> Result<std::process::Child> {
    info!("Starting child process for service: {}", service_name);

    let mut cmd = Command::new(executable_path);

    // 设置工作目录
    if let Some(work_dir) = working_directory {
        cmd.current_dir(work_dir);
    }

    // 设置参数
    cmd.args(arguments);
    cmd.stdin(Stdio::null());

    // 配置标准输出
    if let Some(stdout_path) = stdout_path {
        let stdout_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(stdout_path)
            .context(format!("Failed to open stdout file: {:?}", stdout_path))?;
        cmd.stdout(Stdio::from(stdout_file));
    } else {
        cmd.stdout(Stdio::null());
    }

    // 配置标准错误
    if let Some(stderr_path) = stderr_path {
        let stderr_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(stderr_path)
            .context(format!("Failed to open stderr file: {:?}", stderr_path))?;
        cmd.stderr(Stdio::from(stderr_file));
    } else {
        cmd.stderr(Stdio::null());
    }

    let child = cmd.spawn()
        .context(format!("Failed to start process: {:?}", executable_path))?;

    info!("Started child process with PID: {}", child.id());
    Ok(child)
}

/// 调试模式运行（非服务环境）
fn run_debug_mode(
    service_name: &str,
    executable_path: PathBuf,
    arguments: Vec<String>,
    working_directory: Option<PathBuf>,
    stdout_path: Option<PathBuf>,
    stderr_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting debug mode for service: {}", service_name);
    info!("Executable: {:?}", executable_path);
    info!("Arguments: {:?}", arguments);
    info!("Working directory: {:?}", working_directory);
    info!("Stdout path: {:?}", stdout_path);
    info!("Stderr path: {:?}", stderr_path);

    // 创建停止标志
    let stop_requested = std::sync::Arc::new(std::sync::Mutex::new(false));
    let stop_requested_for_handler = stop_requested.clone();
    let stop_requested_for_main = stop_requested.clone();

    // 设置Ctrl+C处理器
    ctrlc::set_handler(move || {
        info!("Received Ctrl+C, stopping service...");
        if let Ok(mut stop) = stop_requested_for_handler.lock() {
            *stop = true;
        }
    }).expect("Error setting Ctrl+C handler");

    // 启动子进程管理器
    let executable_path_clone = executable_path.clone();
    let arguments_clone = arguments.clone();
    let working_directory_clone = working_directory.clone();
    let stdout_path_clone = stdout_path.clone();
    let stderr_path_clone = stderr_path.clone();
    let service_name_clone = service_name.to_string();
    let stop_requested_for_child = stop_requested.clone();

    std::thread::spawn(move || {
        manage_child_process(
            &service_name_clone,
            &executable_path_clone,
            &arguments_clone,
            &working_directory_clone,
            &stdout_path_clone,
            &stderr_path_clone,
            &stop_requested_for_child,
        );
    });

    info!("Service '{}' started in debug mode. Press Ctrl+C to stop.", service_name);

    // 主循环 - 等待停止信号
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));

        // 检查是否收到停止请求
        if let Ok(stop) = stop_requested_for_main.lock() {
            if *stop {
                break;
            }
        }
    }

    info!("Service '{}' stopped", service_name);
    Ok(())
}