use std::proto::Protocol;

#[repr(packed)]
#[derive(Clone, Copy, Debug)]
pub struct PartitionProtoInfoMbr {
    pub boot: u8,
    pub chs_start: [u8; 3],
    pub ty: u8,
    pub chs_end: [u8; 3],
    pub start_lba: u32,
    pub lba_size: u32,
}

#[repr(packed)]
#[derive(Clone, Copy)]
pub struct PartitionProtoInfoGpt {
    pub part_ty_guid: [u8; 16],
    pub uniq_guid: [u8; 16],
    pub start_lba: u64,
    pub end_lba: u64,
    pub attrs: u64,
    pub name: [u16; 36],
    // reserved until end of block
}

#[repr(packed)]
#[derive(Clone, Copy)]
pub union PartitionProtoDataInfo {
    pub mbr: PartitionProtoInfoMbr,
    pub gpt: PartitionProtoInfoGpt,
}

#[repr(packed)]
pub struct PartitionProtoData {
    pub rev: u32,
    pub ty: u32,
    pub sys: u8,
    pub resv: [u8; 7],
    pub info: PartitionProtoDataInfo,
}

pub const PARTITION_INFO_PROTOCOL_REVISION: u32 = 0x1000;
pub const ESP_GUID: [u8; 16] = [0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x0, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b]; // c12a7328-f81f-11d2-bA4b-00a0c93ec93b
pub const LINUX_FS_GUID: [u8; 16] = [0xaf, 0x3d, 0xc6, 0xf, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d, 0xe4]; // 0fc63daf-8483-4772-8e79-3d69d8477de4
pub const REDOX_FS_GUID: [u8; 16] = [0xfd, 0x98, 0x78, 0x52, 0xe3, 0xff, 0xc2, 0x42, 0xe3, 0x96, 0x10, 0x5b, 0xa6, 0x3f, 0x5a, 0xbf]; // 527898fd-ffe3-42c2-96e3-bf5a3fa65b10

#[repr(u32)]
pub enum PartitionProtoDataTy {
    Other = 0,
    Mbr = 1,
    Gpt = 2,
}

pub struct PartitionProto(pub &'static mut PartitionProtoData);

impl Protocol<PartitionProtoData> for PartitionProto {
    fn guid() -> uefi::guid::Guid {
        uefi::guid::Guid(0x8cf2f62c, 0xbc9b, 0x4821, [0x80, 0x8d, 0xec, 0x9e, 0xc4, 0x21, 0xa1, 0xa0])
    }
    fn new(inner: &'static mut PartitionProtoData) -> Self {
        Self(inner)
    }
}
