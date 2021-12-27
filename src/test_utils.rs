use async_std::{net::TcpListener, prelude::*};

use crate::{DeviceInformation, Result};

pub(crate) async fn create_webserver(web_socket: TcpListener) -> Result<()> {
    loop {
        let (mut stream, _) = web_socket.accept().await?;

        let mut buf = Vec::new();
        buf.resize(1024, 0x00);

        let mut len = 0;
        loop {
            let chunk_len = stream.read(&mut buf[len..]).await?;
            if chunk_len == 0 {
                return Err("EOF before HTTP header".into());
            }

            len += chunk_len;

            if DeviceInformation::find_http_body_idx(&buf[0..len]).is_some() {
                break;
            }
        }

        let response = b"HTTP/1.0 200 OK\r\n\r\nvendor = \"RESOL\"\r\nproduct = \"DL2\"\r\nserial = \"001E66xxxxxx\"\r\nversion = \"2.2.0\"\r\nbuild = \"rc1\"\r\nname = \"DL2-001E66xxxxxx\"\r\nfeatures = \"vbus,dl2\"\r\n";
        stream.write_all(response).await?;

        stream.flush().await?;

        drop(stream);
    }
}
