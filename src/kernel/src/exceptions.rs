use core::ptr::NonNull;

pub enum PageFaultReason {
    Permission,
    Security,
}

pub enum ExceptionKind {
    PageFault { ptr: NonNull<u8>, reason: PageFaultReason },
}

pub struct Exception {
    kind: ExceptionKind,
    ip: NonNull<u8>,
    sp: NonNull<u8>,
}
