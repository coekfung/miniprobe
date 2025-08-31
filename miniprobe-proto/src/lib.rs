use serde::{Deserialize, Serialize};

pub mod msg;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicStatus {
    pub cpu: Vec<CpuStatus>,
    pub memory: MemoryStatus,
    pub network: NetworkStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuStatus {
    pub usage: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStatus {
    pub total: u64,
    pub used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStatus {
    pub ifname: String,
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticStatus {
    pub system: SystemStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub system_name: Option<String>,
    pub kernel_version: Option<String>,
    pub os_version: Option<String>,
    pub host_name: Option<String>,
    pub cpu_arch: String,
}
