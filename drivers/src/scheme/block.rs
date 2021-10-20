use super::Scheme;
use crate::DeviceResult;

use async_trait::async_trait;
use alloc::boxed::Box;

#[async_trait]
pub trait BlockScheme: Scheme {
    async fn read_block(&self, block_id: usize, buf: &mut [u8]) -> DeviceResult;
    async fn write_block(&self, block_id: usize, buf: &[u8]) -> DeviceResult;
    async fn flush(&self) -> DeviceResult;
}
