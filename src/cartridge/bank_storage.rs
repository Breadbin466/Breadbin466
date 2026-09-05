// =======================================================
// src/cartridge/bank_storage.rs — Binary bank storage
// =======================================================

use super::constants::{STORAGE_BANK_SIZE, STORAGE_MAX_ALLOWED_BANKS};

#[derive(Clone)] /* BankStorage normalises sparse CRT chip packets into fixed 8 KiB banks. Empty slots remain distinguishable from programmed banks so unusual images can omit bank numbers without shifting every later bank. */
pub struct BankStorage {
	banks: Vec<Option<Box<[u8; STORAGE_BANK_SIZE]>>>,
}

impl BankStorage {
	/* New storage starts sparse rather than allocating the maximum CRT bank space; banks appear only when a CHIP packet actually provides data. */
	pub fn new() -> Self {
		Self { banks: Vec::new() }
	}

	/* A CHIP packet may populate only the ROML or ROMH half represented by this storage bank. Copying at an explicit offset preserves partial packet placement and fills unspecified bytes with the erased value. */
	pub fn store_bank(&mut self, index: usize, data: &[u8], offset: usize) {
		if index >= STORAGE_MAX_ALLOWED_BANKS {
			return;
		}
		while self.banks.len() <= index {
			self.banks.push(None);
		}
		let bank = self.banks[index].get_or_insert_with(|| Box::new([0xFF; STORAGE_BANK_SIZE]));
		let len = data.len().min(STORAGE_BANK_SIZE - offset);
		bank[offset..offset + len].copy_from_slice(&data[..len]);
	}

	/* Direct lookup preserves the distinction between an absent CHIP packet and a populated bank whose bytes happen to be 0xFF. */
	pub fn get_bank(&self, index: usize) -> Option<&[u8; STORAGE_BANK_SIZE]> {
		self.banks.get(index).and_then(|b| b.as_deref())
	}

	pub fn get_bank_mut(&mut self, index: usize) -> Option<&mut [u8; STORAGE_BANK_SIZE]> {
		self.banks.get_mut(index).and_then(|b| b.as_deref_mut())
	}

	/* Resolved reads apply the mapper-facing mirroring rule while direct get_bank access remains available to code that must distinguish a missing packet. */
	pub fn get_resolved(&self, requested: usize) -> Option<&[u8; STORAGE_BANK_SIZE]> {
		let index = self.resolve_bank(requested)?;
		self.get_bank(index)
	}

	/* Mutable resolution uses the same mirroring rule as reads, ensuring writes target the bank that the mapper currently exposes. */
	pub fn get_resolved_mut(&mut self, requested: usize) -> Option<&mut [u8; STORAGE_BANK_SIZE]> {
		let index = self.resolve_bank(requested)?;
		self.get_bank_mut(index)
	}

	/* The populated count, rather than vector length, defines the mirroring period for sparse CRT images. */
	pub fn populated_len(&self) -> usize {
		self.banks.iter().filter(|bank| bank.is_some()).count()
	}

	/* Sparse images sometimes select a bank number that was not supplied. Resolution mirrors through the populated set rather than renumbering packets at load time, keeping mapper-visible bank values intact. */
	pub fn resolve_bank(&self, requested: usize) -> Option<usize> {
		if self.get_bank(requested).is_some() {
			return Some(requested);
		}
		let populated = self.populated_len();
		if populated == 0 {
			return None;
		}
		let ordinal = requested % populated;
		self.banks
			.iter()
			.enumerate()
			.filter(|(_, bank)| bank.is_some())
			.nth(ordinal)
			.map(|(index, _)| index)
	}

	/* Vector length reports the highest represented bank range, including deliberate holes used by sparse images. */
	pub fn len(&self) -> usize {
		self.banks.len()
	}
}