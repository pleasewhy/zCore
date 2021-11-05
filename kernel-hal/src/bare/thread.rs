//! Thread spawning.

use core::future::Future;

hal_fn_impl! {
    impl mod crate::hal_fn::thread {
        fn spawn(future: impl Future<Output = ()> + Send + 'static) {
            executor::spawn(future);
        }

        fn block_on<T>(future: impl Future<Output = T> + Send /* + 'static*/) -> T
        {
            executor::Executor::block_on(future, || {})
        }

        fn block_on_with_wfi<T>(future: impl Future<Output = T> + Send /*+ 'static*/) -> T
        {
            executor::Executor::block_on(future, || { crate::hal_fn::interrupt::wait_for_interrupt() })
        }

        fn set_tid(_tid: u64, _pid: u64) {}

        fn get_tid() -> (u64, u64) {
            (0, 0)
        }
    }
}
