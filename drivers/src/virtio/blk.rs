use alloc::sync::Arc;
use virtio_drivers::{VirtIOBlk as InnerDriver, VirtIOHeader};

use crate::scheme::{BlockScheme, Scheme};
use crate::DeviceResult;

use async_trait::async_trait;
use alloc::boxed::Box;

pub struct VirtIoBlk<'a> {
    inner: Arc<InnerDriver<'a>>,
}

impl<'a> VirtIoBlk<'a> {
    pub fn new(header: &'static mut VirtIOHeader) -> DeviceResult<Self> {
        Ok(Self {
            inner: Arc::new(InnerDriver::new(header)?),
        })
    }
}

impl<'a> Scheme for VirtIoBlk<'a> {
    fn name(&self) -> &str {
        "virtio-blk"
    }

    fn handle_irq(&self, _irq_num: usize) {
        self.inner.handle_irq().unwrap();
        assert_eq!(self.inner.ack_interrupt(), true);
    }
}

#[async_trait]
impl<'a> BlockScheme for VirtIoBlk<'a> {
    async fn read_block(&self, block_id: usize, buf: &mut [u8]) -> DeviceResult {
        self.inner.read_block(block_id, buf).await?;
        Ok(())
    }

    async fn write_block(&self, block_id: usize, buf: &[u8]) -> DeviceResult {
        self.inner.write_block(block_id, buf).await?;
        Ok(())
    }

    async fn flush(&self) -> DeviceResult {
        Ok(())
    }
}
