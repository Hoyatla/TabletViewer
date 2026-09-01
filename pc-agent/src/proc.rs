//! Cross-platform process info wrapper.

use serde::Serialize;
use sysinfo::Pid;

#[derive(Serialize, Clone, Debug)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub cmd: String,
    pub exe: Option<String>,
    pub cwd: Option<String>,
    pub cpu_pct: f32,
    pub mem_kb: u64,
    pub status: String,
}

impl ProcessInfo {
    pub fn collect(sys: &sysinfo::System) -> Vec<Self> {
        let mut out: Vec<ProcessInfo> = sys
            .processes()
            .iter()
            .map(|(pid, p)| Self::from_process(*pid, p))
            .collect();
        out.sort_by_key(|p| p.pid);
        out
    }

    fn from_process(pid: Pid, p: &sysinfo::Process) -> ProcessInfo {
        ProcessInfo {
            pid: pid.as_u32(),
            parent_pid: p.parent().map(|p| p.as_u32()),
            name: p.name().to_string(),
            cmd: p.cmd().join(" "),
            exe: p.exe().map(|e| e.to_string_lossy().into_owned()),
            cwd: p.cwd().map(|c| c.to_string_lossy().into_owned()),
            cpu_pct: p.cpu_usage(),
            mem_kb: p.memory() / 1024,
            status: format!("{:?}", p.status()),
        }
    }
}
