// =======================================================
// src/pla/and_plane.rs — MOS 906114-01 programmed AND plane
// =======================================================

use super::signals::PlaInputSignals;

/* ProductTerms mirrors the programmed AND terms of the C64 PLA. The missing term numbers are unused by the original fuse map, so the historical numbering is preserved for comparison with published equations. */
#[derive(Debug, Clone, Copy)]
pub struct ProductTerms {
	pub p0: bool,
	pub p1: bool,
	pub p2: bool,
	pub p3: bool,
	pub p4: bool,
	pub p5: bool,
	pub p6: bool,
	pub p7: bool,
	pub p9: bool,
	pub p10: bool,
	pub p11: bool,
	pub p12: bool,
	pub p13: bool,
	pub p14: bool,
	pub p15: bool,
	pub p16: bool,
	pub p17: bool,
	pub p18: bool,
	pub p19: bool,
	pub p20: bool,
	pub p21: bool,
	pub p22: bool,
	pub p23: bool,
	pub p24: bool,
	pub p25: bool,
	pub p26: bool,
	pub p27: bool,
	pub p28: bool,
	pub p30: bool,
	pub p31: bool,
}

/* Each expression is one programmed product term. Keeping the equations explicit makes address, bus-owner and cartridge-line dependencies auditable against the published 906114-01 matrix (C64-PLA-DISSECTED-2012, Appendix A). */
#[inline(always)]
pub fn evaluate_and_plane(i: PlaInputSignals) -> ProductTerms {
	let cpu_bus = i.aec;
	let vic_bus = !i.aec;
	let read = i.rw;
	let write = !i.rw;
	let game_asserted = !i.game_n;
	let exrom_asserted = !i.exrom_n;
	let cas_asserted = !i.cas_n;

	ProductTerms {
		p0: i.loram && i.hiram
			&& i.a15 && !i.a14 && i.a13
			&& cpu_bus && read && i.game_n,

		p1: i.hiram
			&& i.a15 && i.a14 && i.a13
			&& cpu_bus && read && i.game_n,

		p2: i.hiram
			&& i.a15 && i.a14 && i.a13
			&& cpu_bus && read && exrom_asserted && game_asserted,

		p3: i.hiram && !i.charen
			&& i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && read && i.game_n,

		p4: i.loram && !i.charen
			&& i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && read && i.game_n,

		p5: i.hiram && !i.charen
			&& i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && read && exrom_asserted && game_asserted,

		p6: i.va14_n && !i.va13 && i.va12
			&& vic_bus && i.game_n,

		p7: i.va14_n && !i.va13 && i.va12
			&& vic_bus && exrom_asserted && game_asserted,

		p9: i.hiram && i.charen
			&& i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && i.ba && read && i.game_n,

		p10: i.hiram && i.charen
			&& i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && write && i.game_n,

		p11: i.loram && i.charen
			&& i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && i.ba && read && i.game_n,

		p12: i.loram && i.charen
			&& i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && write && i.game_n,

		p13: i.hiram && i.charen
			&& i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && i.ba && read && exrom_asserted && game_asserted,

		p14: i.hiram && i.charen
			&& i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && write && exrom_asserted && game_asserted,

		p15: i.loram && i.charen
			&& i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && i.ba && read && exrom_asserted && game_asserted,

		p16: i.loram && i.charen
			&& i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && write && exrom_asserted && game_asserted,

		p17: i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && i.ba && read && i.exrom_n && game_asserted,

		p18: i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && write && i.exrom_n && game_asserted,

		p19: i.loram && i.hiram
			&& i.a15 && !i.a14 && !i.a13
			&& cpu_bus && read && exrom_asserted,

		p20: i.a15 && !i.a14 && !i.a13
			&& cpu_bus && i.exrom_n && game_asserted,

		p21: i.hiram
			&& i.a15 && !i.a14 && i.a13
			&& cpu_bus && read && exrom_asserted && game_asserted,

		p22: i.a15 && i.a14 && i.a13
			&& cpu_bus && i.exrom_n && game_asserted,

		p23: i.va13 && i.va12
			&& vic_bus && i.exrom_n && game_asserted,

		p24: !i.a15 && !i.a14 && i.a12
			&& i.exrom_n && game_asserted,

		p25: !i.a15 && !i.a14 && i.a13
			&& i.exrom_n && game_asserted,

		p26: !i.a15 && i.a14
			&& i.exrom_n && game_asserted,

		p27: i.a15 && !i.a14 && i.a13
			&& i.exrom_n && game_asserted,

		p28: i.a15 && i.a14 && !i.a13 && !i.a12
			&& i.exrom_n && game_asserted,

		p30: cas_asserted,

		p31: i.cas_n
			&& i.a15 && i.a14 && !i.a13 && i.a12
			&& cpu_bus && write,
	}
}