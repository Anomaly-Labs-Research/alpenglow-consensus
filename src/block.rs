use crate::message::{AlpenGlowHash, Slot};

pub struct Shred {
    slot: Slot,
    slice_index: usize,
    last_slice_index: usize,
    shred_index: usize,
    data: Vec<u8>,
    merkle_path: Vec<AlpenGlowHash>,
    signature: AlpenGlowHash,
}

impl Shred {
    pub fn slot(&self) -> Slot {
        self.slot
    }

    pub fn slice_index(&self) -> usize {
        self.slice_index
    }

    pub fn last_slice_index(&self) -> usize {
        self.last_slice_index
    }

    pub fn shred_index(&self) -> usize {
        self.shred_index
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn merkle_path(&self) -> &[AlpenGlowHash] {
        &self.merkle_path
    }

    pub fn signature(&self) -> &AlpenGlowHash {
        &self.signature
    }
}

pub struct Slice {
    slot: Slot,
    slice_index: usize,
    last_slice_index: usize,
    merkle_root: AlpenGlowHash,
    data: Vec<u8>,
    signature: AlpenGlowHash,
}

impl Slice {
    pub fn slot(&self) -> Slot {
        self.slot
    }

    pub fn slice_index(&self) -> usize {
        self.slice_index
    }

    pub fn last_slice_index(&self) -> usize {
        self.last_slice_index
    }

    pub fn merkle_root(&self) -> &AlpenGlowHash {
        &self.merkle_root
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn signature(&self) -> &AlpenGlowHash {
        &self.signature
    }

    pub fn reconstruct_from_shreds(_shreds: &[Shred]) -> Self {
        todo!()
    }
}
