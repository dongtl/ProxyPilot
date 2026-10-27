//! Start at logon through Task Scheduler, which can run the program with
//! highest privileges without a UAC prompt. The task is defined in XML because
//! `schtasks /Create` flags cannot turn off "stop on battery", which would
//! otherwise keep the task from starting on laptops.

#[cfg(windows)]
mod imp {
    use proxypilot_core::sys::hide_window;
    use std::path::Path;
    use std::process::{Command, Stdio};

    /// Distinct from the old Python tool's "ProxyPilot" task, so one cannot
    /// overwrite or delete the other's.
    const TASK_NAME: &str = "ProxyPilot Desktop";

    fn escape(text: &str) -> String {
        text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    }

    pub fn task_xml(exe: &Path, user: &str) -> String {
        let exe = escape(&exe.to_string_lossy());
        let user = escape(user);
        format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>ProxyPilot 代理分流助手开机启动</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>6</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec><Command>"{exe}"</Command><Arguments>--autostart</Arguments></Exec>
  </Actions>
</Task>
"#
        )
    }

    fn schtasks(args: &[&str]) -> Result<(), String> {
        let mut command = Command::new("schtasks.exe");
        command.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        hide_window(&mut command);
        // Its messages are in the system code page; the exit code is what we can rely on.
        let status = command.status().map_err(|error| format!("无法运行任务计划程序：{error}"))?;
        if status.success() { Ok(()) } else { Err(format!("任务计划程序返回错误（代码 {}）", status.code().unwrap_or(-1))) }
    }

    pub fn enable(scratch_dir: &Path) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
            (Ok(domain), Ok(name)) => format!("{domain}\\{name}"),
            (_, Ok(name)) => name,
            _ => return Err("无法确定当前用户".into()),
        };
        // schtasks reads the XML most reliably as UTF-16LE with a BOM.
        let mut bytes = vec![0xFF, 0xFE];
        for unit in task_xml(&exe, &user).encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let file = scratch_dir.join("autostart-task.xml");
        std::fs::write(&file, bytes).map_err(|error| format!("无法写入任务定义：{error}"))?;
        let path = file.to_string_lossy().into_owned();
        let result = schtasks(&["/Create", "/TN", TASK_NAME, "/XML", &path, "/F"]);
        let _ = std::fs::remove_file(&file);
        result.map_err(|error| format!("无法设置开机自启：{error}。需要以管理员身份运行"))
    }

    pub fn disable() -> Result<(), String> {
        if schtasks(&["/Query", "/TN", TASK_NAME]).is_err() {
            return Ok(());
        }
        schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]).map_err(|error| format!("无法取消开机自启：{error}"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn xml_escapes_paths_and_users() {
            let xml = task_xml(Path::new(r"C:\Program Files\A&B\ProxyPilot.exe"), "CORP\\li<x>");
            assert!(xml.contains(r#"<Command>"C:\Program Files\A&amp;B\ProxyPilot.exe"</Command>"#));
            assert!(xml.contains("<UserId>CORP\\li&lt;x&gt;</UserId>"));
            assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn enable(_scratch_dir: &std::path::Path) -> Result<(), String> {
        Err("当前系统暂不支持开机自启".into())
    }

    pub fn disable() -> Result<(), String> {
        Ok(())
    }
}

pub use imp::{disable, enable};
