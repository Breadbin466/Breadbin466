// =======================================================
// src/cartridge/crt_layout.rs — Shared helper for split/8000-A000 CRT layouts
// =======================================================

use super::bank_storage::BankStorage;

/* add_bank_split interprets the common CRT convention where one packet may contain adjacent ROML and ROMH halves, while packets loaded at A000 or above target ROMH directly. */
#[inline]
pub(super) fn add_bank_split(roml: &mut BankStorage, romh: &mut BankStorage, bank: usize, addr: u16, data: &[u8]) {
	if addr == 0x8000 {
		let len_l = data.len().min(8192);
		roml.store_bank(bank, &data[..len_l], 0);
		if data.len() > 8192 { romh.store_bank(bank, &data[8192..], 0); }
	} else if addr >= 0xA000 {
		romh.store_bank(bank, data, 0);
	} else {
		roml.store_bank(bank, data, 0);
	}
}