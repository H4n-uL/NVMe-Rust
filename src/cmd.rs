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

// I/O Command Opcodes
const OPCODE_FLUSH: u8 = 0x00;
const OPCODE_WRITE: u8 = 0x01;
const OPCODE_READ: u8 = 0x02;
const OPCODE_WRITE_UNCORRECTABLE: u8 = 0x04;
const OPCODE_COMPARE: u8 = 0x05;
const OPCODE_WRITE_ZEROES: u8 = 0x08;
const OPCODE_DATASET_MANAGEMENT: u8 = 0x09;
const OPCODE_VERIFY: u8 = 0x0C;
const OPCODE_RESERVATION_REGISTER: u8 = 0x0D;
const OPCODE_RESERVATION_REPORT: u8 = 0x0E;
const OPCODE_RESERVATION_ACQUIRE: u8 = 0x11;
const OPCODE_RESERVATION_RELEASE: u8 = 0x15;
const OPCODE_COPY: u8 = 0x19;

// Admin Command Opcodes
const OPCODE_SUB_QUEUE_DELETE: u8 = 0x00;
const OPCODE_SUB_QUEUE_CREATE: u8 = 0x01;
const OPCODE_GET_LOG_PAGE: u8 = 0x02;
const OPCODE_COMP_QUEUE_DELETE: u8 = 0x04;
const OPCODE_COMP_QUEUE_CREATE: u8 = 0x05;
const OPCODE_IDENTIFY: u8 = 0x06;
const OPCODE_ABORT: u8 = 0x08;
const OPCODE_SET_FEATURES: u8 = 0x09;
const OPCODE_GET_FEATURES: u8 = 0x0A;
const OPCODE_ASYNC_EVENT_REQUEST: u8 = 0x0C;
const OPCODE_NAMESPACE_MANAGEMENT: u8 = 0x0D;
const OPCODE_FIRMWARE_COMMIT: u8 = 0x10;
const OPCODE_FIRMWARE_IMAGE_DOWNLOAD: u8 = 0x11;
const OPCODE_DEVICE_SELF_TEST: u8 = 0x14;
const OPCODE_NAMESPACE_ATTACHMENT: u8 = 0x15;
const OPCODE_KEEP_ALIVE: u8 = 0x18;
const OPCODE_DIRECTIVE_SEND: u8 = 0x19;
const OPCODE_DIRECTIVE_RECEIVE: u8 = 0x1A;
const OPCODE_VIRTUALIZATION_MANAGEMENT: u8 = 0x1C;
const OPCODE_NVME_MI_SEND: u8 = 0x1D;
const OPCODE_NVME_MI_RECEIVE: u8 = 0x1E;
const OPCODE_DOORBELL_BUFFER_CONFIG: u8 = 0x7C;
const OPCODE_FORMAT_NVM: u8 = 0x80;
const OPCODE_SECURITY_SEND: u8 = 0x81;
const OPCODE_SECURITY_RECEIVE: u8 = 0x82;
const OPCODE_SANITIZE: u8 = 0x84;

#[derive(Debug, Clone, Copy)]
pub(crate) enum LogPageId {
    SupportedLogPages = 0x00,
    ErrorInformation = 0x01,
    SmartHealth = 0x02,
    FirmwareSlot = 0x03,
    ChangedNamespaceList = 0x04,
    CommandsSupportedAndEffects = 0x05,
    DeviceSelfTest = 0x06,
    TelemetryHostInitiated = 0x07,
    TelemetryControllerInitiated = 0x08,
    EnduranceGroupInformation = 0x09,
    PredictableLatencyPerNvmSet = 0x0A,
    PredictableLatencyEventAggregate = 0x0B,
    AsymmetricNamespaceAccess = 0x0C,
    PersistentEventLog = 0x0D,
    LbaStatusInformation = 0x0E,
    EnduranceGroupEventAggregate = 0x0F,
    MediaUnitStatus = 0x10,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FeatureId {
    Arbitration = 0x01,
    PowerManagement = 0x02,
    LbaRangeType = 0x03,
    TemperatureThreshold = 0x04,
    ErrorRecovery = 0x05,
    VolatileWriteCache = 0x06,
    NumberOfQueues = 0x07,
    InterruptCoalescing = 0x08,
    InterruptVectorConfig = 0x09,
    WriteAtomicityNormal = 0x0A,
    AsyncEventConfig = 0x0B,
    AutonomousPowerState = 0x0C,
    HostMemBuffer = 0x0D,
    Timestamp = 0x0E,
    KeepAliveTimer = 0x0F,
    HostControlledThermal = 0x10,
    NonOperationalPowerState = 0x11,
    // NVMe 2.3 specific features
    PredictableLatencyModeConfig = 0x13,
    PredictableLatencyModeWindow = 0x14,
    LbaStatusInformationAttributes = 0x15,
    HostBehaviorSupport = 0x16,
    SanitizeConfig = 0x17,
    EnduranceGroupEventConfig = 0x18,
}

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

    pub fn get_log_page(
        cmd_id: u16,
        address: usize,
        log_id: LogPageId,
        num_dwords: u32,
        offset: u64,
    ) -> Self {
        Self {
            opcode: OPCODE_GET_LOG_PAGE,
            cmd_id,
            data_ptr: [address as u64, 0],
            cmd_10: ((num_dwords - 1) << 16) | (log_id as u32),
            cmd_11: (offset >> 32) as u32,
            cmd_12: offset as u32,
            ..Default::default()
        }
    }

    pub fn set_features(
        cmd_id: u16,
        feature_id: FeatureId,
        value: u32,
        save: bool,
    ) -> Self {
        let sv = if save { 0x80000000 } else { 0 };
        Self {
            opcode: OPCODE_SET_FEATURES,
            cmd_id,
            cmd_10: sv | (feature_id as u32),
            cmd_11: value,
            ..Default::default()
        }
    }

    pub fn get_features(
        cmd_id: u16,
        feature_id: FeatureId,
        sel: u8,
    ) -> Self {
        Self {
            opcode: OPCODE_GET_FEATURES,
            cmd_id,
            cmd_10: ((sel as u32) << 8) | (feature_id as u32),
            ..Default::default()
        }
    }

    pub fn abort(cmd_id: u16, sqid: u16, cid: u16) -> Self {
        Self {
            opcode: OPCODE_ABORT,
            cmd_id,
            cmd_10: ((cid as u32) << 16) | (sqid as u32),
            ..Default::default()
        }
    }

    pub fn async_event_request(cmd_id: u16) -> Self {
        Self {
            opcode: OPCODE_ASYNC_EVENT_REQUEST,
            cmd_id,
            ..Default::default()
        }
    }

    pub fn keep_alive(cmd_id: u16) -> Self {
        Self {
            opcode: OPCODE_KEEP_ALIVE,
            cmd_id,
            ..Default::default()
        }
    }

    pub fn namespace_management(
        cmd_id: u16,
        ns_id: u32,
        sel: u8,
        address: usize,
    ) -> Self {
        Self {
            opcode: OPCODE_NAMESPACE_MANAGEMENT,
            cmd_id,
            ns_id,
            data_ptr: [address as u64, 0],
            cmd_10: sel as u32,
            ..Default::default()
        }
    }

    pub fn namespace_attachment(
        cmd_id: u16,
        ns_id: u32,
        sel: u8,
        address: usize,
    ) -> Self {
        Self {
            opcode: OPCODE_NAMESPACE_ATTACHMENT,
            cmd_id,
            ns_id,
            data_ptr: [address as u64, 0],
            cmd_10: sel as u32,
            ..Default::default()
        }
    }

    pub fn firmware_image_download(
        cmd_id: u16,
        address: usize,
        num_dwords: u32,
        offset: u32,
    ) -> Self {
        Self {
            opcode: OPCODE_FIRMWARE_IMAGE_DOWNLOAD,
            cmd_id,
            data_ptr: [address as u64, 0],
            cmd_10: (num_dwords - 1),
            cmd_11: offset,
            ..Default::default()
        }
    }

    pub fn firmware_commit(
        cmd_id: u16,
        slot: u8,
        action: u8,
        bpid: u8,
    ) -> Self {
        Self {
            opcode: OPCODE_FIRMWARE_COMMIT,
            cmd_id,
            cmd_10: ((bpid as u32) << 31) | ((action as u32) << 3) | (slot as u32),
            ..Default::default()
        }
    }

    pub fn format_nvm(
        cmd_id: u16,
        ns_id: u32,
        lbaf: u8,
        mset: u8,
        pi: u8,
        pil: u8,
        ses: u8,
    ) -> Self {
        Self {
            opcode: OPCODE_FORMAT_NVM,
            cmd_id,
            ns_id,
            cmd_10: ((ses as u32) << 9) | ((pil as u32) << 8) | ((pi as u32) << 5) | ((mset as u32) << 4) | (lbaf as u32),
            ..Default::default()
        }
    }

    pub fn security_send(
        cmd_id: u16,
        ns_id: u32,
        address: usize,
        secp: u8,
        spsp: u16,
        tl: u32,
    ) -> Self {
        Self {
            opcode: OPCODE_SECURITY_SEND,
            cmd_id,
            ns_id,
            data_ptr: [address as u64, 0],
            cmd_10: ((secp as u32) << 24) | (spsp as u32),
            cmd_11: tl,
            ..Default::default()
        }
    }

    pub fn security_receive(
        cmd_id: u16,
        ns_id: u32,
        address: usize,
        secp: u8,
        spsp: u16,
        al: u32,
    ) -> Self {
        Self {
            opcode: OPCODE_SECURITY_RECEIVE,
            cmd_id,
            ns_id,
            data_ptr: [address as u64, 0],
            cmd_10: ((secp as u32) << 24) | (spsp as u32),
            cmd_11: al,
            ..Default::default()
        }
    }

    pub fn sanitize(
        cmd_id: u16,
        ns_id: u32,
        sanact: u8,
        ause: bool,
        owpass: u8,
        oipbp: bool,
        ndas: bool,
    ) -> Self {
        let mut cmd_10: u32 = sanact as u32;
        if ause { cmd_10 |= 1 << 3; }
        cmd_10 |= (owpass as u32) << 4;
        if oipbp { cmd_10 |= 1 << 8; }
        if ndas { cmd_10 |= 1 << 9; }

        Self {
            opcode: OPCODE_SANITIZE,
            cmd_id,
            ns_id,
            cmd_10,
            ..Default::default()
        }
    }

    // I/O Commands
    pub fn flush(cmd_id: u16, ns_id: u32) -> Self {
        Self {
            opcode: OPCODE_FLUSH,
            cmd_id,
            ns_id,
            ..Default::default()
        }
    }

    pub fn write_uncorrectable(
        cmd_id: u16,
        ns_id: u32,
        lba: u64,
        block_count: u16,
    ) -> Self {
        Self {
            opcode: OPCODE_WRITE_UNCORRECTABLE,
            cmd_id,
            ns_id,
            cmd_10: lba as u32,
            cmd_11: (lba >> 32) as u32,
            cmd_12: block_count as u32,
            ..Default::default()
        }
    }

    pub fn compare(
        cmd_id: u16,
        ns_id: u32,
        lba: u64,
        block_count: u16,
        data_ptr: [u64; 2],
    ) -> Self {
        Self {
            opcode: OPCODE_COMPARE,
            cmd_id,
            ns_id,
            data_ptr,
            cmd_10: lba as u32,
            cmd_11: (lba >> 32) as u32,
            cmd_12: block_count as u32,
            ..Default::default()
        }
    }

    pub fn write_zeroes(
        cmd_id: u16,
        ns_id: u32,
        lba: u64,
        block_count: u16,
        deac: bool,
    ) -> Self {
        let mut cmd_12 = block_count as u32;
        if deac { cmd_12 |= 1 << 25; }

        Self {
            opcode: OPCODE_WRITE_ZEROES,
            cmd_id,
            ns_id,
            cmd_10: lba as u32,
            cmd_11: (lba >> 32) as u32,
            cmd_12,
            ..Default::default()
        }
    }

    pub fn dataset_management(
        cmd_id: u16,
        ns_id: u32,
        address: usize,
        nr: u8,
        ad: bool,
        idw: bool,
        idr: bool,
    ) -> Self {
        let mut cmd_11: u32 = 0;
        if ad { cmd_11 |= 1 << 2; }
        if idw { cmd_11 |= 1 << 1; }
        if idr { cmd_11 |= 1; }

        Self {
            opcode: OPCODE_DATASET_MANAGEMENT,
            cmd_id,
            ns_id,
            data_ptr: [address as u64, 0],
            cmd_10: nr as u32,
            cmd_11,
            ..Default::default()
        }
    }

    pub fn verify(
        cmd_id: u16,
        ns_id: u32,
        lba: u64,
        block_count: u16,
    ) -> Self {
        Self {
            opcode: OPCODE_VERIFY,
            cmd_id,
            ns_id,
            cmd_10: lba as u32,
            cmd_11: (lba >> 32) as u32,
            cmd_12: block_count as u32,
            ..Default::default()
        }
    }

    pub fn copy(
        cmd_id: u16,
        ns_id: u32,
        address: usize,
        sdlba: u64,
        nr: u8,
        desc_format: u8,
    ) -> Self {
        Self {
            opcode: OPCODE_COPY,
            cmd_id,
            ns_id,
            data_ptr: [address as u64, 0],
            cmd_10: sdlba as u32,
            cmd_11: (sdlba >> 32) as u32,
            cmd_12: ((desc_format as u32) << 4) | (nr as u32),
            ..Default::default()
        }
    }
}
