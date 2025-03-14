#[derive(Debug, Default)]
#[repr(C, packed)]
pub struct Command {
    opcode: u8,
    flags: u8,
    cmd_id: u16,
    namespace_id: u32,
    _reserved: u64,
    metadata_ptr: u64,
    data_ptr: [u64; 2],
    cmd_10: u32,
    cmd_11: u32,
    cmd_12: u32,
    cmd_13: u32,
    cmd_14: u32,
    cmd_15: u32,
}

#[derive(Debug)]
pub enum QueueType {
    Submission,
    Completion,
}

#[derive(Debug)]
pub enum IdentifyType {
    Namespace(u32),
    Controller,
    NamespaceList(u32),
}

const OPCODE_SUB_QUEUE_DELETE: u8 = 0;
const OPCODE_WRITE: u8 = 1;
const OPCODE_SUB_QUEUE_CREATE: u8 = 1;
const OPCODE_READ: u8 = 2;
const OPCODE_COMP_QUEUE_DELETE: u8 = 4;
const OPCODE_COMP_QUEUE_CREATE: u8 = 5;
const OPCODE_IDENTIFY: u8 = 6;

impl Command {
    pub fn read_write(
        cmd_id: u16,
        namespace_id: u32,
        lba: u64,
        block_count: u16,
        data_ptr: [u64; 2],
        is_write: bool,
    ) -> Self {
        Self {
            opcode: if is_write { OPCODE_WRITE } else { OPCODE_READ },
            cmd_id,
            namespace_id,
            data_ptr,
            cmd_10: lba as u32,
            cmd_11: (lba >> 32) as u32,
            cmd_12: block_count as u32,
            ..Default::default()
        }
    }

    pub fn create_queue(
        cmd_id: u16,
        queue_id: u16,
        address: usize,
        size: u16,
        target: QueueType,
        cqueue_id: Option<u16>,
    ) -> Command {
        let (opcode, cmd_11) = match target {
            QueueType::Submission => {
                let id = cqueue_id.unwrap_or(0);
                (OPCODE_SUB_QUEUE_CREATE, ((id as u32) << 16) | 1)
            }
            QueueType::Completion => (OPCODE_COMP_QUEUE_CREATE, 1),
        };

        Self {
            opcode,
            cmd_id,
            data_ptr: [address as u64, 0],
            cmd_10: ((size as u32) << 16) | (queue_id as u32),
            cmd_11,
            ..Default::default()
        }
    }

    pub fn delete_queue(cmd_id: u16, queue_id: u16, target: QueueType) -> Self {
        let opcode = match target {
            QueueType::Submission => OPCODE_SUB_QUEUE_DELETE,
            QueueType::Completion => OPCODE_COMP_QUEUE_DELETE,
        };

        Self {
            opcode,
            cmd_id,
            cmd_10: queue_id as u32,
            ..Default::default()
        }
    }

    pub fn identify(cmd_id: u16, address: usize, target: IdentifyType) -> Self {
        let (namespace_id, cmd_10) = match target {
            IdentifyType::Namespace(id) => (id, 0),
            IdentifyType::Controller => (0, 1),
            IdentifyType::NamespaceList(base) => (base, 2),
        };

        Self {
            opcode: OPCODE_IDENTIFY,
            cmd_id,
            namespace_id,
            data_ptr: [address as u64, 0],
            cmd_10,
            ..Default::default()
        }
    }
}
