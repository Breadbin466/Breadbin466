// =======================================================
// src/pla/or_plane.rs — MOS 906114-01 programmed OR plane
// =======================================================

use super::and_plane::ProductTerms;
use super::signals::PlaOutputSignals;

/* The OR plane groups product terms by output. CASRAM is special because many otherwise unrelated maps select DRAM, while each ROM or I/O output is driven by a smaller family of terms. */
#[inline(always)]
pub fn evaluate_or_plane(p: ProductTerms) -> PlaOutputSignals {
	let casram_asserted = p.p0
		|| p.p1
		|| p.p2
		|| p.p3
		|| p.p4
		|| p.p5
		|| p.p6
		|| p.p7
		|| p.p9
		|| p.p10
		|| p.p11
		|| p.p12
		|| p.p13
		|| p.p14
		|| p.p15
		|| p.p16
		|| p.p17
		|| p.p18
		|| p.p19
		|| p.p20
		|| p.p21
		|| p.p22
		|| p.p23
		|| p.p24
		|| p.p25
		|| p.p26
		|| p.p27
		|| p.p28
		|| p.p30;

	PlaOutputSignals {
		casram_n: casram_asserted,
		basic_n: !p.p0,
		kernal_n: !(p.p1 || p.p2),
		charom_n: !(p.p3 || p.p4 || p.p5 || p.p6 || p.p7),
		io_n: !(p.p9
			|| p.p10
			|| p.p11
			|| p.p12
			|| p.p13
			|| p.p14
			|| p.p15
			|| p.p16
			|| p.p17
			|| p.p18),
		roml_n: !(p.p19 || p.p20),
		romh_n: !(p.p21 || p.p22 || p.p23),
		grw_n: !p.p31,
	}
}