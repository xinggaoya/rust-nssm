mod cli;
mod service_host;
mod service_manager;

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Commands};
use log::{info, error};
use service_manager::{ServiceConfig, ServiceManager};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    env_logger::init();

    // 解析命令行参数
    let cli = Cli::parse();

    // 执行命令
    match cli.command {
        Commands::Install {
            name,
            display_name,
            description,
            executable,
            args,
            working_directory,
            stdout,
            stderr,
            service_name,
            service_executable,
        } => {
            // 优先使用位置参数，如果不存在则使用命名参数
            let final_name = service_name.or(name).ok_or_else(|| {
                anyhow::anyhow!("服务名称是必需的，请使用位置参数或 --name/-n 参数")
            })?;

            let final_executable = service_executable.or(executable).ok_or_else(|| {
                anyhow::anyhow!("可执行文件路径是必需的，请使用位置参数或 --executable/-e 参数")
            })?;

            install_service(final_name, display_name, description, final_executable, args, working_directory, stdout, stderr).await?;
        }
        Commands::Uninstall { name } => {
            uninstall_service(name).await?;
        }
        Commands::Start { name } => {
            start_service(name).await?;
        }
        Commands::Stop { name } => {
            stop_service(name).await?;
        }
        Commands::Restart { name } => {
            restart_service(name).await?;
        }
        Commands::Status { name } => {
            get_service_status(name).await?;
        }
        Commands::List => {
            list_services().await?;
        }
        Commands::Run { name } => {
            run_service_host(name).await?;
        }
    }

    Ok(())
}

/// 安装服务
async fn install_service(
    name: String,
    display_name: Option<String>,
    description: Option<String>,
    executable: PathBuf,
    args: Vec<String>,
    working_directory: Option<PathBuf>,
    stdout: Option<PathBuf>,
    stderr: Option<PathBuf>,
) -> Result<()> {
    // 验证可执行文件是否存在
    if !executable.exists() {
        return Err(anyhow::anyhow!("Executable file does not exist: {:?}", executable));
    }

    // 创建服务管理器
    let service_manager = ServiceManager::new()
        .context("Failed to create service manager")?;

    // 创建服务配置
    let config = ServiceConfig {
        name: name.clone(),
        display_name: display_name.unwrap_or_else(|| name.clone()),
        description: description.unwrap_or_else(|| format!("Service managed by rust-nssm: {}", name)),
        executable_path: executable,
        arguments: args,
        working_directory,
        stdout_path: stdout,
        stderr_path: stderr,
    };

    // 安装服务
    service_manager.install_service(&config)
        .context(format!("Failed to install service '{}'", name))?;

    println!("Service '{}' installed successfully!", name);
    Ok(())
}

/// 卸载服务
async fn uninstall_service(name: String) -> Result<()> {
    let service_manager = ServiceManager::new()
        .context("Failed to create service manager")?;

    service_manager.uninstall_service(&name)
        .context(format!("Failed to uninstall service '{}'", name))?;

    println!("Service '{}' uninstalled successfully!", name);
    Ok(())
}

/// 启动服务
async fn start_service(name: String) -> Result<()> {
    let service_manager = ServiceManager::new()
        .context("Failed to create service manager")?;

    service_manager.start_service(&name)
        .context(format!("Failed to start service '{}'", name))?;

    println!("Service '{}' started successfully!", name);
    Ok(())
}

/// 停止服务
async fn stop_service(name: String) -> Result<()> {
    let service_manager = ServiceManager::new()
        .context("Failed to create service manager")?;

    service_manager.stop_service(&name)
        .context(format!("Failed to stop service '{}'", name))?;

    println!("Service '{}' stopped successfully!", name);
    Ok(())
}

/// 重启服务
async fn restart_service(name: String) -> Result<()> {
    let service_manager = ServiceManager::new()
        .context("Failed to create service manager")?;

    service_manager.restart_service(&name)
        .context(format!("Failed to restart service '{}'", name))?;

    println!("Service '{}' restarted successfully!", name);
    Ok(())
}

/// 获取服务状态
async fn get_service_status(name: String) -> Result<()> {
    let service_manager = ServiceManager::new()
        .context("Failed to create service manager")?;

    let status = service_manager.get_service_status(&name)
        .context(format!("Failed to get service status '{}'", name))?;

    let status_name = match status {
        1 => "STOPPED",
        2 => "START_PENDING",
        3 => "STOP_PENDING",
        4 => "RUNNING",
        5 => "CONTINUE_PENDING",
        6 => "PAUSE_PENDING",
        7 => "PAUSED",
        _ => "UNKNOWN",
    };

    println!("Service '{}': {}", name, status_name);
    Ok(())
}

/// 列出服务
async fn list_services() -> Result<()> {
    let service_manager = ServiceManager::new()
        .context("Failed to create service manager")?;

    let services = service_manager.list_services()
        .context("Failed to list services")?;

    if services.is_empty() {
        println!("No services found.");
        return Ok(());
    }

    println!("Found {} services:", services.len());
    for service in services {
        println!("  - {}", service);
    }

    Ok(())
}

/// 运行服务主机
async fn run_service_host(name: String) -> Result<()> {
    info!("Starting service host for: {}", name);

    // 初始化日志文件输出
    if let Err(e) = init_file_logging() {
        error!("Failed to initialize file logging: {}", e);
    }

    // 这里应该初始化Windows服务框架
    // 简化版本，直接运行服务
    service_host::run_service(&name)?;

    Ok(())
}

/// 初始化文件日志
fn init_file_logging() -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let log_file = "D:\\dev\\Rust\\rust-nssm\\rust-nssm.log";
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)?;

    writeln!(file, "[{}] Service host starting...", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))?;

    Ok(())
}