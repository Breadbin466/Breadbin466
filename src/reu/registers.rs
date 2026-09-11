// =======================================================
// src/reu/registers.rs — MOS 8726 register interface
// =======================================================

/*
 * MOS 8726 register interface.
 *
 * Register accesses are kept separate from DMA execution so that CPU-visible
 * latches, live counters and command side effects remain easy to audit.  The
 * controller decodes five low address bits; unused positions therefore return
 * an undriven high value rather than aliasing implemented registers.
 */

use super::constants::{
	ADDRESS_CONTROL, ADDRESS_CONTROL_UNUSED_READ_HIGH, C64_ADDRESS_HIGH, C64_ADDRESS_LOW, COMMAND,
	COMMAND_EXECUTE, COMMAND_FF00_DISABLE, COMMAND_WRITABLE_MASK, INTERRUPT_MASK,
	INTERRUPT_UNUSED_READ_HIGH, REU_ADDRESS_BANK, REU_ADDRESS_HIGH, REU_ADDRESS_LOW, STATUS,
	STATUS_EVENT_MASK, TRANSFER_LENGTH_HIGH, TRANSFER_LENGTH_LOW, UNUSED_REGISTER_VALUE,
};
use super::reu::Reu;

impl Reu {
	/*
	 * Reading STATUS acknowledges all latched events and releases the REU IRQ
	 * request after returning the pre-acknowledge value to the CPU.
	 */
	pub fn read(&mut self, addr: u16) -> u8 {
		let index = (addr & 0x1F) as usize;
		if index >= self.regs.len() {
			return UNUSED_REGISTER_VALUE;
		}

		let value = match index {
			REU_ADDRESS_BANK => self.regs[REU_ADDRESS_BANK] | 0xF8,
			INTERRUPT_MASK => self.regs[INTERRUPT_MASK] | INTERRUPT_UNUSED_READ_HIGH,
			ADDRESS_CONTROL => self.regs[ADDRESS_CONTROL] | ADDRESS_CONTROL_UNUSED_READ_HIGH,
			_ => self.regs[index],
		};

		if index == STATUS {
			self.regs[STATUS] &= !STATUS_EVENT_MASK;
			self.irq_pending = false;
		}
		value
	}

	/*
	 * Address and length writes update the Autoload shadows immediately.  A
	 * command with EXECUTE set begins at once when FF00 triggering is disabled;
	 * otherwise the controller remains armed until the designated CPU access.
	 */
	pub fn write(&mut self, addr: u16, value: u8) {
		let index = (addr & 0x1F) as usize;
		if index >= self.regs.len() {
			return;
		}

		match index {
			STATUS => {}
			COMMAND => {
				self.regs[COMMAND] = value & COMMAND_WRITABLE_MASK;
				self.waiting_ff00 = false;
				if self.regs[COMMAND] & COMMAND_EXECUTE != 0 {
					if self.regs[COMMAND] & COMMAND_FF00_DISABLE != 0 {
						self.start_dma();
					} else {
						self.waiting_ff00 = true;
					}
				}
			}
			C64_ADDRESS_LOW | C64_ADDRESS_HIGH => {
				let shift = (index - C64_ADDRESS_LOW) * 8;
				self.shadow_c64_addr = (self.shadow_c64_addr & !(0xFFu16 << shift))
					| (u16::from(value) << shift);
				self.regs[C64_ADDRESS_LOW] = self.shadow_c64_addr as u8;
				self.regs[C64_ADDRESS_HIGH] = (self.shadow_c64_addr >> 8) as u8;
			}
			REU_ADDRESS_LOW | REU_ADDRESS_HIGH => {
				let shift = (index - REU_ADDRESS_LOW) * 8;
				self.shadow_reu_addr = (self.shadow_reu_addr & !(0xFFusize << shift))
					| (usize::from(value) << shift);
				self.regs[REU_ADDRESS_LOW] = self.shadow_reu_addr as u8;
				self.regs[REU_ADDRESS_HIGH] = (self.shadow_reu_addr >> 8) as u8;
			}
			REU_ADDRESS_BANK => {
				self.regs[REU_ADDRESS_BANK] = value | 0xF8;
				self.shadow_reu_addr = (self.shadow_reu_addr & 0xFFFF)
					| (usize::from(value | 0xF8) << 16);
			}
			TRANSFER_LENGTH_LOW | TRANSFER_LENGTH_HIGH => {
				let shift = (index - TRANSFER_LENGTH_LOW) * 8;
				self.shadow_len = (self.shadow_len & !(0xFFusize << shift))
					| (usize::from(value) << shift);
				self.regs[TRANSFER_LENGTH_LOW] = self.shadow_len as u8;
				self.regs[TRANSFER_LENGTH_HIGH] = (self.shadow_len >> 8) as u8;
			}
			INTERRUPT_MASK => {
				self.regs[INTERRUPT_MASK] = value | INTERRUPT_UNUSED_READ_HIGH;
				self.update_irq();
			}
			ADDRESS_CONTROL => {
				self.regs[ADDRESS_CONTROL] = value | ADDRESS_CONTROL_UNUSED_READ_HIGH;
			}
			_ => {}
		}
	}
}