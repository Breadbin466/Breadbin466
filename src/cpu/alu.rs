// =======================================================
// src/cpu/alu.rs — ALU operations, flags, BCD
// =======================================================

use super::Cpu;
use super::{C_FLAG, D_FLAG, N_FLAG, V_FLAG, Z_FLAG};

/* Flag updates follow NMOS 6502 rules. Decimal ADC and SBC derive N, Z and V from the processor's intermediate binary/BCD behaviour rather than treating BCD as a post-processing format (MOS-6500-PROGRAMMING-1976, decimal arithmetic). */
impl Cpu {
	#[inline(always)]
	/* N and Z are replaced together from the supplied result while every unrelated status bit is preserved. */
	pub fn update_nz(&mut self, value: u8) {
		let is_zero = (value == 0) as u8;
		self.p = (self.p & !(Z_FLAG | N_FLAG)) | (is_zero << 1) | (value & N_FLAG);
	}

	/* Decimal ADC feeds the corrected low-digit carry into the high-digit sum before N and V are latched; the final high-digit correction changes only A and C (VISUAL6502-DECIMAL, Tests for ADC). */
	pub fn alu_adc(&mut self, value: u8) {
		let a = self.a;
		let m = value;
		let c = (self.p & C_FLAG) as u32;

		if (self.p & D_FLAG) != 0 {
			/* Add the low decimal digit first. Values ten through fifteen are invalid packed-BCD digits, so adding six converts the binary nibble sum into the decimal result and exposes a carry into the high digit. */
			let mut tmp = (a & 0x0f) as u32 + (m & 0x0f) as u32 + c;
			if tmp > 9 {
				tmp += 6;
			}
			tmp = if tmp <= 0x0f {
				(tmp & 0x0f) + (a & 0xf0) as u32 + (m & 0xf0) as u32
			} else {
				(tmp & 0x0f) + (a & 0xf0) as u32 + (m & 0xf0) as u32 + 0x10
			};
			/* NMOS Z follows the uncorrected binary sum. N and V follow the high-digit sum including the decimal carry from the low digit, before the high digit is corrected. */
			let bin = (a as u32) + (m as u32) + c;
			self.p &= !(Z_FLAG | N_FLAG | V_FLAG | C_FLAG);
			if (bin & 0xff) == 0 {
				self.p |= Z_FLAG;
			}
			if (tmp & 0x80) != 0 {
				self.p |= N_FLAG;
			}
			if ((a as u32 ^ tmp) & 0x80 != 0) && ((a as u32 ^ m as u32) & 0x80 == 0) {
				self.p |= V_FLAG;
			}
			/* A high digit above nine receives the corresponding +6 correction. Carry is set when the corrected packed-decimal result exceeds two digits. */
			if (tmp & 0x1f0) > 0x90 {
				tmp += 0x60;
			}
			if (tmp & 0xff0) > 0xf0 {
				self.p |= C_FLAG;
			}
			self.a = tmp as u8;
		} else {
			let tmp = (a as u32) + (m as u32) + c;
			self.p &= !(Z_FLAG | N_FLAG | V_FLAG | C_FLAG);
			let r = (tmp & 0xff) as u8;
			if r == 0 {
				self.p |= Z_FLAG;
			}
			if (r & 0x80) != 0 {
				self.p |= N_FLAG;
			}
			if ((a ^ m) & 0x80 == 0) && ((a ^ r) & 0x80 != 0) {
				self.p |= V_FLAG;
			}
			if tmp > 0xff {
				self.p |= C_FLAG;
			}
			self.a = r;
		}
	}

	/* SBC represents borrow as an inverted carry input; carry remains set when no borrow is required. */
	pub fn alu_sbc(&mut self, value: u8) {
		let a = self.a;
		let src = value;
		let c_in = if (self.p & C_FLAG) != 0 { 0u32 } else { 1u32 };
		let tmp = (a as u32).wrapping_sub(src as u32).wrapping_sub(c_in);

		self.p &= !(Z_FLAG | N_FLAG | V_FLAG | C_FLAG);
		let r = (tmp & 0xff) as u8;
		if r == 0 {
			self.p |= Z_FLAG;
		}
		if (r & 0x80) != 0 {
			self.p |= N_FLAG;
		}
		if ((a ^ r) & 0x80 != 0) && ((a ^ src) & 0x80 != 0) {
			self.p |= V_FLAG;
		}
		if tmp < 0x100 {
			self.p |= C_FLAG;
		}

		if (self.p & D_FLAG) != 0 {
			/* Subtract the low decimal digit first. A borrow out of the nibble is detected through bit 4 of the wrapping result; subtracting six converts the invalid binary digit back into packed BCD and propagates the borrow into the high digit. */
			let mut tmp_a: u32 = (a as u32 & 0xf)
				.wrapping_sub(src as u32 & 0xf)
				.wrapping_sub(c_in);
			if tmp_a & 0x10 != 0 {
				tmp_a = ((tmp_a.wrapping_sub(6)) & 0xf)
					| (a as u32 & 0xf0)
						.wrapping_sub(src as u32 & 0xf0)
						.wrapping_sub(0x10);
			} else {
				tmp_a = (tmp_a & 0xf) | (a as u32 & 0xf0).wrapping_sub(src as u32 & 0xf0);
			}
			/* A borrow from the high decimal digit requires the matching -6 correction in the upper nibble. Flags above were intentionally taken from the uncorrected binary subtraction, matching NMOS behaviour. */
			if tmp_a & 0x100 != 0 {
				tmp_a = tmp_a.wrapping_sub(0x60);
			}
			self.a = tmp_a as u8;
		} else {
			self.a = r;
		}
	}

	#[inline(always)]
	/* CMP-family operations perform an unsigned subtraction only for flags: C records no borrow while N and Z reflect the wrapped eight-bit difference. */
	pub fn alu_cmp(&mut self, reg: u8, value: u8) {
		let tmp = (reg as u32).wrapping_sub(value as u32);
		let r = (tmp & 0xff) as u8;
		let is_zero = (r == 0) as u8;
		self.p = (self.p & !(Z_FLAG | N_FLAG | C_FLAG))
			| (is_zero << 1)
			| (r & N_FLAG)
			| (((tmp < 0x100) as u8) * C_FLAG);
	}

	#[inline(always)]
	/* BIT derives Z from A AND operand, while N and V are copied directly from operand bits 7 and 6. */
	pub fn alu_bit(&mut self, value: u8) {
		let intersected = self.a & value;
		let is_zero = (intersected == 0) as u8;
		self.p = (self.p & !(Z_FLAG | 0xc0)) | (is_zero << 1) | (value & 0xc0);
	}

	#[inline(always)]
	/* ASL moves bit 7 into carry and shifts a zero into bit 0 before updating N and Z. */
	pub fn alu_asl(&mut self, value: u8) -> u8 {
		let res = value << 1;
		let is_zero = (res == 0) as u8;
		self.p = (self.p & !(C_FLAG | Z_FLAG | N_FLAG))
			| ((value >> 7) & C_FLAG)
			| (is_zero << 1)
			| (res & N_FLAG);
		res
	}

	#[inline(always)]
	/* LSR moves bit 0 into carry and shifts a zero into bit 7, which necessarily clears N. */
	pub fn alu_lsr(&mut self, value: u8) -> u8 {
		let res = value >> 1;
		let is_zero = (res == 0) as u8;
		self.p = (self.p & !(C_FLAG | Z_FLAG | N_FLAG))
			| (value & C_FLAG)
			| (is_zero << 1)
			| (res & N_FLAG);
		res
	}

	#[inline(always)]
	/* ROL shifts through the previous carry, then exposes the original bit 7 as the new carry. */
	pub fn alu_rol(&mut self, value: u8) -> u8 {
		let old_c = self.p & C_FLAG;
		let res = (value << 1) | old_c;
		let is_zero = (res == 0) as u8;
		self.p = (self.p & !(C_FLAG | Z_FLAG | N_FLAG))
			| ((value >> 7) & C_FLAG)
			| (is_zero << 1)
			| (res & N_FLAG);
		res
	}

	#[inline(always)]
	/* ROR shifts through the previous carry, then exposes the original bit 0 as the new carry. */
	pub fn alu_ror(&mut self, value: u8) -> u8 {
		let old_c = (self.p & C_FLAG) << 7;
		let res = (value >> 1) | old_c;
		let is_zero = (res == 0) as u8;
		self.p = (self.p & !(C_FLAG | Z_FLAG | N_FLAG))
			| (value & C_FLAG)
			| (is_zero << 1)
			| (res & N_FLAG);
		res
	}

	#[inline(always)]
	/* AXS is the undocumented (A AND X) minus immediate operation; it stores the wrapped difference in X and reports carry as no borrow. */
	pub fn alu_axs(&mut self, value: u8) {
		let src = self.a & self.x;
		let tmp = (src as u32).wrapping_sub(value as u32);
		self.x = tmp as u8;
		let is_zero = (self.x == 0) as u8;
		self.p = (self.p & !(N_FLAG | Z_FLAG | C_FLAG))
			| (((src >= value) as u8) * C_FLAG)
			| (is_zero << 1)
			| (self.x & N_FLAG);
	}

	#[inline(always)]
	/* ARR is an undocumented compound operation whose flags depend on internal rotate and decimal-adjust behaviour, so it cannot be expressed as a simple AND followed by ROR. */
	pub fn alu_arr(&mut self, value: u8) {
		let tmp = self.a & value;
		let carry_in = (self.p & C_FLAG) != 0;

		if (self.p & D_FLAG) != 0 {
			/* ARR first rotates the AND result through carry. Its decimal corrections are based on the pre-rotate digit patterns, which is why this path cannot reuse ADC or a normal ROR result. */
			let mut tmp2 = tmp as u32;
			tmp2 |= if carry_in { 0x100 } else { 0 };
			tmp2 >>= 1;
			self.p &= !(N_FLAG | Z_FLAG | V_FLAG | C_FLAG);
			if carry_in {
				self.p |= N_FLAG;
			}
			if (tmp2 & 0xff) == 0 {
				self.p |= Z_FLAG;
			}
			if (tmp2 as u8 ^ tmp) & 0x40 != 0 {
				self.p |= V_FLAG;
			}
			/* The low digit threshold uses the original bit 0 as the rotate contribution; crossing five requires the +6 packed-decimal correction. */
			if (tmp & 0x0f) as u32 + (tmp & 0x01) as u32 > 0x05 {
				tmp2 = (tmp2 & 0xf0) | ((tmp2 + 0x06) & 0x0f);
			}
			/* The high digit applies the equivalent threshold at $50. Crossing it adds $60 and sets carry, reflecting the undocumented NMOS decimal result. */
			if (tmp & 0xf0) as u32 + (tmp & 0x10) as u32 > 0x50 {
				tmp2 = (tmp2 & 0x0f) | ((tmp2 + 0x60) & 0xf0);
				self.p |= C_FLAG;
			}
			self.a = tmp2 as u8;
		} else {
			let mut tmp = tmp as u32;
			tmp |= if carry_in { 0x100 } else { 0 };
			tmp >>= 1;
			self.p &= !(N_FLAG | Z_FLAG | V_FLAG | C_FLAG);
			let r = tmp as u8;
			self.update_nz(r);
			if (tmp & 0x40) != 0 {
				self.p |= C_FLAG;
			}
			if ((tmp & 0x40) ^ ((tmp & 0x20) << 1)) != 0 {
				self.p |= V_FLAG;
			}
			self.a = r;
		}
	}
}