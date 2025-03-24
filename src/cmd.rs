#[derive(Debug, Default, Clone, Copy)]
#[repr(C, packed)]
pub(crate) struct Command {
    /// Opcode
    opcode: u8,
    /// Flags; FUSE (2 bits) | Reserved (4 bits) | PSDT (2 bits)
    flags: u8,
    /// Command ID
    cmd_id: u16,
    /// Namespace ID
    ns_id: u32,
    /// Reserved
    _rsvd: u64,
    /// Metadata pointer
    md_ptr: u64,
    /// Data pointer (PRP or SGL)
    data_ptr: [u64; 2],
    /// Command dword 10
    cmd_10: u32,
    /// Command dword 11
    cmd_11: u32,
    /// Command dword 12
    cmd_12: u32,
    /// Command dword 13
    cmd_13: u32,
    /// Command dword 14
    cmd_14: u32,
    /// Command dword 15
    cmd_15: u32,
}

#[derive(Debug)]
pub(crate) enum IdentifyType {
    Namespace(u32),
    Controller,
    NamespaceList(u32),
}

const OPCODE_READ: u8 = 2;
const OPCODE_WRITE: u8 = 1;
const OPCODE_IDENTIFY: u8 = 6;
const OPCODE_SUB_QUEUE_CREATE: u8 = 1;
const OPCODE_COMP_QUEUE_CREATE: u8 = 5;
const OPCODE_SUB_QUEUE_DELETE: u8 = 0;
const OPCODE_COMP_QUEUE_DELETE: u8 = 4;

impl Command {
    pub fn read_write(
        cmd_id: u16,
        ns_id: u32,
        lba: u64,
        block_count: u16,
        data_ptr: [u64; 2],
        is_write: bool,
    ) -> Self {
        Self {
            opcode: if is_write { OPCODE_WRITE } else { OPCODE_READ },
            cmd_id,
            ns_id,
            data_ptr,
            cmd_10: lba as u32,
            cmd_11: (lba >> 32) as u32,
            cmd_12: block_count as u32,
            ..Default::default()
        }
    }

    pub fn create_submission_queue(
        cmd_id: u16,
        queue_id: u16,
        address: usize,
        size: u16,
        cqueue_id: u16,
    ) -> Command {
        Self {
            opcode: OPCODE_SUB_QUEUE_CREATE,
            cmd_id,
            data_ptr: [address as u64, 0],
            cmd_10: ((size as u32) << 16) | (queue_id as u32),
            cmd_11: ((cqueue_id as u32) << 16) | 1,
            ..Default::default()
        }
    }

    pub fn create_completion_queue(
        cmd_id: u16,
        queue_id: u16,
        address: usize,
        size: u16,
    ) -> Command {
        Self {
            opcode: OPCODE_COMP_QUEUE_CREATE,
            cmd_id,
            data_ptr: [address as u64, 0],
            cmd_10: ((size as u32) << 16) | (queue_id as u32),
            cmd_11: 1,
            ..Default::default()
        }
    }

    pub fn delete_completion_queue(cmd_id: u16, queue_id: u16) -> Self {
        Self {
            opcode: OPCODE_COMP_QUEUE_DELETE,
            cmd_id,
            cmd_10: queue_id as u32,
            ..Default::default()
        }
    }

    pub fn delete_submission_queue(cmd_id: u16, queue_id: u16) -> Self {
        Self {
            opcode: OPCODE_SUB_QUEUE_DELETE,
            cmd_id,
            cmd_10: queue_id as u32,
            ..Default::default()
        }
    }

    pub fn identify(cmd_id: u16, address: usize, target: IdentifyType) -> Self {
        let (ns_id, cmd_10) = match target {
            IdentifyType::Namespace(id) => (id, 0),
            IdentifyType::Controller => (0, 1),
            IdentifyType::NamespaceList(base) => (base, 2),
        };

        Self {
            opcode: OPCODE_IDENTIFY,
            cmd_id,
            ns_id,
            data_ptr: [address as u64, 0],
            cmd_10,
            ..Default::default()
        }
    }
}
