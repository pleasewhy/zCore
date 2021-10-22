//! Implement Device

use rcore_fs::dev::{Device, Result};
use spin::RwLock;

use alloc::boxed::Box;
use async_trait::async_trait;

/// memory buffer for device
pub struct MemBuf(RwLock<&'static mut [u8]>);

impl MemBuf {
    /// create a MemBuf struct
    pub fn new(buf: &'static mut [u8]) -> Self {
        MemBuf(RwLock::new(buf))
    }
}

#[async_trait]
impl Device for MemBuf {
    async fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
        let slice = self.0.read();
        let len = buf.len().min(slice.len() - offset);
        buf[..len].copy_from_slice(&slice[offset..offset + len]);
        Ok(len)
    }
    async fn write_at(&self, offset: usize, buf: &[u8]) -> Result<usize> {
        let mut slice = self.0.write();
        let len = buf.len().min(slice.len() - offset);
        slice[offset..offset + len].copy_from_slice(&buf[..len]);
        Ok(len)
    }
    async fn sync(&self) -> Result<()> {
        Ok(())
    }
}
