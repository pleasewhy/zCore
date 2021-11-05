#![allow(dead_code)]
#![allow(clippy::identity_op)]

use trapframe::TrapFrame;

use crate::context::TrapReason;

    pub const X86_INT_LOCAL_APIC_BASE: usize = 0xf0;
    pub const X86_INT_APIC_SPURIOUS: usize = X86_INT_LOCAL_APIC_BASE + 0x0;
    pub const X86_INT_APIC_TIMER: usize = X86_INT_LOCAL_APIC_BASE + 0x1;
    pub const X86_INT_APIC_ERROR: usize = X86_INT_LOCAL_APIC_BASE + 0x2;

    // ISA IRQ numbers
    pub const X86_ISA_IRQ_PIT: usize = 0;
    pub const X86_ISA_IRQ_KEYBOARD: usize = 1;
    pub const X86_ISA_IRQ_PIC2: usize = 2;
    pub const X86_ISA_IRQ_COM2: usize = 3;
    pub const X86_ISA_IRQ_COM1: usize = 4;
    pub const X86_ISA_IRQ_CMOSRTC: usize = 8;
    pub const X86_ISA_IRQ_MOUSE: usize = 12;
    pub const X86_ISA_IRQ_IDE: usize = 14;
}

pub use consts::*;

fn breakpoint() {
    panic!("\nEXCEPTION: Breakpoint");
}

#[no_mangle]
pub extern "C" fn trap_handler(tf: &mut TrapFrame) {
    trace!(
        "Interrupt: {:#x} @ CPU{}",
        tf.trap_num,
        super::cpu::cpu_id()
    );
    match TrapReason::from(tf.trap_num, tf.error_code) {
        TrapReason::HardwareBreakpoint | TrapReason::SoftwareBreakpoint => breakpoint(),
        TrapReason::PageFault(vaddr, flags) => crate::KHANDLER.handle_page_fault(vaddr, flags),
        TrapReason::Interrupt(vector) => crate::interrupt::handle_irq(vector),
        other => panic!("Unhandled trap {:x?} {:#x?}", other, tf),
    }
}
