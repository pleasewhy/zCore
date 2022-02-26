use riscv::register::scause;
use trapframe::TrapFrame;

use crate::context::TrapReason;

fn breakpoint(sepc: &mut usize) {
    info!("Exception::Breakpoint: A breakpoint set @0x{:x} ", sepc);

    // sepc 为触发中断指令 ebreak 的地址
    // 防止无限循环中断，让 sret 返回时跳转到 sepc 的下一条指令地址
    *sepc += 2
}

pub(super) fn super_timer() {
    super::timer::timer_set_next();
    crate::timer::timer_tick();
}

pub(super) fn super_soft() {
    super::sbi::clear_ipi();
    info!("Interrupt::SupervisorSoft!");
}

#[no_mangle]
pub extern "C" fn trap_handler(tf: &mut TrapFrame) {
    // log::warn!("in trap handler");
    let scause = scause::read();
    match TrapReason::from(scause) {
        TrapReason::SoftwareBreakpoint => breakpoint(&mut tf.sepc),
        TrapReason::PageFault(vaddr, flags) => {
            // log::warn!("sepc={:x}", riscv::register::sepc::read());
            // log::warn!("sstatus.spp={:?}", riscv::register::sstatus::read().spp());
            crate::KHANDLER.handle_page_fault(vaddr, flags)
        }
        TrapReason::Interrupt(vector) => crate::interrupt::handle_irq(vector),
        other => panic!("Undefined trap: {:x?} {:#x?}", other, tf),
    }
}
