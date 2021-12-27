use std::{net::SocketAddr, time::Duration};

use async_std::{net::TcpStream, prelude::*};

use crate::error::Result;

/// A struct containing information about a VBus-over-TCP device.
#[derive(Debug, Clone)]
pub struct DeviceInformation {
    /// The `SocketAddr` of the web server.
    pub address: SocketAddr,

    /// The vendor of the device.
    pub vendor: Option<String>,

    /// The product name of the device.
    pub product: Option<String>,

    /// The serial number of the device.
    pub serial: Option<String>,

    /// The firmware version of the device.
    pub version: Option<String>,

    /// The firmware build of the device.
    pub build: Option<String>,

    /// The user-chosen name of the device.
    pub name: Option<String>,

    /// The comma separated list of features supported by the device.
    pub features: Option<String>,
}

impl DeviceInformation {
    pub(crate) fn find_http_body_idx(buf: &[u8]) -> Option<usize> {
        let mut body_idx = None;

        let len = buf.len();
        let mut idx = 0;
        while idx + 4 <= len {
            if buf[idx] == 13 && buf[idx + 1] == 10 && buf[idx + 2] == 13 && buf[idx + 3] == 10 {
                body_idx = Some(idx + 4);
                break;
            }

            idx += 1;
        }

        body_idx
    }

    /// Fetch and parse the information from a VBus-over-TCP device.
    ///
    /// This function performs a web request to the `/cgi-bin/get_resol_device_information`
    /// endpoint and tries to parse the resulting information into a `DeviceInformation`
    /// instance.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> async_resol_vbus::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_resol_vbus::DeviceInformation;
    ///
    /// let address = "192.168.5.217:80".parse()?;
    /// let duration = std::time::Duration::from_millis(2000);
    /// let device = DeviceInformation::fetch(address, duration).await?;
    /// assert_eq!(address, device.address);
    /// #
    /// # Ok(()) }) }
    /// ```
    pub async fn fetch(addr: SocketAddr, timeout: Duration) -> Result<DeviceInformation> {
        let f = async {
            let mut stream = TcpStream::connect(addr).await?;

            let host = if addr.port() == 80 {
                format!("{}", addr.ip())
            } else {
                format!("{}:{}", addr.ip(), addr.port())
            };

            let request_string = format!("GET /cgi-bin/get_resol_device_information HTTP/1.0\r\nHost: {}\r\nUser-Agent: async-resol-vbus.rs\r\n\r\n", host);

            stream.write_all(request_string.as_bytes()).await?;

            stream.flush().await?;

            let mut buf = Vec::with_capacity(1024);
            let len = stream.read_to_end(&mut buf).await?;

            std::io::Result::Ok((buf, len))
        };

        // let (buf, len) = f.await?;
        let (buf, len) = async_std::io::timeout(timeout, f).await?;

        let buf = &buf[0..len];

        let body_idx = match DeviceInformation::find_http_body_idx(buf) {
            Some(idx) => idx,
            None => return Err("No HTTP header separator found".into()),
        };

        let body_bytes = &buf[body_idx..];
        let body = std::str::from_utf8(body_bytes)?;

        DeviceInformation::parse(addr, body)
    }

    /// Parse the information of a VBus-over-TCP device.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> async_resol_vbus::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_resol_vbus::DeviceInformation;
    ///
    /// let address = "192.168.5.217:80".parse()?;
    /// let string = "vendor = \"RESOL\"\nproduct = \"KM2\"\n...";
    /// let device = DeviceInformation::parse(address, string)?;
    /// assert_eq!(address, device.address);
    /// assert_eq!("RESOL", device.vendor.as_ref().unwrap());
    /// assert_eq!("KM2", device.product.as_ref().unwrap());
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn parse(address: SocketAddr, s: &str) -> Result<DeviceInformation> {
        #[derive(PartialEq)]
        enum Phase {
            InKey,
            WaitingForEquals,
            WaitingForValueStartQuote,
            InValue,
            AfterValueEndQuote,
            Malformed,
        }

        let mut vendor = None;
        let mut product = None;
        let mut serial = None;
        let mut version = None;
        let mut build = None;
        let mut name = None;
        let mut features = None;

        for line in s.lines() {
            let key_start_idx = 0;
            let mut key_end_idx = 0;
            let mut value_start_idx = 0;
            let mut value_end_idx = 0;
            let mut phase = Phase::InKey;

            for (idx, c) in line.char_indices() {
                let is_word_char = matches!(c, '0'..='9' | 'A'..='Z' | 'a'..='z' | '_');

                match phase {
                    Phase::InKey => {
                        if !is_word_char {
                            key_end_idx = idx;
                            phase = if c == '=' {
                                Phase::WaitingForValueStartQuote
                            } else {
                                Phase::WaitingForEquals
                            };
                        }
                    }
                    Phase::WaitingForEquals => {
                        if c == '=' {
                            phase = Phase::WaitingForValueStartQuote;
                        } else {
                            phase = Phase::Malformed;
                        }
                    }
                    Phase::WaitingForValueStartQuote => {
                        if c == '"' {
                            value_start_idx = idx + 1;
                            phase = Phase::InValue;
                        } else if is_word_char {
                            phase = Phase::Malformed;
                        }
                    }
                    Phase::InValue => {
                        if c == '"' {
                            value_end_idx = idx;
                            phase = Phase::AfterValueEndQuote;
                        }
                    }
                    Phase::AfterValueEndQuote => phase = Phase::Malformed,
                    Phase::Malformed => {}
                }
            }

            if phase == Phase::AfterValueEndQuote {
                let key = &line[key_start_idx..key_end_idx];
                let value = &line[value_start_idx..value_end_idx];

                if key.eq_ignore_ascii_case("vendor") {
                    vendor = Some(value.into());
                } else if key.eq_ignore_ascii_case("product") {
                    product = Some(value.into());
                } else if key.eq_ignore_ascii_case("serial") {
                    serial = Some(value.into());
                } else if key.eq_ignore_ascii_case("version") {
                    version = Some(value.into());
                } else if key.eq_ignore_ascii_case("build") {
                    build = Some(value.into());
                } else if key.eq_ignore_ascii_case("name") {
                    name = Some(value.into());
                } else if key.eq_ignore_ascii_case("features") {
                    features = Some(value.into());
                } else {
                    // unknown key...
                }
            }
        }

        Ok(DeviceInformation {
            address,
            vendor,
            product,
            serial,
            version,
            build,
            name,
            features,
        })
    }
}

#[cfg(test)]
mod tests {
    use async_std::net::{SocketAddr, TcpListener};

    use super::*;

    #[test]
    fn test() -> Result<()> {
        async_std::task::block_on(async {
            let web_addr = "127.0.0.1:0".parse::<SocketAddr>()?;
            let web_socket = TcpListener::bind(web_addr).await?;
            let web_addr = web_socket.local_addr()?;

            let web_future = async_std::task::spawn(async move {
                crate::test_utils::create_webserver(web_socket).await
            });

            let fetch_future = async_std::task::spawn::<_, Result<()>>(async move {
                let device = DeviceInformation::fetch(web_addr, Duration::from_millis(100)).await?;

                assert_eq!(Some("RESOL"), device.vendor.as_ref().map(|s| s.as_str()));
                assert_eq!(Some("DL2"), device.product.as_ref().map(|s| s.as_str()));
                assert_eq!(
                    Some("001E66xxxxxx"),
                    device.serial.as_ref().map(|s| s.as_str())
                );
                assert_eq!(Some("2.2.0"), device.version.as_ref().map(|s| s.as_str()));
                assert_eq!(Some("rc1"), device.build.as_ref().map(|s| s.as_str()));
                assert_eq!(
                    Some("DL2-001E66xxxxxx"),
                    device.name.as_ref().map(|s| s.as_str())
                );
                assert_eq!(
                    Some("vbus,dl2"),
                    device.features.as_ref().map(|s| s.as_str())
                );

                Ok(())
            });

            fetch_future.await?;
            drop(web_future);

            Ok(())
        })
    }
}
