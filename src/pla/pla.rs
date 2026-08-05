// =======================================================
// src/pla/pla.rs — MOS 906114-01 combinational logic array
// =======================================================

use super::and_plane::evaluate_and_plane;
use super::or_plane::evaluate_or_plane;
use super::signals::{PlaInputSignals, PlaOutputSignals};

/* The physical PLA is purely combinational: selected input polarities activate programmed product terms in the AND plane, and the OR plane combines those terms into eight active-low outputs (C64-PLA-DISSECTED-2012, logic equations). */
#[inline(always)]
pub fn evaluate(input: PlaInputSignals) -> PlaOutputSignals {
	let product_terms = evaluate_and_plane(input);
	evaluate_or_plane(product_terms)
}