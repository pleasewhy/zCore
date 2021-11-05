//! Device wrappers that implement `rcore_fs::dev::Device`, which can loaded
//! file systems on (e.g. `rcore_fs_sfs::SimpleFileSystem::open()`).

use alloc::sync::Arc;

extern crate rcore_fs;

use kernel_hal::drivers::scheme::BlockScheme;
use rcore_fs::dev::{BlockDevice, DevError, Device, Result};
use spin::RwLock;
use async_trait::async_trait;
use alloc::boxed::Box;

/// A naive LRU cache layer for `BlockDevice`, re-exported from `rcore-fs`.
pub use rcore_fs::dev::block_cache::BlockCache;

/// Memory buffer for device.
pub struct MemBuf(RwLock<&'static mut [u8]>);

impl MemBuf {
    /// create a [`MemBuf`] struct.
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

/// Block device implements [`BlockScheme`].
pub struct Block(Arc<dyn BlockScheme>);

impl Block {
    /// create a [`Block`] struct.
    pub fn new(block: Arc<dyn BlockScheme>) -> Self {
        Self(block)
    }
}

#[async_trait]
impl BlockDevice for Block {
    const BLOCK_SIZE_LOG2: u8 = 9; // 512

    async fn read_at(&self, block_id: usize, buf: &mut [u8]) -> Result<()> {
        // self.0.read_block(block_id, buf).await.map_err(|_| DevError)
        if let Err(e) = self.0.read_block(block_id, buf).await {
            error!("Read Device Error {:?}, block_id {}, buf.len {}", e, block_id, buf.len());
            return Err(DevError);
        }
        Ok(())
    }
    
    async fn write_at(&self, block_id: usize, buf: &[u8]) -> Result<()> {
        // self.0.write_block(block_id, buf).await.map_err(|_| DevError)
        if let Err(e) = self.0.write_block(block_id, buf).await {
            error!("Write Device Error {:?}, block_id {}, buf.len {}", e, block_id, buf.len());
            return Err(DevError);
        }
        Ok(())
    }

    async fn sync(&self) -> Result<()> {
        self.0.flush().await.map_err(|_| DevError)
    }
}