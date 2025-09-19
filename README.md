# Rust-NSSM

一个基于Rust的Windows服务管理工具，类似于NSSM (Non-Sucking Service Manager)。这个工具允许你将任意程序注册为Windows服务，并提供强大的进程管理和输出重定向功能。

## 🚀 功能特性

- ✅ **服务安装/卸载** - 完整的服务生命周期管理
- ✅ **服务启动/停止/重启** - 精确的服务控制
- ✅ **进程自动重启** - 智能故障恢复机制
- ✅ **输出重定向** - 完整的stdout/stderr日志管理
- ✅ **自定义工作目录** - 灵活的环境配置
- ✅ **多种启动类型** - 自动/手动/禁用启动选项
- ✅ **服务账户管理** - 支持自定义运行账户
- ✅ **配置持久化** - 注册表配置存储
- ✅ **多服务并发** - 支持同时管理多个服务
- ✅ **优雅停止** - 正确的进程终止处理
- ✅ **详细日志** - 完整的调试和监控信息

## 🛠️ 构建要求

- Windows 10/11
- Rust 1.70+
- Visual Studio Build Tools (C++开发工具)

## 🏗️ 构建项目

```bash
cargo build --release
```

构建后的可执行文件位于 `target/release/rust-nssm.exe`

## 📖 使用方法

### 安装服务

将任意程序安装为Windows服务：

```powershell
# 基本安装（需要管理员权限）
.\rust-nssm.exe install my-service "C:\path\to\program.exe"

# 带参数的安装
.\rust-nssm.exe install my-service "C:\path\to\program.exe" --args "--port 8080 --config config.json"

# 完整安装选项
.\rust-nssm.exe install my-service `
    --executable "C:\path\to\program.exe" `
    --args "--port 8080" `
    --display-name "My Service" `
    --description "A service managed by rust-nssm" `
    --start-type auto `
    --working-directory "C:\path\to\working\dir" `
    --stdout "C:\logs\stdout.log" `
    --stderr "C:\logs\stderr.log" `
    --account "NT AUTHORITY\LocalService"
```

### 服务管理

```powershell
# 启动服务
.\rust-nssm.exe start my-service

# 停止服务
.\rust-nssm.exe stop my-service

# 重启服务
.\rust-nssm.exe restart my-service

# 强制停止服务
.\rust-nssm.exe stop my-service --force
```

### 卸载服务

```powershell
# 安全卸载（先停止服务）
.\rust-nssm.exe uninstall my-service

# 强制卸载（不停止服务）
.\rust-nssm.exe uninstall my-service --force
```

### 查询服务状态

```powershell
# 查看服务状态
.\rust-nssm.exe status my-service

# 查看详细信息
.\rust-nssm.exe status my-service --verbose
```

## ⚙️ 命令行参数

### install - 安装服务

- `-n, --name <NAME>`: 服务名称 (必需)
- `-e, --executable <PATH>`: 可执行文件路径 (必需)
- `-i, --display-name <NAME>`: 显示名称
- `--description <DESC>`: 服务描述
- `--args <ARGS>`: 命令行参数 (可重复)
- `-w, --working-directory <PATH>`: 工作目录
- `--stdout <PATH>`: 标准输出重定向文件
- `--stderr <PATH>`: 标准错误重定向文件
- `-s, --start-type <TYPE>`: 启动类型 (auto/manual/disabled)
- `-a, --account <ACCOUNT>`: 服务账户
- `-p, --password <PASSWORD>`: 账户密码

### uninstall - 卸载服务

- `-n, --name <NAME>`: 服务名称 (必需)
- `-f, --force`: 强制卸载

### start - 启动服务

- `-n, --name <NAME>`: 服务名称 (必需)

### stop - 停止服务

- `-n, --name <NAME>`: 服务名称 (必需)
- `-f, --force`: 强制停止

### restart - 重启服务

- `-n, --name <NAME>`: 服务名称 (必需)

### status - 查看状态

- `-n, --name <NAME>`: 服务名称 (必需)
- `-v, --verbose`: 详细信息

## 💾 配置存储

服务配置存储在Windows注册表中：
```
HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Services\<服务名称>\Parameters
```

存储的配置包括：
- `WorkingDirectory`: 工作目录
- `StdoutPath`: 标准输出文件路径
- `StderrPath`: 标准错误文件路径
- `Arguments`: 命令行参数 (JSON格式)

## 📊 日志功能

程序内置日志功能，可以通过环境变量控制日志级别：

```powershell
# 设置日志级别
$env:RUST_LOG="info"
.\rust-nssm.exe install my-service "C:\path\to\program.exe"

# 详细日志（调试用）
$env:RUST_LOG="debug"
.\rust-nssm.exe start my-service
```

## 🔧 高级特性

### 进程自动重启
- 子进程意外退出时自动重启
- 指数退避重试策略（最多5次）
- 重启间隔逐渐增加：5s, 10s, 20s, 40s, 80s

### 多服务支持
- 支持同时管理多个独立服务
- 每个服务拥有独立的配置和进程空间
- 服务间完全隔离，互不影响

### 输出重定向
- 完整的stdout和stderr重定向
- 自动创建日志目录（如果不存在）
- 支持日志文件轮转（通过外部工具）

## 🎯 使用示例

### 示例1：安装Node.js应用为服务
```powershell
.\rust-nssm.exe install node-app `
    --executable "C:\Program Files\nodejs\node.exe" `
    --args "C:\projects\myapp\app.js" `
    --display-name "My Node.js App" `
    --description "Node.js web application" `
    --start-type auto `
    --working-directory "C:\projects\myapp" `
    --stdout "C:\logs\node-app\stdout.log" `
    --stderr "C:\logs\node-app\stderr.log"
```

### 示例2：安装Python脚本为服务
```powershell
.\rust-nssm.exe install python-service `
    --executable "C:\Python39\python.exe" `
    --args "C:\scripts\monitor.py" `
    --display-name "Python Monitor Service" `
    --start-type auto `
    --stdout "C:\logs\python\service.log"
```

### 示例3：安装批处理文件为服务
```powershell
.\rust-nssm.exe install batch-task `
    --executable "C:\Windows\System32\cmd.exe" `
    --args "/c C:\tasks\backup-task.bat" `
    --display-name "Backup Task" `
    --start-type manual `
    --stdout "C:\logs\backup\task.log"
```

## 🐛 故障排除

### 常见问题

1. **服务启动失败**
   - 检查是否以管理员身份运行
   - 确认可执行文件路径正确
   - 查看Windows事件查看器中的错误日志

2. **权限问题**
   - 确保对日志目录有写入权限
   - 检查服务账户权限设置

3. **进程频繁重启**
   - 检查目标程序是否有bug导致崩溃
   - 查看stderr日志获取错误信息

### 调试步骤

```powershell
# 启用详细日志
$env:RUST_LOG="debug"

# 重新安装服务
.\rust-nssm.exe uninstall my-service
.\rust-nssm.exe install my-service "C:\path\to\program.exe"

# 启动服务并观察日志
.\rust-nssm.exe start my-service

# 查看Windows事件日志
Get-WinEvent -LogName Application -MaxEvents 20 | Where-Object {$_.Message -like "*rust-nssm*"} | Format-List
```

## 🔒 安全注意事项

- 需要管理员权限来安装/卸载服务
- 谨慎配置服务账户权限
- 确保日志文件不被未授权访问
- 定期备份重要的服务配置

## 📄 许可证

MIT License - 详见 [LICENSE](LICENSE) 文件

## 🤝 贡献

欢迎提交Issue和Pull Request！

1. Fork 本仓库
2. 创建功能分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建Pull Request

## 📞 支持

如果您遇到问题或有建议，请：
- 提交 [Issue](https://github.com/yourusername/rust-nssm/issues)
- 查看 [Wiki](https://github.com/yourusername/rust-nssm/wiki) 文档
- 加入我们的讨论区

## 🙏 致谢

感谢以下开源项目的支持：
- [rust-lang](https://github.com/rust-lang/rust) - Rust语言
- [windows-rs](https://github.com/microsoft/windows-rs) - Windows API绑定
- [clap](https://github.com/clap-rs/clap) - 命令行解析

---

**⭐ 如果这个项目对您有帮助，请考虑给我们一个Star！**