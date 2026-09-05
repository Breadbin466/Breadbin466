// =======================================================
// src/fdd1541/nib.rs — NIB disk image encoding and decoding
// =======================================================

pub use super::nbz::{decode_nbz, encode_nbz};
use super::{d64, g64};
use crate::fdd1541::constants::{
	NIB_HALF_TRACK_COUNT, NIB_HEADER_LENGTH, NIB_SIGNATURE, NIB_TRACK_BYTES_MAX,
	NIB_TRACK_BYTES_MIN, NIB_TRACK_LENGTH,
};

/* A NIB capture stores a fixed 8192-byte observation for each recorded head position. The drive model needs one circular revolution instead, so decoding recovers the most strongly repeated period that remains physically plausible for the recorded speed zone. */
pub struct NibMedium {
	pub rings: Vec<Vec<u8>>,
	pub zones: Vec<u8>,
}

#[derive(Clone, Copy)]
struct OrbitEstimate {
	span: usize,
	agreements: usize,
	observations: usize,
}

/* The catalogue begins at byte 0x10 and contains pairs of half-track position and speed-zone code. Capture blocks follow the 256-byte catalogue in the same order as its populated entries. */
pub fn decode_nib(container_bytes: &[u8]) -> Option<NibMedium> {
	if container_bytes.len() < NIB_HEADER_LENGTH
		|| container_bytes.get(..NIB_SIGNATURE.len()) != Some(NIB_SIGNATURE)
	{
		return None;
	}

	let mut catalogue = Vec::new();
	let mut cursor = 0x10usize;
	while cursor + 1 < NIB_HEADER_LENGTH {
		let head_code = container_bytes[cursor];
		if head_code == 0 {
			break;
		}
		if head_code < 2 {
			return None;
		}

		let slot = usize::from(head_code - 2);
		if slot >= NIB_HALF_TRACK_COUNT
			|| catalogue.iter().any(|(known_slot, _)| *known_slot == slot)
		{
			return None;
		}

		catalogue.push((slot, container_bytes[cursor + 1] & 0x03));
		cursor += 2;
	}

	if catalogue.is_empty() {
		return None;
	}

	let payload_bytes = catalogue.len().checked_mul(NIB_TRACK_LENGTH)?;
	let required_bytes = NIB_HEADER_LENGTH.checked_add(payload_bytes)?;
	if container_bytes.len() < required_bytes {
		return None;
	}

	let mut rings = vec![Vec::new(); NIB_HALF_TRACK_COUNT];
	let mut zones = vec![0; NIB_HALF_TRACK_COUNT];
	for (ordinal, (slot, zone_code)) in catalogue.into_iter().enumerate() {
		let capture_begin = NIB_HEADER_LENGTH + ordinal * NIB_TRACK_LENGTH;
		let capture_end = capture_begin + NIB_TRACK_LENGTH;
		let capture_window = &container_bytes[capture_begin..capture_end];
		rings[slot] = recover_orbit(capture_window, zone_code);
		zones[slot] = zone_code;
	}

	Some(NibMedium { rings, zones })
}

/* Serialisation preserves every supplied circular byte stream verbatim and repeats it only as necessary to fill the fixed NIB capture window. */
pub fn encode_nib(rings: &[Vec<u8>], zones: &[u8]) -> Option<Vec<u8>> {
	let occupied_slots: Vec<usize> = rings
		.iter()
		.enumerate()
		.filter_map(|(slot, ring)| (!ring.is_empty()).then_some(slot))
		.collect();
	if occupied_slots.is_empty() || occupied_slots.len() > NIB_HALF_TRACK_COUNT {
		return None;
	}

	let payload_bytes = occupied_slots.len().checked_mul(NIB_TRACK_LENGTH)?;
	let total_bytes = NIB_HEADER_LENGTH.checked_add(payload_bytes)?;
	let mut container_bytes = vec![0u8; total_bytes];
	container_bytes[..NIB_SIGNATURE.len()].copy_from_slice(NIB_SIGNATURE);
	container_bytes[NIB_SIGNATURE.len()] = 1;

	for (ordinal, slot) in occupied_slots.into_iter().enumerate() {
		let head_code = u8::try_from(slot.checked_add(2)?).ok()?;
		let catalogue_cursor = 0x10 + ordinal * 2;
		container_bytes[catalogue_cursor] = head_code;
		container_bytes[catalogue_cursor + 1] = zones.get(slot).copied().unwrap_or(0) & 0x03;

		let ring = rings.get(slot)?;
		let capture_begin = NIB_HEADER_LENGTH + ordinal * NIB_TRACK_LENGTH;
		for capture_cursor in 0..NIB_TRACK_LENGTH {
			container_bytes[capture_begin + capture_cursor] = ring[capture_cursor % ring.len()];
		}
	}

	Some(container_bytes)
}

/* At 300 RPM the 1541 sees five revolutions per second. Its four bit-cell rates therefore constrain a revolution to a narrow byte-count interval. The NIB window is deliberately longer than every legal interval, so the same magnetic circumference is observed again near the end of the capture. Recovering that circumference is a periodicity problem; sector layout, sync placement and the legality of the GCR payload are irrelevant. */
fn recover_orbit(capture_window: &[u8], zone_code: u8) -> Vec<u8> {
	if capture_window.len() != NIB_TRACK_LENGTH {
		return Vec::new();
	}

	let zone_slot = usize::from(zone_code & 0x03);
	let lower_span = NIB_TRACK_BYTES_MIN[zone_slot];
	let upper_span = NIB_TRACK_BYTES_MAX[zone_slot].min(capture_window.len().saturating_sub(1));
	if lower_span == 0 || lower_span > upper_span {
		return capture_window.to_vec();
	}

	let preferred_span = lower_span + (upper_span - lower_span) / 2;
	let mut strongest = OrbitEstimate {
		span: preferred_span,
		agreements: 0,
		observations: 1,
	};

	for probe_span in lower_span..=upper_span {
		let observation_count = capture_window.len() - probe_span;
		let agreement_count = (0..observation_count)
			.filter(|&probe_cursor| {
				capture_window[probe_cursor] == capture_window[probe_cursor + probe_span]
			})
			.count();
		let proposal = OrbitEstimate {
			span: probe_span,
			agreements: agreement_count,
			observations: observation_count,
		};
		if estimate_is_better(proposal, strongest, preferred_span) {
			strongest = proposal;
		}
	}

	let meaningful_repetition = strongest.agreements.saturating_mul(4) >= strongest.observations;
	let selected_span = if meaningful_repetition {
		strongest.span
	} else {
		preferred_span
	};
	capture_window[..selected_span].to_vec()
}

fn estimate_is_better(
	proposal: OrbitEstimate,
	incumbent: OrbitEstimate,
	preferred_span: usize,
) -> bool {
	let proposal_weight = proposal.agreements.saturating_mul(incumbent.observations);
	let incumbent_weight = incumbent.agreements.saturating_mul(proposal.observations);
	if proposal_weight != incumbent_weight {
		return proposal_weight > incumbent_weight;
	}

	proposal.span.abs_diff(preferred_span) < incumbent.span.abs_diff(preferred_span)
}

pub(crate) fn create_formatted() -> Vec<u8> {
	let logical_disk = d64::create_formatted();
	let occupied_cylinders = 35usize;
	let mut container_bytes = vec![0u8; NIB_HEADER_LENGTH + occupied_cylinders * NIB_TRACK_LENGTH];
	container_bytes[..NIB_SIGNATURE.len()].copy_from_slice(NIB_SIGNATURE);
	container_bytes[NIB_SIGNATURE.len()] = 1;

	for cylinder_number in 1..=35u8 {
		let slot = (usize::from(cylinder_number) - 1) * 2;
		let catalogue_cursor = 0x10 + (usize::from(cylinder_number) - 1) * 2;
		container_bytes[catalogue_cursor] = (slot + 2) as u8;
		container_bytes[catalogue_cursor + 1] = g64::standard_density(cylinder_number);

		let ring = g64::formatted_track(&logical_disk, cylinder_number);
		let capture_begin =
			NIB_HEADER_LENGTH + (usize::from(cylinder_number) - 1) * NIB_TRACK_LENGTH;
		for capture_cursor in 0..NIB_TRACK_LENGTH {
			container_bytes[capture_begin + capture_cursor] = ring[capture_cursor % ring.len()];
		}
	}

	container_bytes
}

pub(crate) fn create_formatted_nbz() -> Vec<u8> {
	encode_nbz(&create_formatted()).unwrap_or_default()
}