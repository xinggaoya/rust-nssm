use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rust-nssm")]
#[command(about = "A Rust-based Windows service manager similar to NSSM")]
#[command(version = "0.1.0")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 安装服务
    Install {
        /// 服务名称
        #[arg(short, long)]
        name: Option<String>,

        /// 显示名称
        #[arg(short = 'i', long)]
        display_name: Option<String>,

        /// 服务描述
        #[arg(short, long)]
        description: Option<String>,

        /// 可执行文件路径
        #[arg(short, long)]
        executable: Option<PathBuf>,

        /// 命令行参数
        #[arg(short, long, num_args = 0..)]
        args: Vec<String>,

        /// 工作目录
        #[arg(short = 'w', long)]
        working_directory: Option<PathBuf>,

        /// 标准输出重定向文件
        #[arg(long)]
        stdout: Option<PathBuf>,

        /// 标准错误重定向文件
        #[arg(long)]
        stderr: Option<PathBuf>,

        /// 服务名称（位置参数）
        #[arg(index = 1)]
        service_name: Option<String>,

        /// 可执行文件路径（位置参数）
        #[arg(index = 2)]
        service_executable: Option<PathBuf>,
    },

    /// 卸载服务
    Uninstall {
        /// 服务名称
        #[arg(short, long)]
        name: String,
    },

    /// 启动服务
    Start {
        /// 服务名称
        #[arg(short, long)]
        name: String,
    },

    /// 停止服务
    Stop {
        /// 服务名称
        #[arg(short, long)]
        name: String,
    },

    /// 重启服务
    Restart {
        /// 服务名称
        #[arg(short, long)]
        name: String,
    },

    /// 获取服务状态
    Status {
        /// 服务名称
        #[arg(short, long)]
        name: String,
    },

    /// 列出所有服务
    List,

    /// 运行服务（用于Windows服务主机）
    Run {
        /// 服务名称
        #[arg(short, long)]
        name: String,
    },
}