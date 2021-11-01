//! Linux LibOS
//! - run process and manage trap/interrupt/syscall
#![no_std]
#![feature(asm)]
#![deny(warnings, unused_must_use, missing_docs)]

extern crate alloc;
#[macro_use]
extern crate log;

use {
    alloc::{boxed::Box, string::String, sync::Arc, vec::Vec},
    cfg_if::cfg_if,
    core::{future::Future, pin::Pin},
    linux_object::{
        fs::{vfs::FileSystem, INodeExt},
        loader::LinuxElfLoader,
        process::ProcessExt,
        thread::{CurrentThreadExt, ThreadExt},
    },
    zircon_object::task::*,
};

cfg_if! {
    if #[cfg(target_arch = "x86_64")] {
        #[path = "arch/x86_64/mod.rs"]
        mod arch;
    } else if #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))] {
        #[path = "arch/riscv/mod.rs"]
        mod arch;
    }
}

pub use arch::{handle_syscall, handler_user_trap};

/// Create and run main Linux process
pub async fn run(
    args: Vec<String>,
    envs: Vec<String>,
    rootfs: impl Future<Output = Arc<dyn FileSystem>>,
) {
    let job = Job::root();
    info!("start init rootfs~");
    let rootfs = rootfs.await;
    info!("rootfs is ready!");
    let proc = Process::create_linux(&job, rootfs.clone()).await.unwrap();
    let thread = Thread::create_linux(&proc).unwrap();
    let loader = LinuxElfLoader {
        #[cfg(feature = "std")]
        syscall_entry: kernel_hal::context::syscall_entry as usize,
        #[cfg(not(feature = "std"))]
        syscall_entry: 0,
        stack_pages: 8,
        root_inode: rootfs.root_inode().await,
    };

    {
        let mut id = 0;
        let rust_dir = rootfs.root_inode().await.lookup("/").await.unwrap();
        trace!("run(), Rootfs: / ");
        while let Ok(name) = rust_dir.get_entry(id).await {
            id += 1;
            trace!("  {}", name);
        }
    }
    info!("args {:?}", args);
    let inode = rootfs.root_inode().await.lookup(&args[0]).await.unwrap();
    let data = inode.read_as_vec().await.unwrap();
    let path = args[0].clone();
    debug!("Linux process: {:?}", path);

    let pg_token = kernel_hal::vm::current_vmtoken();
    debug!("current pgt = {:#x}", pg_token);
    //调用zircon-object/src/task/thread.start设置好要执行的thread
    let (entry, sp) = loader
        .load(&proc.vmar(), &data, args, envs, path)
        .await
        .unwrap();

    thread
        .start(entry, sp, 0, 0, thread_fn)
        .expect("failed to start main thread");
}

/// The function of a new thread.
///
/// loop:
/// - wait for the thread to be ready
/// - get user thread context
/// - enter user mode
/// - handle trap/interrupt/syscall according to the return value
/// - return the context to the user thread
async fn new_thread(thread: CurrentThread) {
    loop {
        // wait
        let mut cx = thread.wait_for_run().await;
        if thread.state() == ThreadState::Dying {
            break;
        }

        // run
        trace!("go to user: {:#x?}", cx);
        kernel_hal::context::context_run(&mut cx);
        trace!("back from user: {:#x?}", cx);
        // handle trap/interrupt/syscall

        if let Err(err) = handler_user_trap(&thread, &mut cx).await {
            thread.exit_linux(err as i32);
        }

        thread.end_running(cx);
    }
}

fn thread_fn(thread: CurrentThread) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
    Box::pin(new_thread(thread))
}
