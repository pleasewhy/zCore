use alloc::boxed::Box;
use alloc::sync::Arc;
use zircon_object::{
    object::KernelObject,
    task::{ThreadState},
    vm::MMUFlags,
};

use core::task::{Context, Poll, Waker};
use core::future::Future;
use core::pin::Pin;
use futures::pin_mut;

use core::ops::Range;

use alloc::vec::Vec;
use alloc::collections::vec_deque::VecDeque;

use zircon_object::task::{Thread, ThreadSwitchFuture};
use zircon_object::vm::*;

use kernel_hal::mem::phys_to_virt;
use core::convert::TryFrom;
use crate::error::{LxError, LxResult};

use spin::Mutex;

use structs::{CompletionRingEntry, /*RequestRingEntry*/};

pub use structs::{AsyncCallBuffer, AsyncCallInfoUser};
pub use syscall_handler::GeneralSyscallHandler;

mod structs;
mod syscall_handler;

///
pub type SyscallHandler = Box<dyn GeneralSyscallHandler + Send + Sync>;

///
pub struct AsyncCall {
    thread: Arc<Thread>,
    handler: SyscallHandler,
    request_entries: usize,
    inner: Mutex<AsyncCallInner>,
}

/// 
pub struct AsyncCallInner {
    request_not_ready: Vec<Option<Waker>>,
    comp_buffer_full: VecDeque<Waker>,
}

type AsyncCallResult = LxResult<usize>;

/// async buffer descriptor wrapper
pub type BufferDesc = u64;

///
pub const MAX_ASYNC_CALL_ENTRY_NUM: usize = 32768;

numeric_enum_macro::numeric_enum! {
    #[repr(u32)]
    /// The state of asynccall requeset
    #[derive(Debug)]
    pub enum AsyncCallState {
        /// Invalid state
        Null = 0,
        /// A request is submited by user
        Init = 1,
        /// A request is being handled by kernel
        Working = 2,
        /// A request is done and the request should be put into comp ring
        Done = 3,
    }
}

numeric_enum_macro::numeric_enum! {
    #[repr(usize)]
    /// The reason of pending worker coroutine
    #[derive(Debug)]
    pub enum PendingReason {
        /// The state of request entry is not `Init`
        RequestNotReady = 0,
        /// Syscall is pending
        SyscallPending = 1,
        /// Complete ring is full
        CompBufferFull = 2,
        /// Unknown reason
        Invalid = 3,
    }
}


impl AsyncCall {
    ///
    pub fn new(thread: Arc<Thread>, request_entries: usize, handler: SyscallHandler) -> Arc<Self> {
        Arc::new(Self {
            thread: thread,
            handler,
            request_entries,
            inner: Mutex::new(AsyncCallInner {
                request_not_ready: {
                    let mut vec = Vec::with_capacity(request_entries);
                    vec.resize_with(request_entries, Default::default);
                    vec
                },
                comp_buffer_full: VecDeque::with_capacity(request_entries),
            }),
        })
    }

    ///
    pub fn setup(
        thread: &Arc<Thread>,
        handler: SyscallHandler,
        req_capacity: usize,
        comp_capacity: usize,
        _worker_num: usize,
    ) -> LxResult<(Arc<AsyncCallBuffer>, AsyncCallInfoUser)> {
        let buf = AsyncCallBuffer::new(req_capacity, comp_capacity)?;
        let vmar = thread.proc().vmar();
        let flags = MMUFlags::READ | MMUFlags::WRITE | MMUFlags::USER;
        let vmo = buf.vmo_clone();
        let len = vmo.len();
        let user_buf_addr = vmar.map(None, vmo, 0, len, flags)?;
        let info = buf.fill_user_info(user_buf_addr);
        spawn_polling(thread, &buf, handler, req_capacity);
        Ok((buf, info))
    }

    async fn do_async_call(&self, syscall_id: usize, args: [usize; 6]) -> AsyncCallResult {
        if self.thread.state() == ThreadState::Dead {
            warn!("AsyncCall: thread has dead");
            return Err(LxError::EPERM);
        }
        let ret = self.handler.handle_trivial_syscall(&self.thread, syscall_id as u32, args).await;
        if ret < 0 {
            warn!("AsyncCall: {:?} <= {:?}", syscall_id, ret);
            Err(LxError::try_from(ret).unwrap_or(LxError::EINVAL))
        } else {
            info!("AsyncCall: {:?} <= {:?}", syscall_id, ret);
            Ok(ret as usize)
        }
    }

    fn wake_worker_wait_on_req_range(&self, range: Range::<u32>) {
        let start = range.start as usize % self.request_entries;
        let end = range.end as usize % self.request_entries;
        let mut inner = self.inner.lock();
        let waker_fn = |waker: &mut Option<Waker>| {
            if let Some(waker) = waker.take() {
                waker.wake();
            }
        };
        if start == end {
            inner.request_not_ready.iter_mut().for_each(waker_fn);
        } else if start < end {
            inner.request_not_ready[start..end].iter_mut().for_each(waker_fn);
        } else {
            inner.request_not_ready[start..].iter_mut().for_each(waker_fn);
            inner.request_not_ready[..end].iter_mut().for_each(waker_fn);
        }
    }

    fn wake_worker_wait_on_comp_count(&self, count: usize) {
        let mut inner = self.inner.lock();
        let count = count.min(inner.comp_buffer_full.len());
        if count > 0 {
            inner.comp_buffer_full.drain(..count).for_each(|waker| waker.wake());
        }
    }

    // async fn polling_inner(&self) -> LxResult {
    //     let proc = self.thread.proc().linux();
    //     let bufs = proc.async_bufs.lock();
    //     let buf = match bufs.get(&self.thread.id()) {
    //         Some(b) => b,
    //         None => return Err(LxError::ENFILE),
    //     };
    //     debug!("thread {}: {:#x?}", self.thread.id(), buf.as_raw());

    //     let mut cached_req_head = buf.read_req_ring_head();
    //     let mut cached_comp_tail = buf.read_comp_ring_tail();
    //     let req_count = buf.request_count(cached_req_head)?;
    //     // TODO: limit requests count or time for one thread
    //     for _ in 0..req_count {
    //         if self.thread.state() == ThreadState::Dying {
    //             break;
    //         }
    //         let req_entry = buf.req_entry_at(cached_req_head);
    //         let res = self.do_async_call(&req_entry).await;
    //         while buf.completion_count(cached_comp_tail)? == buf.comp_capacity {
    //             yield_now().await;
    //         }
    //         *buf.comp_entry_at(cached_comp_tail) =
    //             CompletionRingEntry::new(req_entry.user_data, res);
    //         cached_comp_tail += 1;
    //         buf.write_comp_ring_tail(cached_comp_tail);
    //         cached_req_head += 1;
    //     }
    //     buf.write_req_ring_head(cached_req_head);
    //     Ok(())
    // }

    // #[allow(dead_code)]
    // async fn polling(&self) {
    //     info!(
    //         "start async call polling for thread {}...",
    //         self.thread.proc().id()
    //     );
    //     while !(self.thread.state() == ThreadState::Dead) {
    //         let res = self.polling_inner().await;
    //         if let Err(_e) = res {
    //             warn!("Something was wrong with asynccall, kill this thread");
    //             self.thread.kill();
    //             break;
    //         }
    //         kernel_hal::thread::yield_now().await;
    //     }
    //     info!(
    //         "async call polling for thread {} is done.",
    //         self.thread.proc().id()
    //     );
    // }

    fn pending(&self, id: usize, waker: Waker, reason: PendingReason) {
        let mut inner = self.inner.lock();
        match reason {
            PendingReason::RequestNotReady => {
                inner.request_not_ready[id] = Some(waker);
            },
            PendingReason::CompBufferFull => {
                inner.comp_buffer_full.push_back(waker);
            },
            _ => panic!("Unknown pending reason"),
        }
    }
}

fn spawn_polling(thread: &Arc<Thread>, buf: &Arc<AsyncCallBuffer>, 
        handler: SyscallHandler, request_entriess: usize) 
{
    let ac = AsyncCall::new(thread.clone(), request_entriess, handler);
    for idx in 0..request_entriess {
        let ac = ac.clone();
        let buf = buf.clone();
        kernel_hal::thread::spawn(
            ThreadSwitchFuture::new(
                thread.clone(),
                Box::pin(async move { 
                    AsynccallFuture::polling(idx, ac, buf).await
                }),
            )
        );
    }
    kernel_hal::thread::spawn(Box::pin(ReqCheckerFuture::new(ac.clone(), buf.clone())));
    kernel_hal::thread::spawn(Box::pin(CompCheckerFuture::new(ac.clone(), buf.clone())));
}

#[derive(Default)]
struct YieldFuture {
    flag: bool,
}

impl Future for YieldFuture {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        if self.flag == false {
            self.flag = true;
            cx.waker().clone().wake();
            return Poll::Pending;
        }
        Poll::Ready(())
    }
}

struct AsynccallFuture;

struct ReqRingFuture {
    id: usize,
    base: Arc<AsyncCall>,
    buf: Arc<AsyncCallBuffer>,
}

impl Future for ReqRingFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let entry = self.buf.req_entry_at(self.id as u32);
        if unsafe { core::intrinsics::atomic_load_acq(&entry.flags) } != AsyncCallState::Init as u32 {
            self.base.pending(self.id, cx.waker().clone(), PendingReason::RequestNotReady);
            return Poll::Pending;
        }
        Poll::Ready(())
    }
}

struct CompRingFuture {
    id: usize,
    base: Arc<AsyncCall>,
    buf: Arc<AsyncCallBuffer>,
}

impl Future for CompRingFuture {
    type Output = u32;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        // TODO: FIX ME, will error in mutil-cores
        let comp_ring_tail = self.buf.read_comp_ring_tail();
        if self.buf.completion_count(comp_ring_tail).unwrap() == self.buf.comp_capacity {
            self.base.pending(self.id, cx.waker().clone(), PendingReason::CompBufferFull);
            return Poll::Pending;
        }
        Poll::Ready(comp_ring_tail)
    }
}

impl AsynccallFuture {
    pub async fn polling(id:usize, base: Arc<AsyncCall>, buf: Arc<AsyncCallBuffer>) {
        info!("entry {} worker GO!", id);
        while base.thread.state() != ThreadState::Dead {
            Self::req_ring_available(id, &base, &buf).await;
            let res = {
                let (args, syscall_id) = {
                    let req = buf.req_entry_at(id as u32);
                    (
                        [req.arg0 as usize, req.arg1 as usize, req.arg2 as usize, 
                            req.arg3 as usize, req.arg4 as usize, req.arg5 as usize], 
                        req.syscall_id
                    )
                };
                let future = base.do_async_call(syscall_id as usize, args);
                pin_mut!(future);
                future.await
            };
            let comp_ring_tail = Self::comp_ring_available(id, &base, &buf).await;
            let mut entry = buf.req_entry_at_mut(id as u32);
            *buf.comp_entry_at(comp_ring_tail) =
                CompletionRingEntry::new(entry.user_data, res);
            buf.write_comp_ring_tail(comp_ring_tail + 1);
            entry.flags = AsyncCallState::Done as u32;
            buf.submit_req_ring();
        }
    }

    fn req_ring_available(id:usize, base: &Arc<AsyncCall>, buf: &Arc<AsyncCallBuffer>) 
        -> impl Future<Output = ()> 
    {
        ReqRingFuture {
            id,
            base: base.clone(),
            buf: buf.clone(),
        }
    }

    fn comp_ring_available(id:usize, base: &Arc<AsyncCall>, buf: &Arc<AsyncCallBuffer>) 
        -> impl Future<Output = u32> 
    {
        CompRingFuture {
            id: id,
            base: base.clone(),
            buf: buf.clone(),
        }
    }
}

// impl Future for AsynccallFuture<'_> {
//     type Output = ();
//     fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
//         info!("worker {} working !", self.id);
//         while self.base.thread.state() != ThreadState::Dead {
//             while unsafe { core::ptr::read_volatile(&self.entry.flags) } != AsyncCallState::Init as u32 {
//                 self.pending(cx.waker().clone(), PendingReason::RequestNotReady);
//                 info!("worker {} no work to do", self.id);
//                 return Poll::Pending;
//             }
//             let res = {
//                 let future = self.base.do_async_call(self.entry);
//                 pin_mut!(future);
//                 if let Poll::Ready(res) = future.as_mut().poll(cx) {
//                     res
//                 } else {
//                     // no need to register waker
//                     return Poll::Pending;
//                 }
//             };
//             let comp_ring_tail = loop {
//                 // TODO: FIX ME, will error in mutil-cores
//                 let comp_ring_tail = self.buf.read_comp_ring_tail();
//                 if self.buf.completion_count(comp_ring_tail).unwrap() == self.buf.comp_capacity {
//                     self.pending(cx.waker().clone(), PendingReason::CompBufferFull);
//                     return Poll::Pending;
//                 }
//                 break comp_ring_tail;
//             };
//             *self.buf.comp_entry_at(comp_ring_tail) =
//                 CompletionRingEntry::new(self.entry.user_data, res);
//             self.buf.write_comp_ring_tail(comp_ring_tail + 1);
//             self.entry.flags = AsyncCallState::Done as u32;
//             self.buf.submit_req_ring();
//         }
//         Poll::Ready(())
//     }
// }

struct ReqCheckerFuture {
    base: Arc<AsyncCall>,
    buf: Arc<AsyncCallBuffer>,
    tail: u32,
}

impl ReqCheckerFuture {
    pub fn new(base: Arc<AsyncCall>, buf: Arc<AsyncCallBuffer>) -> Self {
        Self {
            base: base, 
            buf: buf,
            tail: 0,
        }
    }
}

impl Future for ReqCheckerFuture {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        if self.base.thread.state() == ThreadState::Dead {
            return Poll::Ready(());
        }
        let req_tail = self.buf.read_req_ring_tail();
        if self.tail != req_tail {
            self.base.wake_worker_wait_on_req_range(self.tail..req_tail);
            self.tail = req_tail; 
        }
        cx.waker().clone().wake();
        Poll::Pending     
    }
}

struct CompCheckerFuture {
    base: Arc<AsyncCall>,
    buf: Arc<AsyncCallBuffer>,
    head: u32,
}

impl CompCheckerFuture {
    pub fn new(base: Arc<AsyncCall>, buf: Arc<AsyncCallBuffer>) -> Self {
        Self {
            base: base, 
            buf: buf,
            head: 0,
        }
    }
}

impl Future for CompCheckerFuture {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        if self.base.thread.state() == ThreadState::Dead {
            return Poll::Ready(());
        }
        let comp_head = self.buf.read_comp_ring_head();
        if self.head != comp_head {
            self.base.wake_worker_wait_on_comp_count((comp_head - self.head) as usize);
            self.head = comp_head;
        }
        cx.waker().clone().wake();
        Poll::Pending
    }
}

