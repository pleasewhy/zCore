use super::*;
use async_trait::async_trait;
use zircon_object::task::CurrentThread;
use alloc::boxed::Box;
use core::{pin::Pin, future::Future};
use linux_object::asynccall::AsyncCallInfoUser;

fn is_trivial_syscall(syscall_id: usize) -> bool {
    static TRIVIAL_SYSCALLS: [Sys; 7] = 
        [Sys::CLONE, Sys::EXECVE, Sys::EXIT, Sys::EXIT_GROUP, Sys::SCHED_YIELD, Sys::WAIT4, Sys::NANOSLEEP];
    TRIVIAL_SYSCALLS.iter().find(|&&x| x as usize == syscall_id).is_none()
}

struct TrivialSyscallHandler;

#[async_trait]
impl linux_object::asynccall::GeneralSyscallHandler for TrivialSyscallHandler {
    async fn handle_trivial_syscall(&self, thread: &Arc<Thread>, num: u32, args: [usize; 6]) -> isize {
        if is_trivial_syscall(num as usize) {
            fn null(_thread: CurrentThread) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
                Box::pin( async {()})
            }
            let mut syscall = Syscall {
                thread: &CurrentThread(thread.clone()),
                syscall_entry: 0,
                thread_fn: null,
            };
            syscall.syscall(num, args).await
        } else {
            -(LxError::EINVAL as isize)
        }
    }
}

impl Syscall<'_> {
    /// Setup asynccall buffer
    pub fn sys_setup_async_call(
        &self,
        req_capacity: usize,
        comp_capacity: usize,
        worker_num: usize,
        mut out_info: UserOutPtr<AsyncCallInfoUser>,
        info_size: usize,
    ) -> SysResult {
        if info_size != core::mem::size_of::<AsyncCallInfoUser>() {
            error!("sizeof AsyncCallInfoUser = {} info_size = {}", 
                core::mem::size_of::<AsyncCallInfoUser>(), info_size);
            return Err(LxError::EINVAL);
        }
        info!("setup_asynccall req = {:?}, comp = {:?}, worker_num = {:?} info = {:?} info_size = {:?}", 
            req_capacity, comp_capacity, worker_num, out_info, info_size);
        let proc = self.thread.proc().linux();
        let info = proc.set_up_async_buf(self.thread.clone(), Box::new(TrivialSyscallHandler), req_capacity, comp_capacity, worker_num)?;
        out_info.write(info)?;
        Ok(0)
    }
}
