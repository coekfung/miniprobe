use miniprobe_proto::{
    CpuStatus, DynamicStatus, MemoryStatus, NetworkStatus, StaticStatus, SystemStatus,
};

#[derive(Debug)]
pub struct StatusQuerent {
    system: sysinfo::System,
    net_interface: netdev::Interface,
}

impl StatusQuerent {
    pub fn try_new(if_name: Option<&str>) -> anyhow::Result<Self> {
        let system = sysinfo::System::new_all();
        let net_interface = match if_name {
            Some(name) => {
                let interface_list = netdev::get_interfaces();
                interface_list
                    .into_iter()
                    .find(|iface| iface.name == name)
                    .ok_or_else(|| anyhow::anyhow!("Network interface '{}' not found", name))?
            }
            None => netdev::get_default_interface()
                .map_err(|e| anyhow::anyhow!("Unable to open default interface: {}", e))?,
        };
        Ok(Self {
            system,
            net_interface,
        })
    }

    fn query_cpus(&mut self) -> Vec<CpuStatus> {
        self.system.refresh_cpu_all();
        let usages = self.system.cpus().iter().map(|cpu| cpu.cpu_usage());
        usages.map(|usage| CpuStatus { usage }).collect()
    }

    fn query_memory(&mut self) -> MemoryStatus {
        self.system.refresh_memory();
        MemoryStatus {
            total: self.system.total_memory(),
            used: self.system.used_memory(),
            swap_total: self.system.total_swap(),
            swap_used: self.system.used_swap(),
        }
    }

    fn query_network_status(&mut self) -> NetworkStatus {
        let _ = self.net_interface.update_stats();
        NetworkStatus {
            ifname: self.net_interface.name.clone(),
            rx_bytes: self
                .net_interface
                .stats
                .as_ref()
                .map(|stats| stats.rx_bytes),
            tx_bytes: self
                .net_interface
                .stats
                .as_ref()
                .map(|stats| stats.tx_bytes),
        }
    }

    pub fn query_dynamic(&mut self) -> DynamicStatus {
        DynamicStatus {
            cpu: self.query_cpus(),
            memory: self.query_memory(),
            network: self.query_network_status(),
        }
    }

    pub fn query_static() -> StaticStatus {
        let system_status = SystemStatus {
            system_name: sysinfo::System::name(),
            kernel_version: sysinfo::System::kernel_version(),
            os_version: sysinfo::System::os_version(),
            host_name: sysinfo::System::host_name(),
            cpu_arch: sysinfo::System::cpu_arch(),
        };
        StaticStatus {
            system: system_status,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_query_cpus() {
        let mut querent = StatusQuerent::try_new(None).expect("Failed to create querent");
        let _ = querent.query_cpus();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        let cpu_status = querent.query_cpus();

        println!("{:?}", cpu_status);
    }

    #[test]
    fn test_query_memory() {
        let mut querent = StatusQuerent::try_new(None).expect("Failed to create querent");
        let memory_status = querent.query_memory();

        println!("{:?}", memory_status);
    }

    #[test]
    fn test_query_network_status() {
        let mut querent = StatusQuerent::try_new(None).expect("Failed to create querent");
        let network_status = querent.query_network_status();

        println!("{:?}", network_status);
    }

    #[test]
    fn test_query_static() {
        let static_status = StatusQuerent::query_static();

        println!("{:?}", static_status);
    }
}
