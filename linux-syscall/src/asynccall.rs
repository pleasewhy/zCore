use super::*;
use async_trait::async_trait;
use zircon_object::task::CurrentThread;
use alloc::boxed::Box;
use core::{pin::Pin, future::Future};

fn is_trivial_syscall(syscall_id: usize) -> bool {
    static TRIVIAL_SYSCALLS: [Sys; 7] = 
        [Sys::CLONE, Sys::EXECVE, Sys::EXIT, Sys::EXIT_GROUP, Sys::SCHED_YIELD, Sys::WAIT4, Sys::NANOSLEEP];
    TRIVIAL_SYSCALLS.iter().find(|&&x| x as usize == syscall_id).is_some()
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
                #[cfg(not(target_arch = "riscv64"))]
                regs: &mut GeneralRegs::default(),
                #[cfg(target_arch = "riscv64")]
                context: &mut UserContext::default(),
                thread_fn: null,
            };
            syscall.syscall(num, args).await
        } else {
            -(LxError::EINVAL as isize)
        }
    }
}

impl Syscall<'_> {
    /// 
    pub fn sys_setup_async_call(
        &self,
        req_capacity: usize,
        comp_capacity: usize,
        worker_num: usize,
    ) -> SysResult {
        let proc = self.thread.proc().linux();
        proc.set_up_async_buf(self.thread.clone(), Box::new(TrivialSyscallHandler), req_capacity, comp_capacity, worker_num)?;
        Ok(0)
    }
}
