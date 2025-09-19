use anyhow::{Context, Result};
use log::{info, warn};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Services::*;

/// 服务配置
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub executable_path: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: Option<PathBuf>,
    pub stdout_path: Option<PathBuf>,
    pub stderr_path: Option<PathBuf>,
}

/// 服务管理器
pub struct ServiceManager {
    scm: SC_HANDLE,
}

impl ServiceManager {
    /// 创建新的服务管理器
    pub fn new() -> Result<Self> {
        let scm = unsafe {
            OpenSCManagerW(
                std::ptr::null(),
                std::ptr::null(),
                SC_MANAGER_ALL_ACCESS,
            )
        };

        if scm == 0 {
            return Err(anyhow::anyhow!("Failed to open Service Control Manager"));
        }

        Ok(Self { scm })
    }

    /// 安装服务
    pub fn install_service(&self, config: &ServiceConfig) -> Result<()> {
        let service_name = to_wstring(&config.name);
        let display_name = to_wstring(&config.display_name);

        // 获取当前可执行文件的路径（rust-nssm自身）
        let current_exe = std::env::current_exe()
            .context("Failed to get current executable path")?;

        // 构建服务命令行：rust-nssm.exe run --name <service_name>
        let mut command_line = OsString::new();
        command_line.push("\"");
        command_line.push(&current_exe);
        command_line.push("\" run --name \"");
        command_line.push(&config.name);
        command_line.push("\"");

        let binary_path = to_wstring(&command_line.to_string_lossy());

        // 创建服务
        let service = unsafe {
            CreateServiceW(
                self.scm,
                service_name.as_ptr(),
                display_name.as_ptr(),
                SERVICE_ALL_ACCESS,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_AUTO_START,
                SERVICE_ERROR_NORMAL,
                binary_path.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };

        if service == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_SERVICE_EXISTS {
                return Err(anyhow::anyhow!("Service already exists"));
            }
            return Err(anyhow::anyhow!("Failed to create service: error {}", error));
        }

        // 设置服务描述
        if let Err(e) = self.set_service_description(service, &config.description) {
            warn!("Failed to set service description: {}", e);
        }

        // 保存额外配置
        if let Err(e) = self.save_service_config(config) {
            warn!("Failed to save service config: {}", e);
        }

        // 关闭服务句柄
        unsafe { CloseServiceHandle(service); }

        info!("Service '{}' installed successfully", config.name);
        Ok(())
    }

    /// 卸载服务
    pub fn uninstall_service(&self, service_name: &str) -> Result<()> {
        let service = self.open_service(service_name, SERVICE_ALL_ACCESS)?;

        // 停止服务
        self.stop_service_internal(service);

        // 删除服务
        let result = unsafe { DeleteService(service) };
        if result == 0 {
            return Err(anyhow::anyhow!("Failed to delete service"));
        }

        // 关闭服务句柄
        unsafe { CloseServiceHandle(service); }

        // 删除注册表配置
        if let Err(e) = self.delete_service_config(service_name) {
            warn!("Failed to delete service config: {}", e);
        }

        info!("Service '{}' uninstalled successfully", service_name);
        Ok(())
    }

    /// 启动服务
    pub fn start_service(&self, service_name: &str) -> Result<()> {
        let service = self.open_service(service_name, SERVICE_ALL_ACCESS)?;

        let result = unsafe { StartServiceW(service, 0, std::ptr::null()) };
        if result == 0 {
            return Err(anyhow::anyhow!("Failed to start service"));
        }

        unsafe { CloseServiceHandle(service); }
        info!("Service '{}' started successfully", service_name);
        Ok(())
    }

    /// 停止服务
    pub fn stop_service(&self, service_name: &str) -> Result<()> {
        let service = self.open_service(service_name, SERVICE_ALL_ACCESS)?;

        self.stop_service_internal(service);
        unsafe { CloseServiceHandle(service); }

        info!("Service '{}' stopped successfully", service_name);
        Ok(())
    }

    /// 重启服务
    pub fn restart_service(&self, service_name: &str) -> Result<()> {
        self.stop_service(service_name)?;
        std::thread::sleep(std::time::Duration::from_secs(2));
        self.start_service(service_name)?;
        info!("Service '{}' restarted successfully", service_name);
        Ok(())
    }

    /// 获取服务状态
    pub fn get_service_status(&self, service_name: &str) -> Result<u32> {
        let service = self.open_service(service_name, SERVICE_QUERY_STATUS)?;

        let mut status = SERVICE_STATUS {
            dwServiceType: 0,
            dwCurrentState: 0,
            dwControlsAccepted: 0,
            dwWin32ExitCode: 0,
            dwServiceSpecificExitCode: 0,
            dwCheckPoint: 0,
            dwWaitHint: 0,
        };
        let result = unsafe { QueryServiceStatus(service, &mut status) };

        unsafe { CloseServiceHandle(service); }

        if result == 0 {
            return Err(anyhow::anyhow!("Failed to query service status"));
        }

        Ok(status.dwCurrentState)
    }

    /// 列出所有服务
    pub fn list_services(&self) -> Result<Vec<String>> {
        let mut services = Vec::new();
        let mut bytes_needed = 0u32;
        let mut services_returned = 0u32;
        let mut resume_handle = 0u32;

        // 第一次调用获取缓冲区大小
        unsafe {
            EnumServicesStatusW(
                self.scm,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                std::ptr::null_mut(),
                0,
                &mut bytes_needed,
                &mut services_returned,
                &mut resume_handle,
            );
        }

        // 分配缓冲区
        let mut buffer = vec![0u8; bytes_needed as usize];
        let buffer_ptr = buffer.as_mut_ptr() as *mut ENUM_SERVICE_STATUSW;

        // 获取服务列表
        let result = unsafe {
            EnumServicesStatusW(
                self.scm,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                buffer_ptr,
                bytes_needed,
                &mut bytes_needed,
                &mut services_returned,
                &mut resume_handle,
            )
        };

        if result != 0 {
            let services_slice = unsafe {
                std::slice::from_raw_parts(buffer_ptr, services_returned as usize)
            };

            for service_info in services_slice {
                let service_name = unsafe {
                    OsString::from_wide(std::slice::from_raw_parts(
                        service_info.lpServiceName,
                        wcslen(service_info.lpServiceName)
                    ))
                    .to_string_lossy()
                    .to_string()
                };
                services.push(service_name);
            }
        }

        Ok(services)
    }

    /// 打开服务
    fn open_service(&self, service_name: &str, access: u32) -> Result<SC_HANDLE> {
        let service_name_w = to_wstring(service_name);
        let service = unsafe {
            OpenServiceW(self.scm, service_name_w.as_ptr(), access)
        };

        if service == 0 {
            return Err(anyhow::anyhow!("Failed to open service"));
        }

        Ok(service)
    }

    /// 停止服务内部实现
    fn stop_service_internal(&self, service: SC_HANDLE) {
        let mut status = SERVICE_STATUS {
            dwServiceType: 0,
            dwCurrentState: 0,
            dwControlsAccepted: 0,
            dwWin32ExitCode: 0,
            dwServiceSpecificExitCode: 0,
            dwCheckPoint: 0,
            dwWaitHint: 0,
        };
        unsafe { ControlService(service, SERVICE_CONTROL_STOP, &mut status); }
    }

    /// 设置服务描述
    fn set_service_description(&self, service: SC_HANDLE, description: &str) -> Result<()> {
        let desc_w = to_wstring(description);
        let description_info = SERVICE_DESCRIPTIONW {
            lpDescription: desc_w.as_ptr() as *mut _,
        };

        let result = unsafe {
            ChangeServiceConfig2W(
                service,
                SERVICE_CONFIG_DESCRIPTION,
                &description_info as *const _ as *const _,
            )
        };

        if result == 0 {
            return Err(anyhow::anyhow!("Failed to set service description"));
        }

        Ok(())
    }

    /// 保存服务配置到注册表
    fn save_service_config(&self, config: &ServiceConfig) -> Result<()> {
        let key_path = format!("SYSTEM\\CurrentControlSet\\Services\\{}\\Parameters", config.name);
        let key_path_w = to_wstring(&key_path);

        let mut hkey = HKEY::default();
        let result = unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                key_path_w.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                std::ptr::null(),
                &mut hkey,
                std::ptr::null_mut(),
            )
        };

        if result != ERROR_SUCCESS {
            return Err(anyhow::anyhow!("Failed to create registry key"));
        }

        // 保存工作目录
        if let Some(work_dir) = &config.working_directory {
            self.save_reg_string(hkey, "WorkingDirectory", &work_dir.to_string_lossy())?;
        }

        // 保存输出路径
        if let Some(stdout_path) = &config.stdout_path {
            self.save_reg_string(hkey, "StdoutPath", &stdout_path.to_string_lossy())?;
        }

        if let Some(stderr_path) = &config.stderr_path {
            self.save_reg_string(hkey, "StderrPath", &stderr_path.to_string_lossy())?;
        }

        // 保存目标可执行文件路径
        self.save_reg_string(hkey, "TargetExecutable", &config.executable_path.to_string_lossy())?;

        // 保存参数
        if !config.arguments.is_empty() {
            let args_json = serde_json::to_string(&config.arguments)?;
            self.save_reg_string(hkey, "Arguments", &args_json)?;
        }

        unsafe { RegCloseKey(hkey); }
        Ok(())
    }

    /// 保存字符串到注册表
    fn save_reg_string(&self, hkey: HKEY, name: &str, value: &str) -> Result<()> {
        let name_w = to_wstring(name);
        let value_w = to_wstring(value);
        let value_bytes = unsafe {
            std::slice::from_raw_parts(
                value_w.as_ptr() as *const u8,
                value_w.len() * 2,
            )
        };

        let result = unsafe {
            RegSetValueExW(
                hkey,
                name_w.as_ptr(),
                0,
                REG_SZ,
                value_bytes.as_ptr(),
                value_bytes.len() as u32,
            )
        };

        if result != ERROR_SUCCESS {
            return Err(anyhow::anyhow!("Failed to set registry value"));
        }

        Ok(())
    }

    /// 删除服务配置
    fn delete_service_config(&self, service_name: &str) -> Result<()> {
        let key_path = format!("SYSTEM\\CurrentControlSet\\Services\\{}\\Parameters", service_name);
        let key_path_w = to_wstring(&key_path);

        let result = unsafe { RegDeleteKeyW(HKEY_LOCAL_MACHINE, key_path_w.as_ptr()) };
        if result != ERROR_SUCCESS {
            warn!("Failed to delete service config registry key");
        }

        Ok(())
    }
}

impl Drop for ServiceManager {
    fn drop(&mut self) {
        if self.scm != 0 {
            unsafe { CloseServiceHandle(self.scm); }
        }
    }
}

/// 转换字符串为宽字符串
fn to_wstring(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 计算宽字符串长度
unsafe fn wcslen(s: *const u16) -> usize {
    let mut len = 0;
    while *s.offset(len) != 0 {
        len += 1;
    }
    len as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_service_config_creation() {
        let config = ServiceConfig {
            name: "test_service".to_string(),
            display_name: "Test Service".to_string(),
            description: "A test service".to_string(),
            executable_path: PathBuf::from("C:\\test\\test.exe"),
            arguments: vec!["--test".to_string(), "--verbose".to_string()],
            working_directory: Some(PathBuf::from("C:\\test")),
            stdout_path: Some(PathBuf::from("C:\\test\\stdout.log")),
            stderr_path: Some(PathBuf::from("C:\\test\\stderr.log")),
        };

        assert_eq!(config.name, "test_service");
        assert_eq!(config.display_name, "Test Service");
        assert_eq!(config.executable_path, PathBuf::from("C:\\test\\test.exe"));
        assert_eq!(config.arguments.len(), 2);
        assert!(config.working_directory.is_some());
        assert!(config.stdout_path.is_some());
        assert!(config.stderr_path.is_some());
    }

    #[test]
    fn test_to_wstring() {
        let test_str = "Hello World";
        let wide_str = to_wstring(test_str);

        // 验证字符串以null结尾
        assert_eq!(wide_str[wide_str.len() - 1], 0);

        // 验证字符串长度（包括null终止符）
        assert_eq!(wide_str.len(), test_str.len() + 1);
    }

    #[test]
    fn test_wcslen() {
        let test_str = "Hello";
        let wide_str = to_wstring(test_str);

        unsafe {
            let len = wcslen(wide_str.as_ptr());
            assert_eq!(len, test_str.len());
        }
    }
}