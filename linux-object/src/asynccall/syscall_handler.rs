
use async_trait::async_trait;
use zircon_object::task::Thread;
use alloc::{boxed::Box, sync::Arc};

/// 
#[async_trait]
pub trait GeneralSyscallHandler {
    /// 
    async fn handle_trivial_syscall(&self, thread: &Arc<Thread>, num: u32, args: [usize; 6]) -> isize;
}