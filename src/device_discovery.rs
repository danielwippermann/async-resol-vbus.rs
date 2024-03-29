use std::{
    collections::HashSet,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    time::Duration,
};

use async_std::net::UdpSocket;

use crate::{device_information::DeviceInformation, error::Result};

/// Allows discovery of VBus-over-TCP devices in a local network.
///
/// All VBus-over-TCP devices listen for UDPv4 broadcast messages on port 7053.
/// If such a message with the correct payload is received, the device sends
/// a unicast message back to the sender of the broadcast to identify itself.
///
/// The `DeviceDiscovery` type allows to send such broadcasts and collect all
/// associated replies.
#[derive(Debug)]
pub struct DeviceDiscovery {
    broadcast_addr: SocketAddr,
    rounds: u8,
    broadcast_timeout: Duration,
    fetch_port: u16,
    fetch_timeout: Duration,
}

impl DeviceDiscovery {
    /// Create a new `DeviceDiscovery` instance using default values.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> async_resol_vbus::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_resol_vbus::DeviceDiscovery;
    ///
    /// let discovery = DeviceDiscovery::new();
    /// let addresses = discovery.discover_device_addresses().await?;
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn new() -> DeviceDiscovery {
        let ip_addr = Ipv4Addr::new(255, 255, 255, 255);
        let broadcast_addr = SocketAddr::V4(SocketAddrV4::new(ip_addr, 7053));

        DeviceDiscovery {
            broadcast_addr,
            rounds: 3,
            broadcast_timeout: Duration::from_millis(500),
            fetch_port: 80,
            fetch_timeout: Duration::from_millis(2000),
        }
    }

    /// Set the broadcast address.
    pub fn set_broadcast_addr(&mut self, addr: SocketAddr) {
        self.broadcast_addr = addr;
    }

    /// Set the number of discovery rounds.
    pub fn set_rounds(&mut self, rounds: u8) {
        self.rounds = rounds;
    }

    /// Set the timeout used to wait for replies after each round's broadcast.
    pub fn set_broadcast_timeout(&mut self, timeout: Duration) {
        self.broadcast_timeout = timeout;
    }

    /// Set the port number used for fetching the device information.
    pub fn set_fetch_port(&mut self, port: u16) {
        self.fetch_port = port;
    }

    /// Set the timeout used for fetching the device information.
    pub fn set_fetch_timeout(&mut self, timeout: Duration) {
        self.fetch_timeout = timeout;
    }

    /// Discover all VBus-over-TCP devices and return their device information.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> async_resol_vbus::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_resol_vbus::DeviceDiscovery;
    ///
    /// let discovery = DeviceDiscovery::new();
    /// let devices = discovery.discover_devices().await?;
    /// #
    /// # Ok(()) }) }
    /// ```
    pub async fn discover_devices(&self) -> Result<Vec<DeviceInformation>> {
        let addresses = self.discover_device_addresses().await?;

        let mut devices = Vec::with_capacity(addresses.len());
        for mut address in addresses {
            address.set_port(self.fetch_port);

            if let Ok(device) = DeviceInformation::fetch(address, self.fetch_timeout).await {
                devices.push(device);
            }
        }

        Ok(devices)
    }

    /// Discover all VBus-over-TCP devices and return their addresses.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> async_resol_vbus::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_resol_vbus::DeviceDiscovery;
    ///
    /// let discovery = DeviceDiscovery::new();
    /// let addresses = discovery.discover_device_addresses().await?;
    /// #
    /// # Ok(()) }) }
    /// ```
    pub async fn discover_device_addresses(&self) -> Result<Vec<SocketAddr>> {
        let broadcast_socket = UdpSocket::bind("0.0.0.0:0").await?;
        broadcast_socket.set_broadcast(true)?;

        let query_bytes = b"---RESOL-BROADCAST-QUERY---";
        let reply_bytes = b"---RESOL-BROADCAST-REPLY---";

        let mut addresses = HashSet::new();
        for _ in 0..self.rounds {
            broadcast_socket
                .send_to(query_bytes, &self.broadcast_addr)
                .await?;

            let future = async_std::io::timeout::<_, ()>(self.broadcast_timeout, async {
                let mut buf = [0u8; 64];
                loop {
                    let (len, address) = broadcast_socket.recv_from(&mut buf).await?;
                    if len == reply_bytes.len() && &buf[0..len] == reply_bytes {
                        addresses.insert(address);
                    }
                }
            });

            drop(future.await);
        }

        let addresses = addresses.into_iter().collect();

        Ok(addresses)
    }
}

impl Default for DeviceDiscovery {
    fn default() -> DeviceDiscovery {
        DeviceDiscovery::new()
    }
}

#[cfg(test)]
mod tests {
    use async_std::net::{SocketAddr, TcpListener, UdpSocket};

    use super::*;

    use crate::test_utils::create_webserver;

    #[test]
    fn test() -> Result<()> {
        async_std::task::block_on(async {
            let device_addr = "0.0.0.0:0".parse::<SocketAddr>()?;
            let device_socket = UdpSocket::bind(device_addr).await?;
            let device_addr = device_socket.local_addr()?;

            let mut broadcast_addr = "255.255.255.255:0".parse::<SocketAddr>()?;
            broadcast_addr.set_port(device_addr.port());

            let web_addr = "0.0.0.0:0".parse::<SocketAddr>()?;
            let web_socket = TcpListener::bind(web_addr).await?;
            let web_addr = web_socket.local_addr()?;

            let device_future = async_std::task::spawn::<_, Result<()>>(async move {
                let mut buf = [0u8; 256];

                let query_bytes = b"---RESOL-BROADCAST-QUERY---";
                let reply_bytes = b"---RESOL-BROADCAST-REPLY---";

                loop {
                    let (len, addr) = device_socket.recv_from(&mut buf).await?;

                    let buf = &buf[0..len];
                    if buf == query_bytes {
                        device_socket.send_to(reply_bytes, addr).await?;
                    }
                }
            });

            let web_future =
                async_std::task::spawn(async move { create_webserver(web_socket).await });

            let discovery_future = async_std::task::spawn::<_, Result<()>>(async move {
                let mut discovery = DeviceDiscovery::new();
                discovery.set_broadcast_addr(broadcast_addr);
                discovery.set_broadcast_timeout(Duration::from_millis(100));
                discovery.set_fetch_port(web_addr.port());
                discovery.set_fetch_timeout(Duration::from_millis(100));

                let addresses = discovery.discover_device_addresses().await?;

                assert_eq!(1, addresses.len());
                assert_eq!(device_addr.port(), addresses[0].port());

                let devices = discovery.discover_devices().await?;

                assert_eq!(1, devices.len());

                Ok(())
            });

            discovery_future.await?;
            drop(device_future);
            drop(web_future);

            Ok(())
        })
    }
}
