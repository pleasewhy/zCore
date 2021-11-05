use alloc::boxed::Box;
use alloc::sync::Arc;
use zircon_object::{
    object::KernelObject,
    task::{Task, ThreadState},
    vm::MMUFlags,
};

use crate::process::*;

use zircon_object::task::{Thread, ThreadSwitchFuture};
use zircon_object::vm::*;

use kernel_hal::mem::phys_to_virt;
use kernel_hal::thread::yield_now;
use core::convert::TryFrom;
use crate::error::{LxError, LxResult};

mod structs;
mod syscall_handler;

use structs::{CompletionRingEntry, RequestRingEntry};

pub use structs::{AsyncCallBuffer, AsyncCallInfoUser};
pub use syscall_handler::GeneralSyscallHandler;

///
pub type SyscallHandler = Box<dyn GeneralSyscallHandler + Send + Sync>;

///
pub struct AsyncCall {
    thread: Arc<Thread>,
    handler: SyscallHandler,
}

type AsyncCallResult = LxResult<usize>;

/// async buffer descriptor wrapper
pub type BufferDesc = u64;

///
pub const MAX_ASYNC_CALL_ENTRY_NUM: usize = 32768;

impl AsyncCall {
    ///
    pub fn new(thread: &Arc<Thread>, handler: SyscallHandler) -> Self {
        Self {
            thread: thread.clone(),
            handler,
        }
    }

    ///
    pub fn setup(
        thread: &Arc<Thread>,
        handler: SyscallHandler,
        req_capacity: usize,
        comp_capacity: usize,
        worker_num: usize,
    ) -> LxResult<(AsyncCallBuffer, AsyncCallInfoUser)> {
        let buf = AsyncCallBuffer::new(req_capacity, comp_capacity)?;
        let vmar = thread.proc().vmar();
        let flags = MMUFlags::READ | MMUFlags::WRITE | MMUFlags::USER;
        let vmo = buf.vmo_clone();
        let len = vmo.len();
        let user_buf_addr = vmar.map(None, vmo, 0, len, flags)?;
        let info = buf.fill_user_info(user_buf_addr);
        spawn_polling(thread, handler, worker_num);
        Ok((buf, info))
    }

    async fn do_async_call(&self, req: &RequestRingEntry) -> AsyncCallResult {
        if self.thread.state() == ThreadState::Dead {
            return Err(LxError::EPERM);
        }
        let args = [req.arg0 as usize, req.arg1 as usize, req.arg2 as usize, 
                    req.arg3 as usize, req.arg4 as usize, req.arg5 as usize];
        let ret = self.handler.handle_trivial_syscall(&self.thread, req.syscall_id as u32, args).await;
        if ret < 0 {
            warn!("AsyncCall: {:?} <= {:?}", req.syscall_id, ret);
            Err(LxError::try_from(ret).unwrap_or(LxError::EINVAL))
        } else {
            info!("AsyncCall: {:?} <= {:?}", req.syscall_id, ret);
            Ok(ret as usize)
        }
    }

    async fn polling_once(&self) -> LxResult {
        let proc = self.thread.proc().linux();
        let bufs = proc.async_bufs.lock();
        let buf = match bufs.get(&self.thread.id()) {
            Some(b) => b,
            None => return Err(LxError::ENFILE),
        };
        debug!("thread {}: {:#x?}", self.thread.id(), buf.as_raw());

        let mut cached_req_head = buf.read_req_ring_head();
        let mut cached_comp_tail = buf.read_comp_ring_tail();
        let req_count = buf.request_count(cached_req_head)?;
        // TODO: limit requests count or time for one thread
        for _ in 0..req_count {
            if self.thread.state() == ThreadState::Dying {
                break;
            }
            let req_entry = buf.req_entry_at(cached_req_head);
            let res = self.do_async_call(&req_entry).await;
            while buf.completion_count(cached_comp_tail)? == buf.comp_capacity {
                yield_now().await;
            }
            *buf.comp_entry_at(cached_comp_tail) =
                CompletionRingEntry::new(req_entry.user_data, res);
            cached_comp_tail += 1;
            buf.write_comp_ring_tail(cached_comp_tail);
            cached_req_head += 1;
        }
        buf.write_req_ring_head(cached_req_head);
        Ok(())
    }

    async fn polling(&self) {
        info!(
            "start async call polling for thread {}...",
            self.thread.proc().id()
        );
        while !(self.thread.state() == ThreadState::Dead) {
            let res = self.polling_once().await;
            if let Err(_e) = res {
                self.thread.kill();
                break;
            }
            kernel_hal::thread::yield_now().await;
        }
        info!(
            "async call polling for thread {} is done.",
            self.thread.proc().id()
        );
    }
}

fn spawn_polling(thread: &Arc<Thread>, handler: SyscallHandler, _worker_num: usize) {
    let ac = AsyncCall::new(thread, handler);
    kernel_hal::thread::spawn(
        ThreadSwitchFuture::new(
            thread.clone(),
            Box::pin(async move { ac.polling().await })
        )
    );
}
