use std::cmp::Ordering;

use async_resol_vbus::*;

fn compare_devices(l: &DeviceInformation, r: &DeviceInformation) -> Ordering {
    let mut result = l.address.ip().cmp(&r.address.ip());
    if result == Ordering::Equal {
        result = l.address.port().cmp(&r.address.port());
    }
    result
}

fn main() -> Result<()> {
    async_std::task::block_on(async {
        let mut discovery = DeviceDiscovery::new();
        // discovery.set_broadcast_addr("192.168.15.255:7053".parse().unwrap());
        discovery.set_fetch_port(3000);

        let mut known_devices = Vec::<DeviceInformation>::new();
        loop {
            println!("---- Discovering... ----");

            let mut found_devices = discovery.discover_devices().await?;
            found_devices.sort_by(|l, r| compare_devices(l, r));

            known_devices.sort_by(|l, r| compare_devices(l, r));

            for found_device in found_devices.iter() {
                let is_known_device = known_devices
                    .iter()
                    .any(|known_device| known_device.address == found_device.address);

                if !is_known_device {
                    let name = match &found_device.name {
                        Some(name) => name.as_str(),
                        None => "???",
                    };

                    println!("FOUND: {} {}", found_device.address, name);
                    known_devices.push(found_device.clone());
                }
            }

            let mut lost_device_addresses = Vec::new();

            for known_device in known_devices.iter() {
                let is_found_device = found_devices
                    .iter()
                    .any(|found_device| known_device.address == found_device.address);

                if !is_found_device {
                    let name = match &known_device.name {
                        Some(name) => name.as_str(),
                        None => "???",
                    };

                    println!("LOST:  {} {}", known_device.address, name);
                    lost_device_addresses.push(known_device.address);
                }
            }

            for lost_device_address in lost_device_addresses {
                let pos = known_devices
                    .iter()
                    .position(|known_device| known_device.address == lost_device_address);

                if let Some(idx) = pos {
                    known_devices.remove(idx);
                }
            }

            async_std::task::sleep(std::time::Duration::from_secs(10)).await;
        }
    })
}
