use std::net::UdpSocket;
use std::sync::Arc;
use std::time::Duration;
use log::{info, warn, error, debug};

pub const LORA_DEFAULT_PORT: u16 = 1780;
pub const LORA_MAX_PACKET_SIZE: usize = 512;
pub const LORA_RECV_TIMEOUT_SECS: u64 = 5;

pub struct ManagedUdpSocket {
    socket: Arc<UdpSocket>,
    bound_addr: String,
}

impl ManagedUdpSocket {
    pub fn bind(addr: &str) -> Result<Self, std::io::Error> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_read_timeout(Some(Duration::from_secs(LORA_RECV_TIMEOUT_SECS)))?;
        socket.set_write_timeout(Some(Duration::from_secs(3)))?;
        let bound_addr = socket.local_addr()?.to_string();
        info!("LoRa UDP套接字已绑定: {}", bound_addr);
        Ok(Self {
            socket: Arc::new(socket),
            bound_addr,
        })
    }

    pub fn send_to(&self, data: &[u8], addr: &str) -> Result<usize, std::io::Error> {
        let n = self.socket.send_to(data, addr)?;
        debug!("UDP发送 {} 字节至 {}", n, addr);
        Ok(n)
    }

    pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, String), std::io::Error> {
        let (n, src) = self.socket.recv_from(buf)?;
        debug!("UDP接收 {} 字节自 {}", n, src);
        Ok((n, src.to_string()))
    }

    pub fn local_addr(&self) -> &str {
        &self.bound_addr
    }

    pub fn set_broadcast(&self, on: bool) -> Result<(), std::io::Error> {
        self.socket.set_broadcast(on)
    }

    pub fn set_multicast_ttl(&self, ttl: u32) -> Result<(), std::io::Error> {
        self.socket.set_multicast_ttl_v4(ttl)
    }

    pub fn join_multicast(&self, multi_addr: &str, iface: &str) -> Result<(), std::io::Error> {
        use std::net::{Ipv4Addr, SocketAddrV4};
        let multi: Ipv4Addr = multi_addr.parse().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, e)
        })?;
        let iface_ip: Ipv4Addr = iface.parse().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, e)
        })?;
        self.socket.join_multicast_v4(&multi, &iface_ip)
    }

    pub fn clone_socket(&self) -> Arc<UdpSocket> {
        Arc::clone(&self.socket)
    }
}

impl Drop for ManagedUdpSocket {
    fn drop(&mut self) {
        info!("LoRa UDP套接字释放: {}", self.bound_addr);
    }
}

pub struct LoraUdpListener {
    socket: ManagedUdpSocket,
    buffer: Vec<u8>,
}

impl LoraUdpListener {
    pub fn new(port: u16) -> Result<Self, std::io::Error> {
        let addr = format!("0.0.0.0:{}", port);
        let socket = ManagedUdpSocket::bind(&addr)?;
        Ok(Self {
            socket,
            buffer: vec![0u8; LORA_MAX_PACKET_SIZE],
        })
    }

    pub fn recv_packet(&mut self) -> Result<(Vec<u8>, String), std::io::Error> {
        let (n, src) = self.socket.recv_from(&mut self.buffer)?;
        let data = self.buffer[..n].to_vec();
        Ok((data, src))
    }

    pub fn send_response(&self, data: &[u8], addr: &str) -> Result<usize, std::io::Error> {
        self.socket.send_to(data, addr)
    }

    pub fn local_addr(&self) -> &str {
        self.socket.local_addr()
    }
}

pub struct LoraUdpSender {
    socket: ManagedUdpSocket,
}

impl LoraUdpSender {
    pub fn new() -> Result<Self, std::io::Error> {
        let socket = ManagedUdpSocket::bind("0.0.0.0:0")?;
        Ok(Self { socket })
    }

    pub fn send(&self, data: &[u8], dest: &str) -> Result<usize, std::io::Error> {
        self.socket.send_to(data, dest)
    }
}

impl Default for LoraUdpSender {
    fn default() -> Self {
        Self::new().expect("Failed to create LoraUdpSender")
    }
}

pub struct UdpSocketGuard {
    inner: Option<ManagedUdpSocket>,
}

impl UdpSocketGuard {
    pub fn new(socket: ManagedUdpSocket) -> Self {
        Self { inner: Some(socket) }
    }

    pub fn as_ref(&self) -> Option<&ManagedUdpSocket> {
        self.inner.as_ref()
    }

    pub fn take(&mut self) -> Option<ManagedUdpSocket> {
        self.inner.take()
    }

    pub fn is_alive(&self) -> bool {
        self.inner.is_some()
    }

    pub fn close(&mut self) {
        if let Some(s) = self.inner.take() {
            info!("主动关闭LoRa UDP套接字: {}", s.local_addr());
            drop(s);
        }
    }
}

impl Drop for UdpSocketGuard {
    fn drop(&mut self) {
        if self.inner.is_some() {
            warn!("UdpSocketGuard析构：确保套接字释放，防止泄露");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_managed_socket_bind() {
        let sock = ManagedUdpSocket::bind("0.0.0.0:0");
        assert!(sock.is_ok(), "应能绑定到任意端口");
        let s = sock.unwrap();
        assert!(!s.local_addr().is_empty());
    }

    #[test]
    fn test_guard_close() {
        let sock = ManagedUdpSocket::bind("0.0.0.0:0").unwrap();
        let mut guard = UdpSocketGuard::new(sock);
        assert!(guard.is_alive());
        guard.close();
        assert!(!guard.is_alive());
    }

    #[test]
    fn test_sender_create() {
        let sender = LoraUdpSender::new();
        assert!(sender.is_ok());
    }
}
