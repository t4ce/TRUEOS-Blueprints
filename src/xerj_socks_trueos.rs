use std::{io, net::UdpSocket};

fn buffer(len: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            "SOCKS datagram allocation failed",
        )
    })?;
    Ok(bytes)
}

impl super::WritevExt for UdpSocket {
    fn writev(&self, bufs: [&[u8]; 2]) -> io::Result<usize> {
        let len = bufs[0].len().checked_add(bufs[1].len()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "SOCKS datagram size overflow")
        })?;
        let mut bytes = buffer(len)?;
        bytes.extend_from_slice(bufs[0]);
        bytes.extend_from_slice(bufs[1]);
        self.send(&bytes)
    }
    fn readv(&self, bufs: [&mut [u8]; 2]) -> io::Result<usize> {
        let len = bufs[0].len().checked_add(bufs[1].len()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "SOCKS datagram size overflow")
        })?;
        let mut bytes = buffer(len)?;
        bytes.resize(len, 0);
        let read = self.recv(&mut bytes)?;
        let prefix = read.min(bufs[0].len());
        bufs[0][..prefix].copy_from_slice(&bytes[..prefix]);
        bufs[1][..read - prefix].copy_from_slice(&bytes[prefix..read]);
        Ok(read)
    }
}
