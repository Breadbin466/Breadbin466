// =======================================================
// src/emulator/debugger_breakpoint.rs — Debugger breakpoint and watchpoint matching
// =======================================================

/*
 * Breadbin466 interactive debugger: breakpoint and watchpoint matching.
 *
 * Breakpoints deliberately describe only the observation that stops the
 * emulator.  They do not retain references to the machine or perform any
 * memory access themselves.  This keeps matching deterministic, allows the
 * same representation to cover instruction execution and bus observations,
 * and prevents debugger state from becoming another owner of emulated state.
 */

/* Execute breakpoints are tested at an instruction boundary. Read and write
 * watchpoints are tested after the motherboard has completed the corresponding
 * bus cycle, when the transferred value is known and all hardware side effects
 * have already occurred. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessKind {
	Execute,
	Read,
	Write,
}

/* An inclusive address interval represents both a single-address breakpoint
 * and a range watchpoint.  value is optional because execution breakpoints and
 * ordinary access watchpoints care only about the address, while a filtered
 * write watchpoint may additionally require the byte placed on the bus. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breakpoint {
	pub id: u32,
	pub kind: AccessKind,
	pub start: u16,
	pub end: u16,
	pub value: Option<u8>,
	pub enabled: bool,
}

impl Breakpoint {
	/* Matching is side-effect free and intentionally contains no debugger-wide
	 * policy.  The caller decides when an access is observable; this method only
	 * applies the stored kind, inclusive range and optional byte filter. */
	pub fn matches(&self, kind: AccessKind, addr: u16, value: Option<u8>) -> bool {
		self.enabled
			&& self.kind == kind
			&& (self.start..=self.end).contains(&addr)
			&& self
				.value
				.map(|expected| value == Some(expected))
				.unwrap_or(true)
	}
}