use std::{marker::Unpin, time::Duration};

use async_std::{
    io::{Read, Write},
    prelude::*,
};

use resol_vbus::{chrono::Utc, live_data_encoder, Data, Datagram, Header, LiveDataBuffer};

use crate::error::Result;

fn try_as_datagram(data: &Data) -> Option<&Datagram> {
    if data.is_datagram() {
        Some(data.as_datagram())
    } else {
        None
    }
}

/// A `Stream`/`Sink` wrapper for RESOL VBus `Data` items encoded in the
/// live / wire representation.
///
/// It also contains methods to communicate with a VBus device to get or set
/// values etc.
#[derive(Debug)]
pub struct LiveDataStream<R: Read + Unpin, W: Write + Unpin> {
    reader: R,
    writer: W,
    channel: u8,
    self_address: u16,
    buf: LiveDataBuffer,
}

impl<R: Read + Unpin, W: Write + Unpin> LiveDataStream<R, W> {
    /// Create a new `LiveDataStream`.
    pub fn new(reader: R, writer: W, channel: u8, self_address: u16) -> LiveDataStream<R, W> {
        LiveDataStream {
            reader,
            writer,
            channel,
            self_address,
            buf: LiveDataBuffer::new(channel),
        }
    }

    /// Consume `self` and return the underlying I/O pair.
    pub fn into_inner(self) -> (R, W) {
        (self.reader, self.writer)
    }

    fn create_datagram(
        &self,
        destination_address: u16,
        command: u16,
        param16: i16,
        param32: i32,
    ) -> Datagram {
        Datagram {
            header: Header {
                timestamp: Utc::now(),
                channel: self.channel,
                destination_address,
                source_address: self.self_address,
                protocol_version: 0x20,
            },
            command,
            param16,
            param32,
        }
    }

    async fn transceive_internal<F>(
        &mut self,
        tx_data: Option<Data>,
        max_tries: usize,
        initial_timeout_ms: u64,
        timeout_increment_ms: u64,
        filter: F,
    ) -> Result<Option<Data>>
    where
        F: Fn(&Data) -> bool,
    {
        let tx_data = match tx_data {
            Some(ref data) => {
                let len = live_data_encoder::length_from_data(data);
                let mut bytes = vec![0u8; len];
                live_data_encoder::bytes_from_data(data, &mut bytes);
                Some(bytes)
            }
            None => None,
        };

        let mut current_try = 0;
        let mut current_timeout_ms = initial_timeout_ms;

        let result = loop {
            if current_try >= max_tries {
                break None;
            }

            if let Some(ref tx_data) = tx_data {
                self.writer.write_all(tx_data).await?;
            }

            let result = async_std::io::timeout(Duration::from_millis(current_timeout_ms), async {
                loop {
                    let data = loop {
                        if let Some(data) = self.buf.read_data() {
                            if filter(&data) {
                                break Some(data);
                            }
                        } else {
                            break None;
                        }
                    };

                    if let Some(data) = data {
                        break Ok(Some(data));
                    }

                    let mut buf = [0u8; 256];
                    let len = self.reader.read(&mut buf).await?;
                    if len == 0 {
                        break Ok(None);
                    }

                    self.buf.extend_from_slice(&buf[0..len]);
                }
            })
            .await;

            if let Ok(data) = result {
                break data;
            }

            current_try += 1;
            current_timeout_ms += timeout_increment_ms;
        };

        Ok(result)
    }

    /// Receive data from the VBus.
    ///
    /// This methods waits for `timeout_ms` milliseconds for incoming
    /// VBus data. Every time a valid `Data` is received over the VBus
    /// the `filter` function is called with that `Data` as its argument.
    /// The function returns a `bool` whether the provided `Data` is the
    /// data it was waiting for.
    ///
    /// If the `filter` function returns `true`, the respective `Data`
    /// is used to return from the `receive` method.
    ///
    /// If the `filter` function did not find the matching data within
    /// `timeout_ms` milliseconds, the `receive` method returns with
    /// `None`.
    pub async fn receive<F>(&mut self, timeout_ms: u64, filter: F) -> Result<Option<Data>>
    where
        F: Fn(&Data) -> bool,
    {
        self.transceive_internal(None, 1, timeout_ms, 0, filter)
            .await
    }

    /// Send data to the VBus and wait for a reply.
    ///
    /// This method sends the `tx_data` to the VBus and waits for up to
    /// `initial_timeout_ms` milliseconds for a reply.
    ///
    /// Every time a valid `Data` is received over the VBus the `filter`
    /// function is called with that `Data` as its argument. The function
    /// returns a `bool` whether the provided `Data` is the reply it was
    /// waiting for.
    ///
    /// If the `filter` function returns `true`, the respective `Data`
    /// is used to return from the `transceive` method.
    ///
    /// If the `filter` function did not find the matching reply within
    /// `initial_timeout_ms` milliseconds, the `tx_data` is send again up
    /// `max_tries` times, increasing the timeout by `timeout_increment_ms`
    /// milliseconds every time.
    ///
    /// After `max_tries` without a matching reply the `transceive` method
    /// returns with `None`.
    pub async fn transceive<F>(
        &mut self,
        tx_data: Data,
        max_tries: usize,
        initial_timeout_ms: u64,
        timeout_increment_ms: u64,
        filter: F,
    ) -> Result<Option<Data>>
    where
        F: Fn(&Data) -> bool,
    {
        self.transceive_internal(
            Some(tx_data),
            max_tries,
            initial_timeout_ms,
            timeout_increment_ms,
            filter,
        )
        .await
    }

    /// Wait for any VBus data.
    pub async fn receive_any_data(&mut self, timeout_ms: u64) -> Result<Option<Data>> {
        self.receive(timeout_ms, |_| true).await
    }

    /// Wait for a datagram that offers VBus control.
    pub async fn wait_for_free_bus(&mut self) -> Result<Option<Datagram>> {
        let rx_data = self
            .receive(20000, |data| {
                if let Some(dgram) = try_as_datagram(data) {
                    dgram.command == 0x0500
                } else {
                    false
                }
            })
            .await?;

        Ok(rx_data.map(|data| data.into_datagram()))
    }

    /// Give back bus control to the regular VBus master.
    pub async fn release_bus(&mut self, address: u16) -> Result<Option<Data>> {
        let tx_dgram = self.create_datagram(address, 0x0600, 0, 0);

        let tx_data = Data::Datagram(tx_dgram);

        let rx_data = self
            .transceive(tx_data, 2, 2500, 2500, |data| data.is_packet())
            .await?;

        Ok(rx_data)
    }

    /// Get a value by its index.
    pub async fn get_value_by_index(
        &mut self,
        address: u16,
        index: i16,
        subindex: u8,
    ) -> Result<Option<Datagram>> {
        let tx_dgram = self.create_datagram(address, 0x0300 | u16::from(subindex), index, 0);

        let tx_data = Data::Datagram(tx_dgram.clone());

        let rx_data = self
            .transceive(tx_data, 3, 500, 500, |data| {
                if let Some(dgram) = try_as_datagram(data) {
                    dgram.header.source_address == tx_dgram.header.destination_address
                        && dgram.header.destination_address == tx_dgram.header.source_address
                        && dgram.command == (0x0100 | u16::from(subindex))
                        && dgram.param16 == tx_dgram.param16
                } else {
                    false
                }
            })
            .await?;

        Ok(rx_data.map(|data| data.into_datagram()))
    }

    /// Set a value by its index.
    pub async fn set_value_by_index(
        &mut self,
        address: u16,
        index: i16,
        subindex: u8,
        value: i32,
    ) -> Result<Option<Datagram>> {
        let tx_dgram = self.create_datagram(address, 0x0200 | u16::from(subindex), index, value);

        let tx_data = Data::Datagram(tx_dgram.clone());

        let rx_data = self
            .transceive(tx_data, 3, 500, 500, |data| {
                if let Some(dgram) = try_as_datagram(data) {
                    dgram.header.source_address == tx_dgram.header.destination_address
                        && dgram.header.destination_address == tx_dgram.header.source_address
                        && dgram.command == (0x0100 | u16::from(subindex))
                        && dgram.param16 == tx_dgram.param16
                } else {
                    false
                }
            })
            .await?;

        Ok(rx_data.map(|data| data.into_datagram()))
    }

    /// Get a value's ID hash by its index.
    pub async fn get_value_id_hash_by_index(
        &mut self,
        address: u16,
        index: i16,
    ) -> Result<Option<Datagram>> {
        let tx_dgram = self.create_datagram(address, 0x1000, index, 0);

        let tx_data = Data::Datagram(tx_dgram.clone());

        let rx_data = self
            .transceive(tx_data, 3, 500, 500, |data| {
                if let Some(dgram) = try_as_datagram(data) {
                    dgram.header.source_address == tx_dgram.header.destination_address
                        && dgram.header.destination_address == tx_dgram.header.source_address
                        && (dgram.command == 0x0100 || dgram.command == 0x1001)
                        && dgram.param16 == tx_dgram.param16
                } else {
                    false
                }
            })
            .await?;

        Ok(rx_data.map(|data| data.into_datagram()))
    }

    /// Get a value's index by its ID hash.
    pub async fn get_value_index_by_id_hash(
        &mut self,
        address: u16,
        id_hash: i32,
    ) -> Result<Option<Datagram>> {
        let tx_dgram = self.create_datagram(address, 0x1100, 0, id_hash);

        let tx_data = Data::Datagram(tx_dgram.clone());

        let rx_data = self
            .transceive(tx_data, 3, 500, 500, |data| {
                if let Some(dgram) = try_as_datagram(data) {
                    dgram.header.source_address == tx_dgram.header.destination_address
                        && dgram.header.destination_address == tx_dgram.header.source_address
                        && (dgram.command == 0x0100 || dgram.command == 0x1101)
                        && dgram.param32 == tx_dgram.param32
                } else {
                    false
                }
            })
            .await?;

        Ok(rx_data.map(|data| data.into_datagram()))
    }

    /// Get the capabilities (part 1) from a VBus device.
    pub async fn get_caps1(&mut self, address: u16) -> Result<Option<Datagram>> {
        let tx_dgram = self.create_datagram(address, 0x1300, 0, 0);

        let tx_data = Data::Datagram(tx_dgram.clone());

        let rx_data = self
            .transceive(tx_data, 3, 500, 500, |data| {
                if let Data::Datagram(ref dgram) = *data {
                    dgram.header.source_address == tx_dgram.header.destination_address
                        && dgram.header.destination_address == tx_dgram.header.source_address
                        && dgram.command == 0x1301
                } else {
                    false
                }
            })
            .await?;

        Ok(rx_data.map(|data| data.into_datagram()))
    }

    /// Begin a bulk value transaction.
    pub async fn begin_bulk_value_transaction(
        &mut self,
        address: u16,
        tx_timeout: i32,
    ) -> Result<Option<Datagram>> {
        let tx_dgram = self.create_datagram(address, 0x1400, 0, tx_timeout);

        let tx_data = Data::Datagram(tx_dgram.clone());

        let rx_data = self
            .transceive(tx_data, 3, 500, 500, |data| {
                if let Data::Datagram(ref dgram) = *data {
                    dgram.header.source_address == tx_dgram.header.destination_address
                        && dgram.header.destination_address == tx_dgram.header.source_address
                        && dgram.command == 0x1401
                } else {
                    false
                }
            })
            .await?;

        Ok(rx_data.map(|data| data.into_datagram()))
    }

    /// Commit a bulk value transaction.
    pub async fn commit_bulk_value_transaction(
        &mut self,
        address: u16,
    ) -> Result<Option<Datagram>> {
        let tx_dgram = self.create_datagram(address, 0x1402, 0, 0);

        let tx_data = Data::Datagram(tx_dgram.clone());

        let rx_data = self
            .transceive(tx_data, 3, 500, 500, |data| {
                if let Data::Datagram(ref dgram) = *data {
                    dgram.header.source_address == tx_dgram.header.destination_address
                        && dgram.header.destination_address == tx_dgram.header.source_address
                        && dgram.command == 0x1403
                } else {
                    false
                }
            })
            .await?;

        Ok(rx_data.map(|data| data.into_datagram()))
    }

    /// Rollback a bulk value transaction.
    pub async fn rollback_bulk_value_transaction(
        &mut self,
        address: u16,
    ) -> Result<Option<Datagram>> {
        let tx_dgram = self.create_datagram(address, 0x1404, 0, 0);

        let tx_data = Data::Datagram(tx_dgram.clone());

        let rx_data = self
            .transceive(tx_data, 3, 500, 500, |data| {
                if let Data::Datagram(ref dgram) = *data {
                    dgram.header.source_address == tx_dgram.header.destination_address
                        && dgram.header.destination_address == tx_dgram.header.source_address
                        && dgram.command == 0x1405
                } else {
                    false
                }
            })
            .await?;

        Ok(rx_data.map(|data| data.into_datagram()))
    }

    /// Set a value by its index while inside a bulk value transaction.
    pub async fn set_bulk_value_by_index(
        &mut self,
        address: u16,
        index: i16,
        subindex: u8,
        value: i32,
    ) -> Result<Option<Datagram>> {
        let tx_dgram = self.create_datagram(address, 0x1500 | u16::from(subindex), index, value);

        let tx_data = Data::Datagram(tx_dgram.clone());

        let rx_data = self
            .transceive(tx_data, 3, 500, 500, |data| {
                if let Data::Datagram(ref dgram) = *data {
                    dgram.header.source_address == tx_dgram.header.destination_address
                        && dgram.header.destination_address == tx_dgram.header.source_address
                        && dgram.command == (0x1600 | u16::from(subindex))
                        && dgram.param16 == tx_dgram.param16
                } else {
                    false
                }
            })
            .await?;

        Ok(rx_data.map(|data| data.into_datagram()))
    }
}

#[cfg(test)]
impl<R: Read + Unpin, W: Write + Unpin> LiveDataStream<R, W> {
    pub fn writer_ref(&self) -> &W {
        &self.writer
    }
}

#[cfg(test)]
mod tests {
    use async_std::io::Cursor;

    use resol_vbus::Packet;

    use super::*;

    fn extend_from_data(buf: &mut Vec<u8>, data: &Data) {
        let len = live_data_encoder::length_from_data(data);
        let idx = buf.len();
        buf.resize(idx + len, 0);
        live_data_encoder::bytes_from_data(data, &mut buf[idx..]);
    }

    fn extend_with_empty_packet(
        buf: &mut Vec<u8>,
        destination_address: u16,
        source_address: u16,
        command: u16,
    ) {
        let data = Data::Packet(Packet {
            header: Header {
                timestamp: Utc::now(),
                channel: 0,
                destination_address,
                source_address,
                protocol_version: 0x20,
            },
            command,
            frame_count: 0,
            frame_data: [0; 508],
        });
        extend_from_data(buf, &data);
    }

    fn extend_from_datagram(
        buf: &mut Vec<u8>,
        destination_address: u16,
        source_address: u16,
        command: u16,
        param16: i16,
        param32: i32,
    ) {
        let data = Data::Datagram(Datagram {
            header: Header {
                timestamp: Utc::now(),
                channel: 0,
                destination_address,
                source_address,
                protocol_version: 0x20,
            },
            command,
            param16,
            param32,
        });
        extend_from_data(buf, &data);
    }

    fn simulate_run<T, F: Future<Output = T>>(f: F) -> T {
        async_std::task::block_on(f)
    }

    trait ToBytes {
        fn to_bytes(&self) -> Vec<u8>;
    }

    fn hex_encode<T: ToBytes>(value: &T) -> String {
        let buf = value.to_bytes();
        buf.iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<String>>()
            .concat()
    }

    impl ToBytes for Cursor<Vec<u8>> {
        fn to_bytes(&self) -> Vec<u8> {
            self.get_ref().clone()
        }
    }

    impl ToBytes for Data {
        fn to_bytes(&self) -> Vec<u8> {
            let len = live_data_encoder::length_from_data(self);
            let mut buf = Vec::new();
            buf.resize(len, 0);
            live_data_encoder::bytes_from_data(self, &mut buf);
            buf
        }
    }

    impl ToBytes for Datagram {
        fn to_bytes(&self) -> Vec<u8> {
            Data::Datagram(self.clone()).to_bytes()
        }
    }

    #[test]
    fn test_wait_for_free_bus() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0000, 0x7E11, 0x0500, 0, 0);

        let mut lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let data = simulate_run(lds.wait_for_free_bus()).unwrap();

        assert_eq!("", hex_encode(lds.writer_ref()));
        assert_eq!(
            "aa0000117e200005000000000000004b",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_release_bus() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0100, 0, 0);
        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);

        let mut lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let data = simulate_run(lds.release_bus(0x7E11)).unwrap();

        assert_eq!(
            "aa117e2000200006000000000000002a",
            hex_encode(lds.writer_ref())
        );
        assert_eq!("aa1000117e100001004f", hex_encode(&data.unwrap()));
    }

    #[test]
    fn test_get_value_by_index() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x0156, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x0156, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0157, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0156, 0x1235, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0156, 0x1234, 0x789abcde);

        let mut lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let data = simulate_run(lds.get_value_by_index(0x7E11, 0x1234, 0x56)).unwrap();

        assert_eq!(
            "aa117e20002056033412000000000011",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e20560134125e3c1a781c4b",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_set_value_by_index() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x0156, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x0156, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0157, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0156, 0x1235, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0156, 0x1234, 0x789abcde);

        let mut lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let data = simulate_run(lds.set_value_by_index(0x7E11, 0x1234, 0x56, 0x789abcde)).unwrap();

        assert_eq!(
            "aa117e200020560234125e3c1a781c4a",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e20560134125e3c1a781c4b",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_get_value_id_hash_by_index() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x0100, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x0100, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0101, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0100, 0x1235, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0100, 0x1234, 0x789abcde);

        let mut lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let data = simulate_run(lds.get_value_id_hash_by_index(0x7E11, 0x1234)).unwrap();

        assert_eq!(
            "aa117e2000200010341200000000005a",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e20000134125e3c1a781c21",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_get_value_index_by_id_hash() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x0100, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x0100, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0101, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0100, 0x1234, 0x789abcdf);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x0100, 0x1234, 0x789abcde);

        let mut lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let data = simulate_run(lds.get_value_index_by_id_hash(0x7E11, 0x789abcde)).unwrap();

        assert_eq!(
            "aa117e200020001100005e3c1a781c57",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e20000134125e3c1a781c21",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_get_caps1() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x1301, 0, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x1301, 0, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1300, 0, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1301, 0, 0x789abcde);

        let mut lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let data = simulate_run(lds.get_caps1(0x7E11)).unwrap();

        assert_eq!(
            "aa117e2000200013000000000000001d",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e20011300005e3c1a781c54",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_begin_bulk_value_transaction() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x1401, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x1401, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1400, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1401, 0, 0);

        let mut lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let data = simulate_run(lds.begin_bulk_value_transaction(0x7E11, 0x789abcde)).unwrap();

        assert_eq!(
            "aa117e200020001400005e3c1a781c54",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e200114000000000000001b",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_commit_value_transaction() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x1403, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x1403, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1402, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1403, 0, 0);

        let mut lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let data = simulate_run(lds.commit_bulk_value_transaction(0x7E11)).unwrap();

        assert_eq!(
            "aa117e2000200214000000000000001a",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e2003140000000000000019",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_rollback_value_transaction() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x1405, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x1405, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1404, 0, 0);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1405, 0, 0);

        let mut lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let data = simulate_run(lds.rollback_bulk_value_transaction(0x7E11)).unwrap();

        assert_eq!(
            "aa117e20002004140000000000000018",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e2005140000000000000017",
            hex_encode(&data.unwrap())
        );
    }

    #[test]
    fn test_set_bulk_value_by_index() {
        let mut rx_buf = Vec::new();
        let tx_buf = Cursor::new(Vec::new());

        extend_with_empty_packet(&mut rx_buf, 0x0010, 0x7E11, 0x0100);
        extend_from_datagram(&mut rx_buf, 0x0021, 0x7E11, 0x1656, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E10, 0x1656, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1657, 0x1234, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1656, 0x1235, 0x789abcde);
        extend_from_datagram(&mut rx_buf, 0x0020, 0x7E11, 0x1656, 0x1234, 0x789abcde);

        let mut lds = LiveDataStream::new(&rx_buf[..], tx_buf, 0, 0x0020);

        let data =
            simulate_run(lds.set_bulk_value_by_index(0x7E11, 0x1234, 0x56, 0x789abcde)).unwrap();

        assert_eq!(
            "aa117e200020561534125e3c1a781c37",
            hex_encode(lds.writer_ref())
        );
        assert_eq!(
            "aa2000117e20561634125e3c1a781c36",
            hex_encode(&data.unwrap())
        );
    }
}
